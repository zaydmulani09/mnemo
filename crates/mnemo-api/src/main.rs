use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use chrono::Utc;
use mnemo_core::{
    db::Database,
    extractor::Extractor,
    graph::{new_shared_graph, GraphNode, SharedGraph},
    models::*,
    provider::MnemoConfig,
    retrieval::RetrievalEngine,
    MnemoError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

#[cfg(test)]
mod tests;

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub graph: SharedGraph,
    pub extractor: Arc<Extractor>,
    pub retrieval: Arc<RetrievalEngine>,
    pub start_time: std::time::Instant,
    pub config: Arc<MnemoConfig>,
    pub config_source: String,
}

// ── Error type ───────────────────────────────────────────────────────────────

pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}

impl From<MnemoError> for ApiError {
    fn from(err: MnemoError) -> Self {
        match err {
            MnemoError::NotFound(msg) => ApiError {
                status: StatusCode::NOT_FOUND,
                message: msg,
            },
            e => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: e.to_string(),
            },
        }
    }
}

// ── Query / request param types ──────────────────────────────────────────────

#[derive(Deserialize)]
struct ListEntitiesParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
struct ListChunksParams {
    limit: Option<i64>,
    offset: Option<i64>,
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct NeighborsParams {
    depth: Option<usize>,
}

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct SearchResponse {
    entities: Vec<Entity>,
    chunks: Vec<MemoryChunk>,
}

#[derive(Serialize)]
struct StatsResponse {
    entity_count: i64,
    chunk_count: i64,
    node_count: usize,
    edge_count: usize,
    uptime_seconds: u64,
}

// ── Router ───────────────────────────────────────────────────────────────────

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ingest", post(ingest_handler))
        .route("/retrieve", post(retrieve_handler))
        .route("/entities", get(list_entities_handler))
        .route("/entities/:id", get(get_entity_handler))
        .route("/entities/:id", delete(delete_entity_handler))
        .route("/entities/:id/neighbors", get(get_neighbors_handler))
        .route("/chunks", get(list_chunks_handler))
        .route("/chunks/:id", get(get_chunk_handler))
        .route("/chunks/:id", delete(delete_chunk_handler))
        .route("/search", post(search_handler))
        .route("/wipe", delete(wipe_handler))
        .route("/stats", get(stats_handler))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn health_handler(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, ApiError> {
    let db_connected = state.db.health_check().await.unwrap_or(false);
    let llm_reachable = state.extractor.health_check().await;
    let (entity_count, chunk_count) = state.retrieval.stats().await.map_err(ApiError::from)?;
    let uptime_seconds = state.start_time.elapsed().as_secs();

    Ok(Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_connected,
        llm_reachable,
        entity_count,
        chunk_count,
        uptime_seconds,
        provider_type: format!("{:?}", state.config.llm.provider),
        provider_model: state.config.llm.model.clone(),
        config_source: state.config_source.clone(),
    }))
}

async fn ingest_handler(
    State(state): State<AppState>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, ApiError> {
    let chunk = MemoryChunk {
        id: Uuid::new_v4(),
        content: req.content.clone(),
        source: req.source.clone(),
        session_id: req.session_id.clone(),
        embedding: None,
        metadata: req.metadata.unwrap_or_else(|| json!({})),
        created_at: Utc::now(),
    };

    state.db.insert_chunk(&chunk).await.map_err(ApiError::from)?;

    let extraction = state.extractor.extract_with_fallback(&req.content).await;

    let mut graph_guard = state.graph.write().await;
    let response = graph_guard
        .ingest(&chunk, &extraction, state.db.as_ref())
        .await
        .map_err(ApiError::from)?;
    drop(graph_guard);

    Ok(Json(response))
}

async fn retrieve_handler(
    State(state): State<AppState>,
    Json(query): Json<RetrievalQuery>,
) -> Result<Json<RetrievalResult>, ApiError> {
    let result = state.retrieval.retrieve(&query).await.map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn list_entities_handler(
    State(state): State<AppState>,
    Query(params): Query<ListEntitiesParams>,
) -> Result<Json<Vec<Entity>>, ApiError> {
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);
    let entities = state.db.list_entities(limit, offset).await.map_err(ApiError::from)?;
    Ok(Json(entities))
}

async fn get_entity_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Entity>, ApiError> {
    state
        .db
        .get_entity_by_id(id)
        .await
        .map_err(ApiError::from)?
        .map(Json)
        .ok_or_else(|| ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("entity {} not found", id),
        })
}

