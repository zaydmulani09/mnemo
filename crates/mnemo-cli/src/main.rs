use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use mnemo_core::models::*;
use prettytable::{format, row, Table};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

// ── Local response types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct StatsResponse {
    entity_count: i64,
    chunk_count: i64,
    node_count: usize,
    edge_count: usize,
    uptime_seconds: u64,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    entities: Vec<Entity>,
    #[allow(dead_code)]
    chunks: Vec<MemoryChunk>,
}

// GraphNode only derives Serialize on the server side; mirror locally for deserialization.
#[derive(Debug, Deserialize)]
struct GraphNeighbor {
    entity_id: Uuid,
    name: String,
    entity_type: EntityType,
    confidence: f32,
}

#[derive(Debug, Serialize)]
struct CliSearchRequest {
    query: String,
    limit: Option<i64>,
}

// ── CLI structure ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "mnemo", version = "0.1.0", about = "Local-first AI memory layer CLI")]
struct Cli {
    /// Server base URL
    #[arg(long, global = true, default_value = "http://localhost:8080")]
    server: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Ingest text into memory
    Ingest {
        /// Text to ingest
        text: String,
        /// Source label
        #[arg(long, default_value = "cli")]
        source: String,
        /// Optional session ID
        #[arg(long)]
        session: Option<String>,
    },
    /// Search memory for relevant context
    Search {
        /// Search query
        query: String,
        /// Max results
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Disable graph expansion
        #[arg(long)]
        no_graph: bool,
        /// Print raw context string instead of formatted output
        #[arg(long)]
        raw: bool,
    },
    /// List all entities
    Entities {
        /// Max results
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Filter by name
        #[arg(long)]
        search: Option<String>,
    },
    /// Show a single entity by ID
    Entity {
        /// Entity UUID
        id: Uuid,
        /// Also show graph neighbors
        #[arg(long)]
        neighbors: bool,
        /// Neighbor depth
        #[arg(long, default_value_t = 2)]
        depth: usize,
    },
    /// List memory chunks
    Chunks {
        /// Max results
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
    },
    /// Show a single chunk by ID
    Chunk {
        /// Chunk UUID
        id: Uuid,
    },
    /// Wipe all memory (asks for confirmation)
    Wipe {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Show memory statistics
    Stats,
    /// Show server and LLM health status
    Health,
    /// Show current server config
    Config,
}

// ── API client ───────────────────────────────────────────────────────────────

struct ApiClient {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl ApiClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap(),
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .send()
            .with_context(|| unreachable_msg(&self.base_url))?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            bail!("server error {}: {}", status, text);
        }
        serde_json::from_str(&text).context("failed to parse response")
    }

    fn post<B: Serialize, T: serde::de::DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(&url)
            .json(body)
            .send()
            .with_context(|| unreachable_msg(&self.base_url))?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            bail!("server error {}: {}", status, text);
        }
        serde_json::from_str(&text).context("failed to parse response")
    }

    #[allow(dead_code)]
    fn delete<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .send()
            .with_context(|| unreachable_msg(&self.base_url))?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            bail!("server error {}: {}", status, text);
        }
        serde_json::from_str(&text).context("failed to parse response")
    }

    fn delete_with_header<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        header_name: &str,
        header_value: &str,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .delete(&url)
            .header(header_name, header_value)
            .send()
            .with_context(|| unreachable_msg(&self.base_url))?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            bail!("server error {}: {}", status, text);
        }
        serde_json::from_str(&text).context("failed to parse response")
    }
}

fn unreachable_msg(url: &str) -> String {
    format!(
        "✗ Cannot connect to mnemo server at {} — is it running?",
        url
    )
}

// ── Spinner helper ───────────────────────────────────────────────────────────

fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

// ── Command handlers ─────────────────────────────────────────────────────────

