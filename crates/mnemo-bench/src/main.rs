use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use mnemo_core::{
    db::Database,
    graph::{new_shared_graph, SharedGraph},
    models::*,
    retrieval::RetrievalEngine,
};
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

// ── BenchResult ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct BenchResult {
    name: String,
    iterations: usize,
    total_ms: f64,
    avg_ms: f64,
    min_ms: f64,
    max_ms: f64,
    p95_ms: f64,
    ops_per_sec: f64,
}

impl BenchResult {
    fn from_samples(name: &str, mut samples_ms: Vec<f64>) -> Self {
        assert!(!samples_ms.is_empty(), "samples must not be empty");
        let n = samples_ms.len();
        samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let total_ms: f64 = samples_ms.iter().sum();
        let avg_ms = total_ms / n as f64;
        let min_ms = samples_ms[0];
        let max_ms = samples_ms[n - 1];
        let p95_idx = ((n as f64 * 0.95) as usize).min(n - 1);
        let p95_ms = samples_ms[p95_idx];
        let ops_per_sec = if avg_ms > 0.0 { 1000.0 / avg_ms } else { 1_000_000.0 };
        BenchResult {
            name: name.to_string(),
            iterations: n,
            total_ms,
            avg_ms,
            min_ms,
            max_ms,
            p95_ms,
            ops_per_sec,
        }
    }
}

// ── BenchSuite ────────────────────────────────────────────────────────────────

struct BenchSuite {
    db: Arc<Database>,
    graph: SharedGraph,
    results: Vec<BenchResult>,
    _tempdir: tempfile::TempDir,
}

impl BenchSuite {
    async fn new() -> Result<Self> {
        let tempdir = tempfile::tempdir()?;
        let db_path = tempdir.path().join("bench.db");
        // Forward-slash path for SQLite URL on all platforms
        let db_path_str = db_path.to_str().unwrap().replace('\\', "/");
        let db = Arc::new(Database::new(&db_path_str).await?);
        let graph = new_shared_graph(db.clone()).await?;
        Ok(Self {
            db,
            graph,
            results: Vec::new(),
            _tempdir: tempdir,
        })
    }

    async fn run_all(&mut self) {
        println!("  Running DB benchmarks...");
        self.bench_db_insert_entity(1000).await;
        self.bench_db_get_entity_by_id(1000).await;
        self.bench_db_search_entities(500).await;
        self.bench_db_insert_chunk(1000).await;
        self.bench_db_search_chunks(500).await;
        self.bench_db_upsert_relation(1000).await;

        println!("  Running graph benchmarks (building 500-node fixture)...");
        self.bench_graph_add_node(1000).await;
        let node_ids = self.build_graph_fixture(500, 1000).await;
        self.bench_graph_get_neighbors_depth1(500, &node_ids).await;
        self.bench_graph_get_neighbors_depth2(500, &node_ids).await;
        self.bench_graph_find_path(200, &node_ids).await;

        println!("  Running retrieval benchmarks (building retrieval fixtures)...");
        self.build_retrieval_fixtures(100, 50).await;
        self.bench_retrieval_score_chunk(1000).await;
        self.bench_retrieval_full_pipeline(100).await;
    }

    // ── Fixture builders ──────────────────────────────────────────────────────

    /// Pre-build a 500-node graph with ~1000 edges (ring + shortcuts) for
    /// neighbor and path-finding benchmarks.
    async fn build_graph_fixture(&self, node_count: usize, edge_count: usize) -> Vec<Uuid> {
        let mut ids = Vec::with_capacity(node_count);
        let mut g = self.graph.write().await;
        for i in 0..node_count {
            let entity = make_entity(&format!("fixture_node_{}", i), EntityType::Concept);
            g.add_node(&entity);
            ids.push(entity.id);
        }
        let half = edge_count / 2;
        // Ring topology
        for i in 0..half {
            let from = ids[i % node_count];
            let to = ids[(i + 1) % node_count];
            g.add_edge(&make_relation(from, to, "ring"));
        }
        // Shortcut topology (stride = node_count/5)
        let stride = (node_count / 5).max(1);
        for i in 0..half {
            let from = ids[i % node_count];
            let to = ids[(i + stride) % node_count];
            g.add_edge(&make_relation(from, to, "skip"));
        }
        ids
    }

    /// Pre-insert 100 chunks and 50 entities for the retrieval pipeline bench.
    async fn build_retrieval_fixtures(&self, chunk_count: usize, entity_count: usize) {
        for i in 0..entity_count {
            let e = make_entity(&format!("retrieval_entity_{}", i), EntityType::Concept);
            let _ = self.db.upsert_entity(&e).await;
            self.graph.write().await.add_node(&e);
        }
        for i in 0..chunk_count {
            let chunk = MemoryChunk {
                id: Uuid::new_v4(),
                content: format!(
                    "retrieval bench content about topic {} memory context pipeline",
                    i % 10
                ),
                source: "bench".to_string(),
                session_id: None,
                embedding: None,
                metadata: serde_json::json!({}),
                created_at: Utc::now(),
            };
            let _ = self.db.insert_chunk(&chunk).await;
        }
    }

