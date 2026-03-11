use crate::db::Database;
use crate::error::Result;
use crate::graph::SharedGraph;
use crate::models::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

struct ScoredChunk {
    chunk: MemoryChunk,
    score: f32,
}

struct ScoredEntity {
    entity: Entity,
    score: f32,
}

pub struct RetrievalEngine {
    db: Arc<Database>,
    graph: SharedGraph,
}

impl RetrievalEngine {
    pub fn new(db: Arc<Database>, graph: SharedGraph) -> Self {
        Self { db, graph }
    }

    pub async fn retrieve(&self, query: &RetrievalQuery) -> Result<RetrievalResult> {
        // Stage 1 — chunk retrieval
        let mut chunk_map: HashMap<Uuid, ScoredChunk> = HashMap::new();

        for chunk in self.db.search_chunks_by_content(&query.text, 50).await? {
            let score = score_chunk(&chunk, query);
            chunk_map.insert(chunk.id, ScoredChunk { chunk, score });
        }

        if let Some(session_id) = &query.session_id {
            for chunk in self.db.list_chunks_by_session(session_id, 50).await? {
                let id = chunk.id;
                let score = score_chunk(&chunk, query);
                chunk_map.entry(id).or_insert(ScoredChunk { chunk, score });
            }
        }

        // Stage 2 — entity retrieval
        let mut entity_map: HashMap<Uuid, ScoredEntity> = HashMap::new();

        for entity in self.db.search_entities_by_name(&query.text, 50).await? {
            let score = score_entity(&entity, query);
            entity_map.insert(entity.id, ScoredEntity { entity, score });
        }

        let mut sorted_chunks: Vec<&ScoredChunk> = chunk_map.values().collect();
        sorted_chunks.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        for sc in sorted_chunks.iter().take(10) {
            for entity in self.db.get_entities_for_chunk(sc.chunk.id).await? {
                let id = entity.id;
                let score = score_entity(&entity, query);
                entity_map.entry(id).or_insert(ScoredEntity { entity, score });
            }
        }

        // Stage 3 — graph expansion
        if query.include_graph {
            let mut top: Vec<(&Uuid, &ScoredEntity)> = entity_map.iter().collect();
            top.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal));

            let top_ids: Vec<Uuid> = top.iter().take(query.max_entities).map(|(id, _)| **id).collect();

            let graph_nodes = {
                let g = self.graph.read().await;
                g.get_subgraph_entities(&top_ids, query.graph_depth)
            };

            for node in graph_nodes {
                if entity_map.contains_key(&node.entity_id) {
                    continue;
                }
                if let Some(entity) = self.db.get_entity_by_id(node.entity_id).await? {
                    let base = score_entity(&entity, query);
                    let score = (base * 0.5).max(0.0);
                    entity_map.insert(entity.id, ScoredEntity { entity, score });
                }
            }
        }

        // Stage 4 — relation retrieval
        let all_entity_ids: HashSet<Uuid> = entity_map.keys().cloned().collect();
        let mut relation_map: HashMap<Uuid, Relation> = HashMap::new();

        for &eid in &all_entity_ids {
            for relation in self.db.get_relations_from(eid).await? {
                if all_entity_ids.contains(&relation.from_entity_id)
                    && all_entity_ids.contains(&relation.to_entity_id)
                {
                    relation_map.insert(relation.id, relation);
                }
            }
        }

        // Stage 5 — ranking and truncation
        let mut scored_chunks: Vec<ScoredChunk> = chunk_map.into_values().collect();
        scored_chunks.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        let chunks: Vec<MemoryChunk> = scored_chunks
            .into_iter()
            .filter(|sc| sc.score >= query.min_confidence)
            .take(query.max_chunks)
            .map(|sc| sc.chunk)
            .collect();

        let mut scored_entities: Vec<ScoredEntity> = entity_map.into_values().collect();
        scored_entities.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        let entities: Vec<Entity> = scored_entities
            .into_iter()
            .filter(|se| se.score >= query.min_confidence)
            .take(query.max_entities)
            .map(|se| se.entity)
            .collect();

        let relations: Vec<Relation> = relation_map.into_values().collect();

        // Stage 6 — context string
        let context_prompt = build_context_prompt(&chunks, &entities, &relations);

        Ok(RetrievalResult {
            chunks,
            entities,
            relations,
            context_prompt,
            retrieved_at: chrono::Utc::now(),
        })
    }

    pub async fn retrieve_for_prompt(&self, text: &str) -> String {
        let query = RetrievalQuery {
            text: text.to_string(),
            ..RetrievalQuery::default()
        };
        match self.retrieve(&query).await {
            Ok(result) => result.context_prompt,
            Err(_) => String::new(),
        }
    }

    pub async fn stats(&self) -> Result<(i64, i64)> {
        let entity_count = self.db.count_entities().await?;
        let chunk_count = self.db.count_chunks().await?;
        Ok((entity_count, chunk_count))
    }
}