fn cmd_ingest(client: &ApiClient, text: String, source: String, session: Option<String>) {
    let pb = spinner("Ingesting into memory...");
    let body = IngestRequest {
        content: text,
        source,
        session_id: session,
        metadata: None,
    };
    match client.post::<_, IngestResponse>("/ingest", &body) {
        Ok(resp) => {
            pb.finish_and_clear();
            println!("{}", "✓ Memory ingested".green().bold());
            println!("  Chunk ID:   {}", resp.chunk_id);
            println!("  Entities:   {} extracted", resp.entities_extracted);
            println!("  Relations:  {} extracted", resp.relations_extracted);
            println!("  Time:       {}ms", resp.processing_time_ms);
        }
        Err(e) => {
            pb.finish_and_clear();
            eprintln!("{} {}", "✗".red(), e);
            std::process::exit(1);
        }
    }
}

fn cmd_search(client: &ApiClient, query: String, limit: usize, no_graph: bool, raw: bool) {
    let pb = spinner("Retrieving from memory...");
    let body = RetrievalQuery {
        text: query.clone(),
        session_id: None,
        max_chunks: limit,
        max_entities: limit * 2,
        min_confidence: 0.0,
        include_graph: !no_graph,
        graph_depth: 2,
    };
    match client.post::<_, RetrievalResult>("/retrieve", &body) {
        Ok(result) => {
            pb.finish_and_clear();
            if raw {
                println!("{}", result.context_prompt);
                return;
            }
            if result.chunks.is_empty() && result.entities.is_empty() {
                println!("No memories found for this query.");
                return;
            }
            println!("=== Search Results for \"{}\" ===", query);
            println!();
            println!("ENTITIES ({})", result.entities.len());
            for e in &result.entities {
                println!(
                    "  • {} [{:?}] — confidence: {:.2}",
                    e.name, e.entity_type, e.confidence
                );
                if let Some(obj) = e.attributes.as_object() {
                    if !obj.is_empty() {
                        let attrs: Vec<String> =
                            obj.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
                        println!("    Attributes: {}", attrs.join(", "));
                    }
                }
            }
            println!();
            println!("RELATIONSHIPS ({})", result.relations.len());
            for r in &result.relations {
                println!(
                    "  • {} —[{}]→ {} (weight: {:.2})",
                    r.from_entity_id, r.relation_type, r.to_entity_id, r.weight
                );
            }
            println!();
            println!("MEMORIES ({})", result.chunks.len());
            for chunk in &result.chunks {
                let date = chunk.created_at.format("%Y-%m-%d").to_string();
                println!("  [{} | {}]", chunk.source, date);
                let preview = if chunk.content.len() > 200 {
                    format!("{}...", &chunk.content[..200])
                } else {
                    chunk.content.clone()
                };
                println!("  {}", preview);
                println!();
            }
        }
        Err(e) => {
            pb.finish_and_clear();
            eprintln!("{} {}", "✗".red(), e);
            std::process::exit(1);
        }
    }
}

fn cmd_entities(client: &ApiClient, limit: usize, search: Option<String>) {
    let entities: Vec<Entity> = if let Some(query) = search {
        let body = CliSearchRequest {
            query,
            limit: Some(limit as i64),
        };
        match client.post::<_, SearchResponse>("/search", &body) {
            Ok(r) => r.entities,
            Err(e) => {
                eprintln!("{} {}", "✗".red(), e);
                std::process::exit(1);
            }
        }
    } else {
        match client.get::<Vec<Entity>>(&format!("/entities?limit={}", limit)) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("{} {}", "✗".red(), e);
                std::process::exit(1);
            }
        }
    };

    if entities.is_empty() {
        println!("No entities stored yet.");
        return;
    }

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.set_titles(row!["ID", "Name", "Type", "Source Count"]);
    for e in &entities {
        table.add_row(row![e.id, e.name, format!("{:?}", e.entity_type), e.source_count]);
    }
    table.printstd();
}

