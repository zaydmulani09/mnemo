use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "TEXT", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntityType {
    Person,
    Organization,
    Place,
    Concept,
    Tool,
    Event,
    Document,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: Uuid,
    pub name: String,
    pub entity_type: EntityType,
    pub aliases: Vec<String>,
    pub attributes: serde_json::Value,
    pub confidence: f32,
    pub source_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub id: Uuid,
    pub from_entity_id: Uuid,
    pub to_entity_id: Uuid,
    pub relation_type: String,
    pub weight: f32,
    pub attributes: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: Uuid,
    pub content: String,
    pub source: String,
    pub session_id: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunkEntity {
    pub chunk_id: Uuid,
    pub entity_id: Uuid,
    pub mention_text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub name: String,
    pub entity_type: EntityType,
    pub aliases: Vec<String>,
    pub attributes: serde_json::Value,
    pub confidence: f32,
    pub mention_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelation {
    pub from_entity: String,
    pub to_entity: String,
    pub relation_type: String,
    pub weight: f32,
    pub attributes: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub entities: Vec<ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalQuery {
    pub text: String,
    pub session_id: Option<String>,
    pub max_chunks: usize,
    pub max_entities: usize,
    pub min_confidence: f32,
    pub include_graph: bool,
    pub graph_depth: usize,
}

impl Default for RetrievalQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            session_id: None,
            max_chunks: 10,
            max_entities: 20,
            min_confidence: 0.5,
            include_graph: true,
            graph_depth: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub chunks: Vec<MemoryChunk>,
    pub entities: Vec<Entity>,
    pub relations: Vec<Relation>,
    pub context_prompt: String,
    pub retrieved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRequest {
    pub content: String,
    pub source: String,
    pub session_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestResponse {
    pub chunk_id: Uuid,
    pub entities_extracted: usize,
    pub relations_extracted: usize,
    pub processing_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub db_connected: bool,
    pub llm_reachable: bool,
    pub entity_count: i64,
    pub chunk_count: i64,
    pub uptime_seconds: u64,
    pub provider_type: String,
    pub provider_model: String,
    pub config_source: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    fn test_entity() -> Entity {
        Entity {
            id: Uuid::new_v4(),
            name: "Rust programming language".to_string(),
            entity_type: EntityType::Tool,
            aliases: vec!["Rust".to_string(), "rustlang".to_string()],
            attributes: json!({"paradigm": "systems", "year": 2010}),
            confidence: 0.95,
            source_count: 3,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn entity_roundtrip() {
        let entity = test_entity();
        let json = serde_json::to_string(&entity).unwrap();
        let back: Entity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, entity.name);
        assert_eq!(back.aliases, entity.aliases);
        assert_eq!(back.source_count, entity.source_count);
    }

    #[test]
    fn entity_type_serializes_correctly() {
        assert_eq!(
            serde_json::to_string(&EntityType::Person).unwrap(),
            "\"Person\""
        );
        assert_eq!(
            serde_json::to_string(&EntityType::Organization).unwrap(),
            "\"Organization\""
        );
        assert_eq!(
            serde_json::to_string(&EntityType::Tool).unwrap(),
            "\"Tool\""
        );
        assert_eq!(
            serde_json::to_string(&EntityType::Other).unwrap(),
            "\"Other\""
        );
    }

    #[test]
    fn entity_type_all_variants_roundtrip() {
        let variants = vec![
            EntityType::Person,
            EntityType::Organization,
            EntityType::Place,
            EntityType::Concept,
            EntityType::Tool,
            EntityType::Event,
            EntityType::Document,
            EntityType::Other,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: EntityType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn relation_roundtrip() {
        let rel = Relation {
            id: Uuid::new_v4(),
            from_entity_id: Uuid::new_v4(),
            to_entity_id: Uuid::new_v4(),
            relation_type: "uses".to_string(),
            weight: 0.8,
            attributes: json!({}),
            created_at: now(),
            updated_at: now(),
        };
        let json = serde_json::to_string(&rel).unwrap();
        let back: Relation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.relation_type, rel.relation_type);
        assert_eq!(back.from_entity_id, rel.from_entity_id);
    }

    #[test]
    fn memory_chunk_roundtrip() {
        let chunk = MemoryChunk {
            id: Uuid::new_v4(),
            content: "Alice uses Rust at her job at Mozilla.".to_string(),
            source: "conversation".to_string(),
            session_id: Some("sess-001".to_string()),
            embedding: Some(vec![0.1, 0.2, 0.3]),
            metadata: json!({"turn": 1}),
            created_at: now(),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: MemoryChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, chunk.content);
        assert_eq!(back.embedding, chunk.embedding);
        assert_eq!(back.session_id, chunk.session_id);
    }

    #[test]
    fn memory_chunk_entity_roundtrip() {
        let mce = MemoryChunkEntity {
            chunk_id: Uuid::new_v4(),
            entity_id: Uuid::new_v4(),
            mention_text: "Rust".to_string(),
            confidence: 0.9,
        };
        let json = serde_json::to_string(&mce).unwrap();
        let back: MemoryChunkEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mention_text, mce.mention_text);
        assert!((back.confidence - mce.confidence).abs() < f32::EPSILON);
    }

    #[test]
    fn extraction_result_roundtrip() {
        let result = ExtractionResult {
            entities: vec![ExtractedEntity {
                name: "Alice".to_string(),
                entity_type: EntityType::Person,
                aliases: vec![],
                attributes: json!({}),
                confidence: 0.97,
                mention_text: "Alice".to_string(),
            }],
            relations: vec![ExtractedRelation {
                from_entity: "Alice".to_string(),
                to_entity: "Mozilla".to_string(),
                relation_type: "works_at".to_string(),
                weight: 0.85,
                attributes: json!({}),
            }],
            summary: Some("Alice works at Mozilla using Rust.".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ExtractionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entities.len(), 1);
        assert_eq!(back.relations.len(), 1);
        assert_eq!(back.summary, result.summary);
    }

    #[test]
    fn retrieval_query_default_values() {
        let q = RetrievalQuery::default();
        assert_eq!(q.text, "");
        assert_eq!(q.max_chunks, 10);
        assert_eq!(q.max_entities, 20);
        assert!((q.min_confidence - 0.5).abs() < f32::EPSILON);
        assert!(q.include_graph);
        assert_eq!(q.graph_depth, 2);
        assert!(q.session_id.is_none());
    }

    #[test]
    fn retrieval_query_roundtrip() {
        let q = RetrievalQuery {
            text: "What does Alice work on?".to_string(),
            session_id: Some("sess-42".to_string()),
            max_chunks: 5,
            max_entities: 10,
            min_confidence: 0.7,
            include_graph: false,
            graph_depth: 1,
        };
        let json = serde_json::to_string(&q).unwrap();
        let back: RetrievalQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, q.text);
        assert_eq!(back.max_chunks, q.max_chunks);
        assert!(!back.include_graph);
    }

    #[test]
    fn retrieval_result_roundtrip() {
        let result = RetrievalResult {
            chunks: vec![],
            entities: vec![test_entity()],
            relations: vec![],
            context_prompt: "MEMORY:\n- Rust is a systems language.".to_string(),
            retrieved_at: now(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: RetrievalResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entities.len(), 1);
        assert_eq!(back.context_prompt, result.context_prompt);
    }

    #[test]
    fn ingest_request_roundtrip() {
        let req = IngestRequest {
            content: "Bob is a senior engineer at Anthropic.".to_string(),
            source: "conversation".to_string(),
            session_id: None,
            metadata: Some(json!({"channel": "slack"})),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: IngestRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, req.content);
        assert!(back.session_id.is_none());
    }

    #[test]
    fn ingest_response_roundtrip() {
        let resp = IngestResponse {
            chunk_id: Uuid::new_v4(),
            entities_extracted: 3,
            relations_extracted: 2,
            processing_time_ms: 42,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: IngestResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entities_extracted, 3);
        assert_eq!(back.processing_time_ms, 42);
    }

    #[test]
    fn health_response_roundtrip() {
        let h = HealthResponse {
            status: "ok".to_string(),
            version: "0.1.0".to_string(),
            db_connected: true,
            llm_reachable: true,
            entity_count: 150,
            chunk_count: 47,
            uptime_seconds: 3600,
            provider_type: "Ollama".to_string(),
            provider_model: "llama3".to_string(),
            config_source: "env".to_string(),
        };
        let json = serde_json::to_string(&h).unwrap();
        let back: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, "ok");
        assert!(back.db_connected);
        assert_eq!(back.entity_count, 150);
    }

    #[test]
    fn memory_chunk_no_embedding_roundtrip() {
        let chunk = MemoryChunk {
            id: Uuid::new_v4(),
            content: "No embedding yet.".to_string(),
            source: "cli".to_string(),
            session_id: None,
            embedding: None,
            metadata: json!({}),
            created_at: now(),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: MemoryChunk = serde_json::from_str(&json).unwrap();
        assert!(back.embedding.is_none());
        assert!(back.session_id.is_none());
    }
}
