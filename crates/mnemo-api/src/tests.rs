use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mnemo_core::{
    db::Database,
    extractor::Extractor,
    graph::new_shared_graph,
    provider::{LlmConfig, MnemoConfig},
    retrieval::RetrievalEngine,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn make_state() -> AppState {
    let db = Arc::new(Database::new_in_memory().await.unwrap());
    let graph = new_shared_graph(db.clone()).await.unwrap();
    let extractor = Arc::new(Extractor::new(LlmConfig::default()));
    let retrieval = Arc::new(RetrievalEngine::new(db.clone(), graph.clone()));
    AppState {
        db,
        graph,
        extractor,
        retrieval,
        start_time: std::time::Instant::now(),
        config: Arc::new(MnemoConfig::default()),
        config_source: "env".to_string(),
    }
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn ingest_req(content: &str) -> Request<Body> {
    Request::builder()
        .uri("/ingest")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "content": content,
                "source": "test",
                "session_id": null,
                "metadata": null
            })
            .to_string(),
        ))
        .unwrap()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_health_endpoint() {
    let app = build_router(make_state().await);
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body.get("status").is_some(), "missing status field: {body}");
    assert!(body.get("db_connected").is_some());
}

#[tokio::test]
async fn test_ingest_endpoint() {
    let app = build_router(make_state().await);
    let resp = app.oneshot(ingest_req("Alice works at Mozilla on Rust.")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body.get("chunk_id").is_some(), "missing chunk_id: {body}");
}