fn cmd_entity(client: &ApiClient, id: Uuid, show_neighbors: bool, depth: usize) {
    let entity: Entity = match client.get(&format!("/entities/{}", id)) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{} {}", "✗".red(), e);
            std::process::exit(1);
        }
    };

    println!("Entity Details");
    println!("──────────────");
    println!("  ID:           {}", entity.id);
    println!("  Name:         {}", entity.name);
    println!("  Type:         {:?}", entity.entity_type);
    println!("  Confidence:   {:.2}", entity.confidence);
    println!("  Source Count: {}", entity.source_count);
    println!("  Aliases:      {}", entity.aliases.join(", "));
    println!("  Attributes:   {}", entity.attributes);
    println!(
        "  Created:      {}",
        entity.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "  Updated:      {}",
        entity.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

    if show_neighbors {
        let neighbors: Vec<GraphNeighbor> = match client
            .get(&format!("/entities/{}/neighbors?depth={}", id, depth))
        {
            Ok(n) => n,
            Err(e) => {
                eprintln!("{} {}", "✗".red(), e);
                std::process::exit(1);
            }
        };
        println!();
        println!("Neighbors (depth {})", depth);
        if neighbors.is_empty() {
            println!("  (none)");
        } else {
            let mut table = Table::new();
            table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
            table.set_titles(row!["ID", "Name", "Type", "Confidence"]);
            for n in &neighbors {
                table.add_row(row![
                    n.entity_id,
                    n.name,
                    format!("{:?}", n.entity_type),
                    format!("{:.2}", n.confidence)
                ]);
            }
            table.printstd();
        }
    }
}

fn cmd_chunks(client: &ApiClient, limit: usize, session: Option<String>) {
    let url = match &session {
        Some(s) => format!("/chunks?limit={}&session_id={}", limit, s),
        None => format!("/chunks?limit={}", limit),
    };
    let chunks: Vec<MemoryChunk> = match client.get(&url) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} {}", "✗".red(), e);
            std::process::exit(1);
        }
    };

    if chunks.is_empty() {
        println!("No chunks stored yet.");
        return;
    }

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.set_titles(row!["ID", "Source", "Session", "Created At", "Content"]);
    for c in &chunks {
        let preview = if c.content.len() > 50 {
            format!("{}...", &c.content[..50])
        } else {
            c.content.clone()
        };
        let session_str = c.session_id.as_deref().unwrap_or("-");
        table.add_row(row![
            c.id,
            c.source,
            session_str,
            c.created_at.format("%Y-%m-%d %H:%M"),
            preview
        ]);
    }
    table.printstd();
}

fn cmd_chunk(client: &ApiClient, id: Uuid) {
    let chunk: MemoryChunk = match client.get(&format!("/chunks/{}", id)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} {}", "✗".red(), e);
            std::process::exit(1);
        }
    };

    println!("Chunk Details");
    println!("─────────────");
    println!("  ID:       {}", chunk.id);
    println!("  Source:   {}", chunk.source);
    println!("  Session:  {}", chunk.session_id.as_deref().unwrap_or("-"));
    println!(
        "  Created:  {}",
        chunk.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("  Metadata: {}", chunk.metadata);
    println!();
    println!("Content:");
    println!("{}", chunk.content);
}

fn cmd_wipe(client: &ApiClient, yes: bool) {
    if !yes {
        use std::io::Write;
        print!("This will delete ALL memory. Type 'yes' to confirm: ");
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        if input.trim() != "yes" {
            println!("Aborted.");
            std::process::exit(2);
        }
    }

    let pb = spinner("Wiping all memory...");
    match client.delete_with_header::<serde_json::Value>("/wipe", "X-Confirm-Wipe", "true") {
        Ok(_) => {
            pb.finish_and_clear();
            println!("{}", "✓ All memory wiped.".green().bold());
        }
        Err(e) => {
            pb.finish_and_clear();
            eprintln!("{} {}", "✗".red(), e);
            std::process::exit(1);
        }
    }
}