fn score_chunk(chunk: &MemoryChunk, query: &RetrievalQuery) -> f32 {
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

    if let (Some(cs), Some(qs)) = (&chunk.session_id, &query.session_id) {
        if cs == qs {
            score += 0.15;
        }
    }

    score.max(0.0).min(1.0)
}

fn score_entity(entity: &Entity, query: &RetrievalQuery) -> f32 {
    let mut score = entity.confidence * 0.5;
    let query_lower = query.text.to_lowercase();

    if query_lower.contains(&entity.name.to_lowercase()) {
        score += 0.3;
    }

    for alias in &entity.aliases {
        if query_lower.contains(&alias.to_lowercase()) {
            score += 0.2;
            break;
        }
    }

    score += (entity.source_count as f32 * 0.02).min(0.2);

    score.max(0.0).min(1.0)
}

fn build_context_prompt(chunks: &[MemoryChunk], entities: &[Entity], relations: &[Relation]) -> String {
    let mut parts: Vec<String> = vec!["=== MEMORY CONTEXT ===".to_string()];

    if !entities.is_empty() {
        let mut section = vec!["[RELEVANT FACTS]".to_string()];
        for entity in entities {
            let attr_part = if let Some(obj) = entity.attributes.as_object() {
                if obj.is_empty() {
                    String::new()
                } else {
                    let pairs: Vec<String> = obj
                        .iter()
                        .map(|(k, v)| {
                            let val = v
                                .as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| v.to_string());
                            format!("{}: {}", k, val)
                        })
                        .collect();
                    format!(": {}", pairs.join(", "))
                }
            } else {
                String::new()
            };
            section.push(format!("- {} ({:?}){}", entity.name, entity.entity_type, attr_part));
        }
        parts.push(section.join("\n"));
    }

    if !relations.is_empty() {
        let entity_map: HashMap<Uuid, &str> =
            entities.iter().map(|e| (e.id, e.name.as_str())).collect();
        let mut section = vec!["[RELATIONSHIPS]".to_string()];
        for relation in relations {
            let from = entity_map
                .get(&relation.from_entity_id)
                .copied()
                .unwrap_or("unknown");
            let to = entity_map
                .get(&relation.to_entity_id)
                .copied()
                .unwrap_or("unknown");
            let rel = relation.relation_type.replace('_', " ");
            section.push(format!(
                "- {} {} {} (confidence: {:.2})",
                from, rel, to, relation.weight
            ));
        }
        parts.push(section.join("\n"));
    }

    if !chunks.is_empty() {
        let mut section = vec!["[RELEVANT MEMORIES]".to_string()];
        for chunk in chunks {
            let date = chunk.created_at.format("%Y-%m-%d");
            let content: String = chunk.content.chars().take(500).collect();
            let content = if chunk.content.chars().count() > 500 {
                format!("{}...", content)
            } else {
                content
            };
            section.push(format!("[{} | {}]\n{}", chunk.source, date, content));
        }
        parts.push(section.join("\n"));
    }

    parts.push("=== END MEMORY CONTEXT ===".to_string());
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::new_shared_graph;
    use proptest::prelude::*;
    use serde_json::json;

    fn make_entity(name: &str, entity_type: EntityType, confidence: f32, source_count: i64) -> Entity {
        Entity {
            id: Uuid::new_v4(),
            name: name.to_string(),
            entity_type,
            aliases: vec![],
            attributes: json!({}),
            confidence,
            source_count,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn make_relation(from: Uuid, to: Uuid, rel_type: &str) -> Relation {
        Relation {
            id: Uuid::new_v4(),
            from_entity_id: from,
            to_entity_id: to,
            relation_type: rel_type.to_string(),
            weight: 0.8,
            attributes: json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn make_chunk(content: &str, session_id: Option<&str>, created_at: chrono::DateTime<chrono::Utc>) -> MemoryChunk {
        MemoryChunk {
            id: Uuid::new_v4(),
            content: content.to_string(),
            source: "test".to_string(),
            session_id: session_id.map(|s| s.to_string()),
            embedding: None,
            metadata: json!({}),
            created_at,
        }
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    fn days_ago(n: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now() - chrono::Duration::days(n)
    }

    fn default_query(text: &str) -> RetrievalQuery {
        RetrievalQuery {
            text: text.to_string(),
            ..RetrievalQuery::default()
        }
    }

    #[tokio::test]
    async fn test_retrieve_empty_db() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let graph = new_shared_graph(db.clone()).await.unwrap();
        let engine = RetrievalEngine::new(db, graph);
        let result = engine.retrieve(&default_query("anything")).await.unwrap();
        assert!(result.chunks.is_empty());
        assert!(result.entities.is_empty());
        assert!(result.relations.is_empty());
    }

    #[test]
    fn test_score_chunk_keyword_overlap() {
        let q = default_query("rust programming language");
        let with_kw = make_chunk("Rust is a programming language for systems", None, now());
        let without_kw = make_chunk("Apples grow on trees in orchards", None, now());
        let s1 = score_chunk(&with_kw, &q);
        let s2 = score_chunk(&without_kw, &q);
        assert!(s1 > s2, "keyword match should score higher: {} vs {}", s1, s2);
    }

    #[test]
    fn test_score_chunk_session_bonus() {
        let q = RetrievalQuery {
            text: "hello".to_string(),
            session_id: Some("sess-42".to_string()),
            ..RetrievalQuery::default()
        };
        let matching = make_chunk("hello world", Some("sess-42"), now());
        let other = make_chunk("hello world", Some("sess-99"), now());
        assert!(score_chunk(&matching, &q) > score_chunk(&other, &q));
    }

    #[test]
    fn test_score_chunk_recency_bonus() {
        let q = default_query("test");
        let recent = make_chunk("test content", None, now());
        let old = make_chunk("test content", None, days_ago(30));
        assert!(score_chunk(&recent, &q) > score_chunk(&old, &q));
    }

    #[test]
    fn test_score_entity_name_match() {
        let q = default_query("Alice went to the store");
        let matching = make_entity("Alice", EntityType::Person, 0.9, 1);
        let other = make_entity("Bob", EntityType::Person, 0.9, 1);
        assert!(score_entity(&matching, &q) > score_entity(&other, &q));
    }

    #[test]
    fn test_score_entity_alias_match() {
        let q = default_query("Rust is great");
        let mut with_alias = make_entity("Rust programming language", EntityType::Tool, 0.9, 1);
        with_alias.aliases = vec!["Rust".to_string()];
        let without_alias = make_entity("Python programming language", EntityType::Tool, 0.9, 1);
        assert!(score_entity(&with_alias, &q) > score_entity(&without_alias, &q));
    }

    #[test]
    fn test_score_entity_source_count() {
        let q = default_query("irrelevant text xyz");
        let high = make_entity("SomeEntity", EntityType::Concept, 0.9, 10);
        let low = make_entity("SomeEntity", EntityType::Concept, 0.9, 1);
        assert!(score_entity(&high, &q) > score_entity(&low, &q));
    }

    #[tokio::test]
    async fn test_retrieve_returns_relevant_chunks() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let graph = new_shared_graph(db.clone()).await.unwrap();
        let engine = RetrievalEngine::new(db.clone(), graph);

        db.insert_chunk(&make_chunk("Rust systems programming", None, now())).await.unwrap();
        db.insert_chunk(&make_chunk("Python machine learning", None, now())).await.unwrap();
        db.insert_chunk(&make_chunk("TypeScript web development", None, now())).await.unwrap();

        let result = engine.retrieve(&default_query("Rust")).await.unwrap();
        assert!(!result.chunks.is_empty());
        assert!(result.chunks[0].content.to_lowercase().contains("rust"));
    }

    #[tokio::test]
    async fn test_retrieve_returns_entities() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let graph = new_shared_graph(db.clone()).await.unwrap();
        let engine = RetrievalEngine::new(db.clone(), graph);

        let entity = make_entity("Alice", EntityType::Person, 0.9, 1);
        let entity_id = entity.id;
        db.upsert_entity(&entity).await.unwrap();

        let chunk = make_chunk("Alice is a great engineer", None, now());
        db.insert_chunk(&chunk).await.unwrap();
        db.link_chunk_entity(&MemoryChunkEntity {
            chunk_id: chunk.id,
            entity_id,
            mention_text: "Alice".to_string(),
            confidence: 0.9,
        }).await.unwrap();

        let result = engine.retrieve(&default_query("Alice")).await.unwrap();
        let names: Vec<&str> = result.entities.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Alice"), "Alice should appear in entities: {:?}", names);
    }

    #[tokio::test]
    async fn test_retrieve_with_graph_expansion() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());

        let a = make_entity("AlphaNode", EntityType::Concept, 0.9, 1);
        let b = make_entity("BetaNode", EntityType::Concept, 0.9, 1);
        let c = make_entity("GammaNode", EntityType::Concept, 0.9, 1);
        let (a_id, b_id, c_id) = (a.id, b.id, c.id);
        db.upsert_entity(&a).await.unwrap();
        db.upsert_entity(&b).await.unwrap();
        db.upsert_entity(&c).await.unwrap();
        db.upsert_relation(&make_relation(a_id, b_id, "connects")).await.unwrap();
        db.upsert_relation(&make_relation(b_id, c_id, "connects")).await.unwrap();

        let chunk = make_chunk("AlphaNode is the starting point", None, now());
        db.insert_chunk(&chunk).await.unwrap();
        db.link_chunk_entity(&MemoryChunkEntity {
            chunk_id: chunk.id,
            entity_id: a_id,
            mention_text: "AlphaNode".to_string(),
            confidence: 0.9,
        }).await.unwrap();

        let graph = new_shared_graph(db.clone()).await.unwrap();
        let engine = RetrievalEngine::new(db, graph);

        let query = RetrievalQuery {
            text: "AlphaNode".to_string(),
            include_graph: true,
            graph_depth: 2,
            min_confidence: 0.0,
            ..RetrievalQuery::default()
        };
        let result = engine.retrieve(&query).await.unwrap();
        let names: Vec<&str> = result.entities.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"BetaNode"), "BetaNode should appear via graph expansion: {:?}", names);
        assert!(names.contains(&"GammaNode"), "GammaNode should appear via graph expansion: {:?}", names);
    }

    #[test]
    fn test_build_context_prompt_all_sections() {
        let entity = make_entity("Alice", EntityType::Person, 0.9, 1);
        let from_id = entity.id;
        let to = make_entity("Mozilla", EntityType::Organization, 0.9, 1);
        let to_id = to.id;
        let entities = vec![entity, to];
        let relations = vec![Relation {
            id: Uuid::new_v4(),
            from_entity_id: from_id,
            to_entity_id: to_id,
            relation_type: "works_at".to_string(),
            weight: 0.8,
            attributes: json!({}),
            created_at: now(),
            updated_at: now(),
        }];
        let chunks = vec![make_chunk("Alice works at Mozilla", None, now())];

        let prompt = build_context_prompt(&chunks, &entities, &relations);
        assert!(prompt.contains("[RELEVANT FACTS]"), "missing RELEVANT FACTS");
        assert!(prompt.contains("[RELATIONSHIPS]"), "missing RELATIONSHIPS");
        assert!(prompt.contains("[RELEVANT MEMORIES]"), "missing RELEVANT MEMORIES");
        assert!(prompt.contains("=== MEMORY CONTEXT ==="));
        assert!(prompt.contains("=== END MEMORY CONTEXT ==="));
    }

    #[test]
    fn test_build_context_prompt_empty_sections_omitted() {
        let entities = vec![make_entity("Alice", EntityType::Person, 0.9, 1)];
        let prompt = build_context_prompt(&[], &entities, &[]);
        assert!(!prompt.contains("[RELATIONSHIPS]"), "RELATIONSHIPS should be omitted when empty");
        assert!(!prompt.contains("[RELEVANT MEMORIES]"), "RELEVANT MEMORIES should be omitted when empty");
        assert!(prompt.contains("[RELEVANT FACTS]"));
    }

    #[tokio::test]
    async fn test_retrieve_for_prompt_returns_string() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let graph = new_shared_graph(db.clone()).await.unwrap();
        let engine = RetrievalEngine::new(db.clone(), graph);

        db.insert_chunk(&make_chunk("Rust is awesome for systems", None, now())).await.unwrap();
        let result = engine.retrieve_for_prompt("Rust").await;
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_stats_returns_correct_counts() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let graph = new_shared_graph(db.clone()).await.unwrap();
        let engine = RetrievalEngine::new(db.clone(), graph);

        for i in 0..3 {
            db.upsert_entity(&make_entity(&format!("E{i}"), EntityType::Concept, 0.9, 1)).await.unwrap();
        }
        for i in 0..2 {
            db.insert_chunk(&make_chunk(&format!("chunk {i}"), None, now())).await.unwrap();
        }

        let (ec, cc) = engine.stats().await.unwrap();
        assert_eq!(ec, 3);
        assert_eq!(cc, 2);
    }

    proptest! {
        #[test]
        fn proptest_score_chunk_always_in_range(
            content in ".*",
            query_text in ".*",
            session_match in proptest::bool::ANY,
        ) {
            let chunk = make_chunk(
                &content,
                if session_match { Some("sess") } else { None },
                chrono::Utc::now(),
            );
            let query = RetrievalQuery {
                text: query_text,
                session_id: if session_match { Some("sess".to_string()) } else { None },
                ..RetrievalQuery::default()
            };
            let score = score_chunk(&chunk, &query);
            prop_assert!(score >= 0.0 && score <= 1.0, "score {} out of [0.0, 1.0]", score);
        }

        #[test]
        fn proptest_score_entity_always_in_range(
            name in ".*",
            aliases in prop::collection::vec(".*", 0..=5usize),
            confidence in 0.0f32..=1.0f32,
            source_count in 0i64..100i64,
        ) {
            let mut entity = make_entity(&name, EntityType::Concept, confidence, source_count);
            entity.aliases = aliases;
            let query = default_query(&name);
            let score = score_entity(&entity, &query);
            prop_assert!(score >= 0.0 && score <= 1.0, "score {} out of [0.0, 1.0]", score);
        }

        #[test]
        fn proptest_build_context_prompt_never_panics(
            contents in prop::collection::vec(".*", 0..=10usize),
            names in prop::collection::vec(".*", 0..=10usize),
        ) {
            let chunks: Vec<MemoryChunk> = contents
                .iter()
                .map(|c| make_chunk(c, None, chrono::Utc::now()))
                .collect();
            let entities: Vec<Entity> = names
                .iter()
                .map(|n| make_entity(n, EntityType::Concept, 0.9, 1))
                .collect();
            let result = build_context_prompt(&chunks, &entities, &[]);
            prop_assert!(
                std::str::from_utf8(result.as_bytes()).is_ok(),
                "output not valid UTF-8"
            );
        }
    }
}