#[tokio::test]
async fn test_ingest_then_retrieve() {
    let state = make_state().await;

    // ingest
    let app = build_router(state.clone());
    let resp = app.oneshot(ingest_req("The quick brown fox jumps over the lazy dog")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // retrieve
    let app2 = build_router(state);
    let req = Request::builder()
        .uri("/retrieve")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "text": "quick brown fox",
                "session_id": null,
                "max_chunks": 10,
                "max_entities": 20,
                "min_confidence": 0.0,
                "include_graph": false,
                "graph_depth": 2
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app2.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let chunks = body["chunks"].as_array().unwrap();
    assert!(!chunks.is_empty(), "expected chunks in retrieve result: {body}");
    assert!(chunks[0]["content"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("fox"));
}

#[tokio::test]
async fn test_list_entities_empty() {
    let app = build_router(make_state().await);
    let req = Request::builder()
        .uri("/entities")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_ingest_then_list_entities() {
    let state = make_state().await;

    // pre-insert entity directly (LLM offline — extractor returns empty)
    state
        .db
        .upsert_entity(&Entity {
            id: Uuid::new_v4(),
            name: "TestEntity".to_string(),
            entity_type: EntityType::Concept,
            aliases: vec![],
            attributes: json!({}),
            confidence: 0.9,
            source_count: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let app = build_router(state);
    let req = Request::builder()
        .uri("/entities")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(!body.as_array().unwrap().is_empty(), "expected entities: {body}");
}

#[tokio::test]
async fn test_get_entity_not_found() {
    let app = build_router(make_state().await);
    let req = Request::builder()
        .uri(&format!("/entities/{}", Uuid::new_v4()))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_search_endpoint() {
    let state = make_state().await;

    // pre-insert chunk via ingest endpoint
    let app = build_router(state.clone());
    let resp = app
        .oneshot(ingest_req("Rust programming language for systems"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // search
    let app2 = build_router(state);
    let req = Request::builder()
        .uri("/search")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(json!({"query": "Rust", "limit": 10}).to_string()))
        .unwrap();
    let resp = app2.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let chunks = body["chunks"].as_array().unwrap();
    assert!(!chunks.is_empty(), "expected chunks in search result: {body}");
}

#[tokio::test]
async fn test_wipe_requires_header() {
    let app = build_router(make_state().await);
    let req = Request::builder()
        .uri("/wipe")
        .method("DELETE")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body.get("error").is_some());
}

#[tokio::test]
async fn test_wipe_with_header() {
    let app = build_router(make_state().await);
    let req = Request::builder()
        .uri("/wipe")
        .method("DELETE")
        .header("X-Confirm-Wipe", "true")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["wiped"], json!(true));
}

#[tokio::test]
async fn test_stats_endpoint() {
    let app = build_router(make_state().await);
    let req = Request::builder()
        .uri("/stats")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body.get("entity_count").is_some(), "missing entity_count: {body}");
    assert!(body.get("chunk_count").is_some());
    assert!(body.get("node_count").is_some());
    assert!(body.get("edge_count").is_some());
    assert!(body.get("uptime_seconds").is_some());
}

#[tokio::test]
async fn test_chunks_endpoint() {
    let state = make_state().await;

    let app = build_router(state.clone());
    let resp = app.oneshot(ingest_req("Memory chunk content for test")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let app2 = build_router(state);
    let req = Request::builder()
        .uri("/chunks")
        .body(Body::empty())
        .unwrap();
    let resp = app2.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(!body.as_array().unwrap().is_empty(), "expected chunks: {body}");
}

#[tokio::test]
async fn test_delete_entity() {
    let state = make_state().await;
    let entity_id = Uuid::new_v4();

    // pre-insert entity directly
    state
        .db
        .upsert_entity(&Entity {
            id: entity_id,
            name: "DeleteMe".to_string(),
            entity_type: EntityType::Concept,
            aliases: vec![],
            attributes: json!({}),
            confidence: 0.9,
            source_count: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let app = build_router(state);
    let req = Request::builder()
        .uri(&format!("/entities/{}", entity_id))
        .method("DELETE")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["deleted"], json!(true));
}

#[tokio::test]
async fn test_full_ingest_retrieve_cycle() {
    let state = make_state().await;

    for content in &[
        "Rust programming language for systems development",
        "Rust memory safety without garbage collection",
        "Rust is used for web assembly and embedded systems",
    ] {
        let app = build_router(state.clone());
        let resp = app.oneshot(ingest_req(content)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let app = build_router(state);
    let req = Request::builder()
        .uri("/retrieve")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "text": "Rust",
                "session_id": null,
                "max_chunks": 10,
                "max_entities": 10,
                "min_confidence": 0.0,
                "include_graph": false,
                "graph_depth": 2
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(!body["chunks"].as_array().unwrap().is_empty(), "expected chunks: {body}");
    assert!(
        !body["context_prompt"].as_str().unwrap_or("").is_empty(),
        "expected non-empty context_prompt: {body}"
    );
}

#[tokio::test]
async fn test_retrieve_empty_returns_valid_structure() {
    let app = build_router(make_state().await);
    let req = Request::builder()
        .uri("/retrieve")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "text": "anything",
                "session_id": null,
                "max_chunks": 10,
                "max_entities": 10,
                "min_confidence": 0.0,
                "include_graph": false,
                "graph_depth": 2
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["chunks"].is_array(), "chunks should be array not null");
    assert!(body["entities"].is_array(), "entities should be array not null");
    assert!(body["relations"].is_array(), "relations should be array not null");
    assert_eq!(body["chunks"].as_array().unwrap().len(), 0);
    assert_eq!(body["entities"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_entities_pagination() {
    let state = make_state().await;

    for i in 0..5 {
        state
            .db
            .upsert_entity(&Entity {
                id: Uuid::new_v4(),
                name: format!("PagTest{}", i),
                entity_type: EntityType::Concept,
                aliases: vec![],
                attributes: json!({}),
                confidence: 0.9,
                source_count: 1,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
    }

    let app = build_router(state.clone());
    let req = Request::builder()
        .uri("/entities?limit=2&offset=0")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let body1 = body_json(resp).await;
    let page1 = body1.as_array().unwrap();
    assert_eq!(page1.len(), 2);

    let app2 = build_router(state);
    let req2 = Request::builder()
        .uri("/entities?limit=2&offset=2")
        .body(Body::empty())
        .unwrap();
    let resp2 = app2.oneshot(req2).await.unwrap();
    let body2 = body_json(resp2).await;
    let page2 = body2.as_array().unwrap();
    assert_eq!(page2.len(), 2);

    let ids1: std::collections::HashSet<&str> =
        page1.iter().map(|e| e["id"].as_str().unwrap()).collect();
    let ids2: std::collections::HashSet<&str> =
        page2.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert!(ids1.is_disjoint(&ids2), "pagination pages should not overlap");
}

#[tokio::test]
async fn test_neighbors_endpoint_depth_param() {
    let state = make_state().await;

    let a = Entity {
        id: Uuid::new_v4(),
        name: "NodeA".to_string(),
        entity_type: EntityType::Concept,
        aliases: vec![],
        attributes: json!({}),
        confidence: 0.9,
        source_count: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let b = Entity { id: Uuid::new_v4(), name: "NodeB".to_string(), ..a.clone() };
    let c = Entity { id: Uuid::new_v4(), name: "NodeC".to_string(), ..a.clone() };
    let (a_id, b_id, c_id) = (a.id, b.id, c.id);

    state.db.upsert_entity(&a).await.unwrap();
    state.db.upsert_entity(&b).await.unwrap();
    state.db.upsert_entity(&c).await.unwrap();
    state
        .db
        .upsert_relation(&Relation {
            id: Uuid::new_v4(),
            from_entity_id: a_id,
            to_entity_id: b_id,
            relation_type: "next".to_string(),
            weight: 0.8,
            attributes: json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    state
        .db
        .upsert_relation(&Relation {
            id: Uuid::new_v4(),
            from_entity_id: b_id,
            to_entity_id: c_id,
            relation_type: "next".to_string(),
            weight: 0.8,
            attributes: json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    state.graph.write().await.reload().await.unwrap();

    // depth=1: only B
    let app = build_router(state.clone());
    let req = Request::builder()
        .uri(&format!("/entities/{}/neighbors?depth=1", a_id))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let neighbors = body.as_array().unwrap();
    assert_eq!(neighbors.len(), 1, "depth=1 should return 1 neighbor");
    assert_eq!(neighbors[0]["name"].as_str().unwrap(), "NodeB");

    // depth=2: B and C
    let app2 = build_router(state);
    let req2 = Request::builder()
        .uri(&format!("/entities/{}/neighbors?depth=2", a_id))
        .body(Body::empty())
        .unwrap();
    let resp2 = app2.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = body_json(resp2).await;
    let neighbors2 = body2.as_array().unwrap();
    assert_eq!(neighbors2.len(), 2, "depth=2 should return 2 neighbors");
    let names2: Vec<&str> = neighbors2.iter().map(|n| n["name"].as_str().unwrap()).collect();
    assert!(
        names2.contains(&"NodeB") && names2.contains(&"NodeC"),
        "expected NodeB and NodeC: {:?}",
        names2
    );

    let _ = c_id; // suppress unused warning
}

#[tokio::test]
async fn test_search_finds_both_entities_and_chunks() {
    let state = make_state().await;

    state
        .db
        .upsert_entity(&Entity {
            id: Uuid::new_v4(),
            name: "Rustacean".to_string(),
            entity_type: EntityType::Concept,
            aliases: vec![],
            attributes: json!({}),
            confidence: 0.9,
            source_count: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let app = build_router(state.clone());
    let resp = app
        .oneshot(ingest_req("Rustacean is a passionate Rust programmer"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let app2 = build_router(state);
    let req = Request::builder()
        .uri("/search")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(json!({"query": "Rustacean", "limit": 10}).to_string()))
        .unwrap();
    let resp2 = app2.oneshot(req).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body = body_json(resp2).await;
    assert!(!body["entities"].as_array().unwrap().is_empty(), "expected entities: {body}");
    assert!(!body["chunks"].as_array().unwrap().is_empty(), "expected chunks: {body}");
}

#[tokio::test]
async fn test_delete_chunk_then_verify_gone() {
    let state = make_state().await;

    let app = build_router(state.clone());
    let resp = app.oneshot(ingest_req("chunk to be deleted")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ingest_body = body_json(resp).await;
    let chunk_id = ingest_body["chunk_id"].as_str().unwrap().to_string();

    let app2 = build_router(state.clone());
    let req = Request::builder()
        .uri(&format!("/chunks/{}", chunk_id))
        .method("DELETE")
        .body(Body::empty())
        .unwrap();
    let resp2 = app2.oneshot(req).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    let app3 = build_router(state);
    let req3 = Request::builder()
        .uri(&format!("/chunks/{}", chunk_id))
        .body(Body::empty())
        .unwrap();
    let resp3 = app3.oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::NOT_FOUND, "deleted chunk should return 404");
}

#[tokio::test]
async fn test_concurrent_ingest_requests() {
    let state = make_state().await;

    let (r1, r2, r3, r4, r5) = tokio::join!(
        build_router(state.clone()).oneshot(ingest_req("concurrent content one")),
        build_router(state.clone()).oneshot(ingest_req("concurrent content two")),
        build_router(state.clone()).oneshot(ingest_req("concurrent content three")),
        build_router(state.clone()).oneshot(ingest_req("concurrent content four")),
        build_router(state.clone()).oneshot(ingest_req("concurrent content five")),
    );

    assert_eq!(r1.unwrap().status(), StatusCode::OK);
    assert_eq!(r2.unwrap().status(), StatusCode::OK);
    assert_eq!(r3.unwrap().status(), StatusCode::OK);
    assert_eq!(r4.unwrap().status(), StatusCode::OK);
    assert_eq!(r5.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn test_wipe_then_stats_shows_zero() {
    let state = make_state().await;

    let app = build_router(state.clone());
    let resp = app.oneshot(ingest_req("some data to wipe")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let app2 = build_router(state.clone());
    let req = Request::builder()
        .uri("/wipe")
        .method("DELETE")
        .header("X-Confirm-Wipe", "true")
        .body(Body::empty())
        .unwrap();
    let resp2 = app2.oneshot(req).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    let app3 = build_router(state);
    let req3 = Request::builder()
        .uri("/stats")
        .body(Body::empty())
        .unwrap();
    let resp3 = app3.oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);
    let body = body_json(resp3).await;
    assert_eq!(body["entity_count"].as_i64().unwrap(), 0, "entity_count should be 0 after wipe");
    assert_eq!(body["chunk_count"].as_i64().unwrap(), 0, "chunk_count should be 0 after wipe");
}