    // ── DB benchmarks ─────────────────────────────────────────────────────────

    async fn bench_db_insert_entity(&mut self, iters: usize) {
        let mut samples = Vec::with_capacity(iters);
        for i in 0..iters {
            let e = make_entity(&format!("insert_entity_{}", i), EntityType::Concept);
            let t = Instant::now();
            let _ = self.db.upsert_entity(&e).await;
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("db_insert_entity", samples));
    }

    async fn bench_db_get_entity_by_id(&mut self, iters: usize) {
        // Pre-insert 100 entities
        let mut ids = Vec::with_capacity(100);
        for i in 0..100 {
            let e = make_entity(&format!("lookup_entity_{}", i), EntityType::Person);
            let _ = self.db.upsert_entity(&e).await;
            ids.push(e.id);
        }
        let mut samples = Vec::with_capacity(iters);
        for i in 0..iters {
            let id = ids[i % ids.len()];
            let t = Instant::now();
            let _ = self.db.get_entity_by_id(id).await;
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("db_get_entity_by_id", samples));
    }

    async fn bench_db_search_entities(&mut self, iters: usize) {
        // Pre-insert 200 entities
        for i in 0..200 {
            let e = make_entity(&format!("entity_{}_searchable", i), EntityType::Concept);
            let _ = self.db.upsert_entity(&e).await;
        }
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t = Instant::now();
            let _ = self.db.search_entities_by_name("entity_1", 10).await;
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("db_search_entities", samples));
    }

    async fn bench_db_insert_chunk(&mut self, iters: usize) {
        let mut samples = Vec::with_capacity(iters);
        for i in 0..iters {
            let chunk = MemoryChunk {
                id: Uuid::new_v4(),
                content: format!("bench chunk content item {}", i),
                source: "bench".to_string(),
                session_id: None,
                embedding: None,
                metadata: serde_json::json!({}),
                created_at: Utc::now(),
            };
            let t = Instant::now();
            let _ = self.db.insert_chunk(&chunk).await;
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("db_insert_chunk", samples));
    }

    async fn bench_db_search_chunks(&mut self, iters: usize) {
        // Pre-insert 200 chunks
        for i in 0..200 {
            let chunk = MemoryChunk {
                id: Uuid::new_v4(),
                content: format!("memory content about topic {} searchable benchmark", i),
                source: "bench".to_string(),
                session_id: None,
                embedding: None,
                metadata: serde_json::json!({}),
                created_at: Utc::now(),
            };
            let _ = self.db.insert_chunk(&chunk).await;
        }
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t = Instant::now();
            let _ = self.db.search_chunks_by_content("topic_1", 10).await;
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("db_search_chunks", samples));
    }

    async fn bench_db_upsert_relation(&mut self, iters: usize) {
        // Pre-insert 2 entities to relate
        let e1 = make_entity("bench_rel_from", EntityType::Person);
        let e2 = make_entity("bench_rel_to", EntityType::Organization);
        let from_id = e1.id;
        let to_id = e2.id;
        let _ = self.db.upsert_entity(&e1).await;
        let _ = self.db.upsert_entity(&e2).await;

        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            // Re-use same from/to/type → exercises ON CONFLICT path
            let rel = make_relation(from_id, to_id, "works_at");
            let t = Instant::now();
            let _ = self.db.upsert_relation(&rel).await;
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("db_upsert_relation", samples));
    }

    // ── Graph benchmarks ──────────────────────────────────────────────────────

    async fn bench_graph_add_node(&mut self, iters: usize) {
        let mut samples = Vec::with_capacity(iters);
        for i in 0..iters {
            let entity = make_entity(&format!("graph_bench_node_{}", i), EntityType::Concept);
            let t = Instant::now();
            self.graph.write().await.add_node(&entity);
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("graph_add_node", samples));
    }

    async fn bench_graph_get_neighbors_depth1(&mut self, iters: usize, node_ids: &[Uuid]) {
        let n = node_ids.len();
        let g = self.graph.read().await; // acquire lock once outside the timed loop
        let mut samples = Vec::with_capacity(iters);
        for i in 0..iters {
            let id = node_ids[i % n];
            let t = Instant::now();
            let _ = g.get_neighbors(id, 1);
            samples.push(elapsed_ms(t));
        }
        drop(g);
        self.results
            .push(BenchResult::from_samples("graph_get_neighbors_d1", samples));
    }

    async fn bench_graph_get_neighbors_depth2(&mut self, iters: usize, node_ids: &[Uuid]) {
        let n = node_ids.len();
        let g = self.graph.read().await;
        let mut samples = Vec::with_capacity(iters);
        for i in 0..iters {
            let id = node_ids[i % n];
            let t = Instant::now();
            let _ = g.get_neighbors(id, 2);
            samples.push(elapsed_ms(t));
        }
        drop(g);
        self.results
            .push(BenchResult::from_samples("graph_get_neighbors_d2", samples));
    }

    async fn bench_graph_find_path(&mut self, iters: usize, node_ids: &[Uuid]) {
        let n = node_ids.len();
        let g = self.graph.read().await;
        let mut samples = Vec::with_capacity(iters);
        for i in 0..iters {
            let from = node_ids[i % n];
            let to = node_ids[(i + n / 2) % n];
            let t = Instant::now();
            let _ = g.find_path(from, to);
            samples.push(elapsed_ms(t));
        }
        drop(g);
        self.results
            .push(BenchResult::from_samples("graph_find_path", samples));
    }

    // ── Retrieval benchmarks ──────────────────────────────────────────────────

    async fn bench_retrieval_score_chunk(&mut self, iters: usize) {
        let chunk = MemoryChunk {
            id: Uuid::new_v4(),
            content: "retrieval bench content about topic memory context pipeline".to_string(),
            source: "bench".to_string(),
            session_id: None,
            embedding: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
        };
        let query = RetrievalQuery {
            text: "retrieval bench content".to_string(),
            session_id: None,
            max_chunks: 10,
            max_entities: 20,
            min_confidence: 0.0,
            include_graph: false,
            graph_depth: 1,
        };
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t = Instant::now();
            let _ = score_chunk_inline(&chunk, &query);
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("retrieval_score_chunk", samples));
    }

    async fn bench_retrieval_full_pipeline(&mut self, iters: usize) {
        let engine = RetrievalEngine::new(self.db.clone(), self.graph.clone());
        let query = RetrievalQuery {
            text: "retrieval bench content".to_string(),
            session_id: None,
            max_chunks: 10,
            max_entities: 20,
            min_confidence: 0.0,
            include_graph: true,
            graph_depth: 2,
        };
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t = Instant::now();
            let _ = engine.retrieve(&query).await;
            samples.push(elapsed_ms(t));
        }
        self.results
            .push(BenchResult::from_samples("retrieval_full_pipeline", samples));
    }

    // ── Report ────────────────────────────────────────────────────────────────

    fn print_report(&self) {
        let border = "═".repeat(66);
        let sep = "─".repeat(66);
        println!("\n{}", "mnemo benchmark report".bold().cyan());
        println!("{}", border.cyan());
        println!(
            "{}",
            format!(
                "{:<32} {:>5}  {:>8}  {:>8}  {:>8}",
                "Benchmark", "Iters", "Avg(ms)", "P95(ms)", "Ops/sec"
            )
            .bold()
            .cyan()
        );
        println!("{}", sep.cyan());
        for r in &self.results {
            let ops_str = if r.ops_per_sec >= 1_000_000.0 {
                ">1M".to_string()
            } else {
                format!("{:.0}", r.ops_per_sec)
            };
            let line = format!(
                "{:<32} {:>5}  {:>8.3}  {:>8.3}  {:>8}",
                r.name, r.iterations, r.avg_ms, r.p95_ms, ops_str
            );
            if r.avg_ms > 10.0 {
                println!("{}", line.yellow());
            } else {
                println!("{}", line);
            }
        }
        println!("{}", border.cyan());
    }

    fn save_json(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.results)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn elapsed_ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn make_entity(name: &str, entity_type: EntityType) -> Entity {
    Entity {
        id: Uuid::new_v4(),
        name: name.to_string(),
        entity_type,
        aliases: vec![],
        attributes: serde_json::json!({}),
        confidence: 0.9,
        source_count: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn make_relation(from: Uuid, to: Uuid, rel_type: &str) -> Relation {
    Relation {
        id: Uuid::new_v4(),
        from_entity_id: from,
        to_entity_id: to,
        relation_type: rel_type.to_string(),
        weight: 0.8,
        attributes: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// Inline replica of retrieval.rs `score_chunk` (private fn → mirrored here).
fn score_chunk_inline(chunk: &MemoryChunk, query: &RetrievalQuery) -> f32 {
    let mut score = 0.5_f32;
    let content_lower = chunk.content.to_lowercase();
    let overlap = query
        .text
        .split_whitespace()
        .filter(|w| content_lower.contains(w.to_lowercase().as_str()))
        .count();
    score += (overlap as f32 * 0.1).min(0.4);
    let now = chrono::Utc::now();
    let age = now.signed_duration_since(chunk.created_at);
    if age.num_hours() < 24 {
        score += 0.1;
    } else if age.num_days() < 7 {
        score += 0.05;
    }
    score.max(0.0).min(1.0)
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let output_json = args
        .iter()
        .position(|a| a == "--json")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str());

    let filter = args
        .iter()
        .position(|a| a == "--filter")
        .and_then(|i| args.get(i + 1))
        .cloned();

    println!("mnemo benchmark suite");
    println!("Building test fixtures...");

    let mut suite = BenchSuite::new().await?;
    suite.run_all().await;

    if let Some(f) = &filter {
        suite.results.retain(|r| r.name.contains(f.as_str()));
    }

    suite.print_report();

    if let Some(path) = output_json {
        suite.save_json(path)?;
        println!("Results saved to {}", path);
    }

    Ok(())
}