async fn delete_entity_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    state.db.delete_entity(id).await.map_err(ApiError::from)?;
    Ok(Json(json!({"deleted": true})))
}

async fn get_neighbors_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<NeighborsParams>,
) -> Result<Json<Vec<GraphNode>>, ApiError> {
    let depth = params.depth.unwrap_or(2).min(5);
    let neighbors = state.graph.read().await.get_neighbors(id, depth);
    Ok(Json(neighbors))
}

async fn list_chunks_handler(
    State(state): State<AppState>,
    Query(params): Query<ListChunksParams>,
) -> Result<Json<Vec<MemoryChunk>>, ApiError> {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    let chunks = if let Some(session_id) = params.session_id {
        state
            .db
            .list_chunks_by_session(&session_id, limit)
            .await
            .map_err(ApiError::from)?
    } else {
        state.db.list_chunks(limit, offset).await.map_err(ApiError::from)?
    };

    Ok(Json(chunks))
}

async fn get_chunk_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryChunk>, ApiError> {
    state
        .db
        .get_chunk_by_id(id)
        .await
        .map_err(ApiError::from)?
        .map(Json)
        .ok_or_else(|| ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("chunk {} not found", id),
        })
}

async fn delete_chunk_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    state.db.delete_chunk(id).await.map_err(ApiError::from)?;
    Ok(Json(json!({"deleted": true})))
}

async fn search_handler(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiError> {
    let limit = req.limit.unwrap_or(10);
    let entities = state
        .db
        .search_entities_by_name(&req.query, limit)
        .await
        .map_err(ApiError::from)?;
    let chunks = state
        .db
        .search_chunks_by_content(&req.query, limit)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(SearchResponse { entities, chunks }))
}

async fn wipe_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let confirmed = headers
        .get("X-Confirm-Wipe")
        .and_then(|v| v.to_str().ok())
        .map(|s| s == "true")
        .unwrap_or(false);

    if !confirmed {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            message: "missing required header X-Confirm-Wipe: true".to_string(),
        });
    }

    state.db.wipe_all().await.map_err(ApiError::from)?;
    state.graph.write().await.reload().await.map_err(ApiError::from)?;
    Ok(Json(json!({"wiped": true})))
}

async fn stats_handler(
    State(state): State<AppState>,
) -> Result<Json<StatsResponse>, ApiError> {
    let (entity_count, chunk_count) = state.retrieval.stats().await.map_err(ApiError::from)?;
    let graph = state.graph.read().await;
    let node_count = graph.node_count();
    let edge_count = graph.edge_count();
    drop(graph);

    Ok(Json(StatsResponse {
        entity_count,
        chunk_count,
        node_count,
        edge_count,
        uptime_seconds: state.start_time.elapsed().as_secs(),
    }))
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // --health-check: lightweight TCP liveness probe used by Docker HEALTHCHECK.
    // Opens a TCP connection to the local API port — no curl, no extra deps.
    // Runs synchronously before the async runtime does any real work.
    if std::env::args().any(|a| a == "--health-check") {
        let port: u16 = std::env::var("MNEMO_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);
        match std::net::TcpStream::connect(format!("127.0.0.1:{}", port)) {
            Ok(_) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let config_source = config_path
        .as_deref()
        .map(|p| format!("file:{}", p))
        .unwrap_or_else(|| "env".to_string());

    let mnemo_config = MnemoConfig::from_env_or_file(config_path.as_deref());
    let port = mnemo_config.port;

    let db = Arc::new(Database::new(&mnemo_config.db_path).await?);
    let graph = new_shared_graph(db.clone()).await?;
    let extractor = Arc::new(Extractor::new(mnemo_config.llm.clone()));
    let retrieval = Arc::new(RetrievalEngine::new(db.clone(), graph.clone()));

    let state = AppState {
        db,
        graph,
        extractor,
        retrieval,
        start_time: std::time::Instant::now(),
        config: Arc::new(mnemo_config),
        config_source,
    };

    let app = build_router(state);
    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("mnemo API server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    tracing::info!("shutdown signal received");
}