fn cmd_stats(client: &ApiClient) {
    match client.get::<StatsResponse>("/stats") {
        Ok(s) => {
            println!("mnemo memory stats");
            println!("──────────────────");
            println!("  Entities:    {}", s.entity_count);
            println!("  Chunks:      {}", s.chunk_count);
            println!("  Graph nodes: {}", s.node_count);
            println!("  Graph edges: {}", s.edge_count);
            println!("  Uptime:      {}s", s.uptime_seconds);
        }
        Err(e) => {
            eprintln!("{} {}", "✗".red(), e);
            std::process::exit(1);
        }
    }
}

fn cmd_health(client: &ApiClient) {
    match client.get::<HealthResponse>("/health") {
        Ok(h) => {
            let llm_status = if h.llm_reachable {
                "✓ reachable".green().to_string()
            } else {
                "✗ unreachable".red().to_string()
            };
            let db_status = if h.db_connected {
                "✓ connected".green().to_string()
            } else {
                "✗ disconnected".red().to_string()
            };
            println!("mnemo health");
            println!("────────────");
            println!("  API server:    {}", "✓ online".green());
            println!("  Database:      {}", db_status);
            println!("  LLM provider:  {}", llm_status);
            println!("  Provider:      {} / {}", h.provider_type, h.provider_model);
            println!("  Entities:      {}", h.entity_count);
            println!("  Chunks:        {}", h.chunk_count);
            println!("  Uptime:        {}s", h.uptime_seconds);
        }
        Err(e) => {
            eprintln!(
                "{} Cannot connect to mnemo server — is it running?",
                "✗".red()
            );
            eprintln!("  {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_config(client: &ApiClient) {
    match client.get::<HealthResponse>("/health") {
        Ok(h) => {
            println!("mnemo config");
            println!("────────────");
            println!("  Provider:      {}", h.provider_type);
            println!("  Model:         {}", h.provider_model);
            println!("  Config source: {}", h.config_source);
        }
        Err(e) => {
            eprintln!("{} {}", "✗".red(), e);
            std::process::exit(1);
        }
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let client = ApiClient::new(cli.server);

    match cli.command {
        Commands::Ingest { text, source, session } => cmd_ingest(&client, text, source, session),
        Commands::Search { query, limit, no_graph, raw } => {
            cmd_search(&client, query, limit, no_graph, raw)
        }
        Commands::Entities { limit, search } => cmd_entities(&client, limit, search),
        Commands::Entity { id, neighbors, depth } => cmd_entity(&client, id, neighbors, depth),
        Commands::Chunks { limit, session } => cmd_chunks(&client, limit, session),
        Commands::Chunk { id } => cmd_chunk(&client, id),
        Commands::Wipe { yes } => cmd_wipe(&client, yes),
        Commands::Stats => cmd_stats(&client),
        Commands::Health => cmd_health(&client),
        Commands::Config => cmd_config(&client),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_client_unreachable_returns_error() {
        let client = ApiClient::new("http://localhost:1".to_string());
        let result = client.get::<serde_json::Value>("/health");
        assert!(result.is_err());
    }

    #[test]
    fn test_ingest_command_parses() {
        let result = Cli::try_parse_from(["mnemo", "ingest", "hello world"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_search_command_parses() {
        let result = Cli::try_parse_from(["mnemo", "search", "rust"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_wipe_command_parses_with_yes_flag() {
        let result = Cli::try_parse_from(["mnemo", "wipe", "--yes"]);
        assert!(result.is_ok());
        if let Ok(cli) = result {
            if let Commands::Wipe { yes } = cli.command {
                assert!(yes);
            }
        }
    }

    #[test]
    fn test_server_flag_parses() {
        let result = Cli::try_parse_from(["mnemo", "--server", "http://example.com", "stats"]);
        assert!(result.is_ok());
        if let Ok(cli) = result {
            assert_eq!(cli.server, "http://example.com");
        }
    }
}
