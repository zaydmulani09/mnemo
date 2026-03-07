use crate::db::Database;
use crate::error::Result;
use crate::models::*;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub entity_id: Uuid,
    pub name: String,
    pub entity_type: EntityType,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub relation_id: Uuid,
    pub relation_type: String,
    pub weight: f32,
}

pub struct KnowledgeGraph {
    graph: DiGraph<GraphNode, GraphEdge>,
    node_index: HashMap<Uuid, NodeIndex>,
    db: Arc<Database>,
}

pub type SharedGraph = Arc<RwLock<KnowledgeGraph>>;

pub async fn new_shared_graph(db: Arc<Database>) -> Result<SharedGraph> {
    let graph = KnowledgeGraph::new(db).await?;
    Ok(Arc::new(RwLock::new(graph)))
}

impl KnowledgeGraph {
    pub async fn new(db: Arc<Database>) -> Result<Self> {
        let mut kg = Self {
            graph: DiGraph::new(),
            node_index: HashMap::new(),
            db,
        };
        kg.load_from_db().await?;
        Ok(kg)
    }

    async fn load_from_db(&mut self) -> Result<()> {
        let entities = self.db.list_entities(10000, 0).await?;
        for entity in &entities {
            self.add_node(entity);
        }
        for entity in &entities {
            let relations = self.db.get_relations_from(entity.id).await?;
            for relation in &relations {
                self.add_edge(relation);
            }
        }
        tracing::info!(
            "graph loaded: {} nodes, {} edges",
            self.graph.node_count(),
            self.graph.edge_count()
        );
        Ok(())
    }

    pub async fn ingest(
        &mut self,
        chunk: &MemoryChunk,
        extraction: &ExtractionResult,
        db: &Database,
    ) -> Result<IngestResponse> {
        let start = Instant::now();
        let mut name_to_id: HashMap<String, Uuid> = HashMap::new();
        let mut entities_count = 0usize;

        for extracted in &extraction.entities {
            let entity = self.resolve_or_create_entity(extracted, db).await?;
            let entity_id = entity.id;
            self.add_node(&entity);
            name_to_id.insert(entity.name.clone(), entity_id);
            db.link_chunk_entity(&MemoryChunkEntity {
                chunk_id: chunk.id,
                entity_id,
                mention_text: extracted.mention_text.clone(),
                confidence: extracted.confidence,
            })
            .await?;
            entities_count += 1;
        }

        let mut relations_count = 0usize;
        for extracted_rel in &extraction.relations {
            let from_id = match name_to_id.get(&extracted_rel.from_entity) {
                Some(&id) => id,
                None => {
                    tracing::warn!(
                        "relation from_entity '{}' not resolved, skipping",
                        extracted_rel.from_entity
                    );
                    continue;
                }
            };
            let to_id = match name_to_id.get(&extracted_rel.to_entity) {
                Some(&id) => id,
                None => {
                    tracing::warn!(
                        "relation to_entity '{}' not resolved, skipping",
                        extracted_rel.to_entity
                    );
                    continue;
                }
            };
            let relation = Relation {
                id: Uuid::new_v4(),
                from_entity_id: from_id,
                to_entity_id: to_id,
                relation_type: extracted_rel.relation_type.clone(),
                weight: extracted_rel.weight,
                attributes: extracted_rel.attributes.clone(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            db.upsert_relation(&relation).await?;
            self.add_edge(&relation);
            relations_count += 1;
        }

        Ok(IngestResponse {
            chunk_id: chunk.id,
            entities_extracted: entities_count,
            relations_extracted: relations_count,
            processing_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    pub async fn resolve_or_create_entity(
        &mut self,
        extracted: &ExtractedEntity,
        db: &Database,
    ) -> Result<Entity> {
        if let Some(existing) = db.get_entity_by_name(&extracted.name).await? {
            let merged_aliases = {
                let mut a = existing.aliases.clone();
                for alias in &extracted.aliases {
                    if !a.contains(alias) {
                        a.push(alias.clone());
                    }
                }
                a
            };
            let merged_attrs =
                match (existing.attributes.as_object(), extracted.attributes.as_object()) {
                    (Some(base), Some(overlay)) => {
                        let mut m = base.clone();
                        for (k, v) in overlay {
                            m.insert(k.clone(), v.clone());
                        }
                        serde_json::Value::Object(m)
                    }
                    _ => existing.attributes.clone(),
                };
            let merged = Entity {
                aliases: merged_aliases,
                attributes: merged_attrs,
                updated_at: chrono::Utc::now(),
                ..existing.clone()
            };
            db.upsert_entity(&merged).await?;
            self.add_node(&existing);
            Ok(existing)
        } else {
            let entity = Entity {
                id: Uuid::new_v4(),
                name: extracted.name.clone(),
                entity_type: extracted.entity_type.clone(),
                aliases: extracted.aliases.clone(),
                attributes: extracted.attributes.clone(),
                confidence: extracted.confidence,
                source_count: 1,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            db.upsert_entity(&entity).await?;
            self.add_node(&entity);
            Ok(entity)
        }
    }

    pub fn add_node(&mut self, entity: &Entity) -> NodeIndex {
        if let Some(&idx) = self.node_index.get(&entity.id) {
            return idx;
        }
        let node = GraphNode {
            entity_id: entity.id,
            name: entity.name.clone(),
            entity_type: entity.entity_type.clone(),
            confidence: entity.confidence,
        };
        let idx = self.graph.add_node(node);
        self.node_index.insert(entity.id, idx);
        idx
    }

    pub fn add_edge(&mut self, relation: &Relation) {
        let from_idx = match self.node_index.get(&relation.from_entity_id) {
            Some(&idx) => idx,
            None => {
                tracing::warn!(
                    "add_edge: from_entity_id {} not in graph",
                    relation.from_entity_id
                );
                return;
            }
        };
        let to_idx = match self.node_index.get(&relation.to_entity_id) {
            Some(&idx) => idx,
            None => {
                tracing::warn!(
                    "add_edge: to_entity_id {} not in graph",
                    relation.to_entity_id
                );
                return;
            }
        };
        self.graph.add_edge(
            from_idx,
            to_idx,
            GraphEdge {
                relation_id: relation.id,
                relation_type: relation.relation_type.clone(),
                weight: relation.weight,
            },
        );
    }

    pub fn get_neighbors(&self, entity_id: Uuid, depth: usize) -> Vec<GraphNode> {
        let start_idx = match self.node_index.get(&entity_id) {
            Some(&idx) => idx,
            None => return vec![],
        };

        let mut visited: HashMap<NodeIndex, usize> = HashMap::new();
        let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();
        let mut result: Vec<GraphNode> = Vec::new();

        visited.insert(start_idx, 0);
        queue.push_back((start_idx, 0));

        while let Some((current, d)) = queue.pop_front() {
            if d > 0 {
                result.push(self.graph[current].clone());
            }
            if d < depth {
                for neighbor in self.graph.neighbors(current) {
                    if !visited.contains_key(&neighbor) {
                        visited.insert(neighbor, d + 1);
                        queue.push_back((neighbor, d + 1));
                    }
                }
            }
        }

        result
    }

    pub fn get_subgraph_entities(&self, entity_ids: &[Uuid], depth: usize) -> Vec<GraphNode> {
        let mut seen: HashSet<Uuid> = HashSet::new();
        let mut result: Vec<GraphNode> = Vec::new();
        for &id in entity_ids {
            for node in self.get_neighbors(id, depth) {
                if seen.insert(node.entity_id) {
                    result.push(node);
                }
            }
        }
        result
    }

    pub fn find_path(&self, from_id: Uuid, to_id: Uuid) -> Option<Vec<GraphNode>> {
        let from_idx = *self.node_index.get(&from_id)?;
        let to_idx = *self.node_index.get(&to_id)?;

        let mut visited: HashMap<NodeIndex, Option<NodeIndex>> = HashMap::new();
        let mut queue: VecDeque<NodeIndex> = VecDeque::new();

        visited.insert(from_idx, None);
        queue.push_back(from_idx);

        while let Some(current) = queue.pop_front() {
            if current == to_idx {
                let mut path: Vec<GraphNode> = Vec::new();
                let mut node = current;
                loop {
                    path.push(self.graph[node].clone());
                    match visited[&node] {
                        None => break,
                        Some(parent) => node = parent,
                    }
                }
                path.reverse();
                return Some(path);
            }
            for neighbor in self.graph.neighbors(current) {
                if !visited.contains_key(&neighbor) {
                    visited.insert(neighbor, Some(current));
                    queue.push_back(neighbor);
                }
            }
        }

        None
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub async fn reload(&mut self) -> Result<()> {
        self.graph.clear();
        self.node_index.clear();
        self.load_from_db().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_entity(name: &str, entity_type: EntityType) -> Entity {
        Entity {
            id: Uuid::new_v4(),
            name: name.to_string(),
            entity_type,
            aliases: vec![],
            attributes: json!({}),
            confidence: 0.9,
            source_count: 1,
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

    fn make_chunk(content: &str) -> MemoryChunk {
        MemoryChunk {
            id: Uuid::new_v4(),
            content: content.to_string(),
            source: "test".to_string(),
            session_id: None,
            embedding: None,
            metadata: json!({}),
            created_at: chrono::Utc::now(),
        }
    }

    fn make_extracted_entity(name: &str, entity_type: EntityType) -> ExtractedEntity {
        ExtractedEntity {
            name: name.to_string(),
            entity_type,
            aliases: vec![],
            attributes: json!({}),
            confidence: 0.9,
            mention_text: name.to_string(),
        }
    }

    fn make_extracted_relation(from: &str, to: &str, rel_type: &str) -> ExtractedRelation {
        ExtractedRelation {
            from_entity: from.to_string(),
            to_entity: to.to_string(),
            relation_type: rel_type.to_string(),
            weight: 0.8,
            attributes: json!({}),
        }
    }

    #[tokio::test]
    async fn test_new_graph_empty() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let graph = KnowledgeGraph::new(db).await.unwrap();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[tokio::test]
    async fn test_add_node_deduplication() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();
        let entity = make_entity("Alice", EntityType::Person);
        graph.add_node(&entity);
        graph.add_node(&entity);
        assert_eq!(graph.node_count(), 1);
    }

    #[tokio::test]
    async fn test_resolve_or_create_entity_creates_new() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db.clone()).await.unwrap();
        let extracted = make_extracted_entity("Alice", EntityType::Person);
        let entity = graph.resolve_or_create_entity(&extracted, &db).await.unwrap();
        assert_eq!(entity.name, "Alice");
        assert_eq!(graph.node_count(), 1);
        assert!(db.get_entity_by_name("Alice").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_resolve_or_create_entity_deduplicates() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db.clone()).await.unwrap();
        let extracted = make_extracted_entity("Alice", EntityType::Person);
        graph.resolve_or_create_entity(&extracted, &db).await.unwrap();
        graph.resolve_or_create_entity(&extracted, &db).await.unwrap();
        assert_eq!(db.count_entities().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_ingest_creates_entities_and_relations() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db.clone()).await.unwrap();
        let chunk = make_chunk("Alice works at Mozilla.");
        db.insert_chunk(&chunk).await.unwrap();

        let extraction = ExtractionResult {
            entities: vec![
                make_extracted_entity("Alice", EntityType::Person),
                make_extracted_entity("Mozilla", EntityType::Organization),
            ],
            relations: vec![make_extracted_relation("Alice", "Mozilla", "works_at")],
            summary: Some("Alice works at Mozilla.".to_string()),
        };

        let response = graph.ingest(&chunk, &extraction, &db).await.unwrap();
        assert_eq!(response.entities_extracted, 2);
        assert_eq!(response.relations_extracted, 1);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
        assert_eq!(db.count_entities().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_get_neighbors_single_hop() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();
        let a = make_entity("A", EntityType::Concept);
        let b = make_entity("B", EntityType::Concept);
        let c = make_entity("C", EntityType::Concept);
        let (a_id, b_id, c_id) = (a.id, b.id, c.id);
        graph.add_node(&a);
        graph.add_node(&b);
        graph.add_node(&c);
        graph.add_edge(&make_relation(a_id, b_id, "connects"));
        graph.add_edge(&make_relation(b_id, c_id, "connects"));

        let neighbors = graph.get_neighbors(a_id, 1);
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].name, "B");
    }

    #[tokio::test]
    async fn test_get_neighbors_multi_hop() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();
        let a = make_entity("A", EntityType::Concept);
        let b = make_entity("B", EntityType::Concept);
        let c = make_entity("C", EntityType::Concept);
        let (a_id, b_id, c_id) = (a.id, b.id, c.id);
        graph.add_node(&a);
        graph.add_node(&b);
        graph.add_node(&c);
        graph.add_edge(&make_relation(a_id, b_id, "connects"));
        graph.add_edge(&make_relation(b_id, c_id, "connects"));

        let neighbors = graph.get_neighbors(a_id, 2);
        assert_eq!(neighbors.len(), 2);
        let names: Vec<&str> = neighbors.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"B"));
        assert!(names.contains(&"C"));
    }

    #[tokio::test]
    async fn test_get_neighbors_unknown_entity() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let graph = KnowledgeGraph::new(db).await.unwrap();
        assert!(graph.get_neighbors(Uuid::new_v4(), 2).is_empty());
    }

    #[tokio::test]
    async fn test_find_path_exists() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();
        let a = make_entity("A", EntityType::Concept);
        let b = make_entity("B", EntityType::Concept);
        let c = make_entity("C", EntityType::Concept);
        let (a_id, b_id, c_id) = (a.id, b.id, c.id);
        graph.add_node(&a);
        graph.add_node(&b);
        graph.add_node(&c);
        graph.add_edge(&make_relation(a_id, b_id, "connects"));
        graph.add_edge(&make_relation(b_id, c_id, "connects"));

        let path = graph.find_path(a_id, c_id).unwrap();
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].name, "A");
        assert_eq!(path[2].name, "C");
    }

    #[tokio::test]
    async fn test_find_path_none() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();
        let a = make_entity("A", EntityType::Concept);
        let b = make_entity("B", EntityType::Concept);
        let (a_id, b_id) = (a.id, b.id);
        graph.add_node(&a);
        graph.add_node(&b);
        assert!(graph.find_path(a_id, b_id).is_none());
    }

    #[tokio::test]
    async fn test_reload_reflects_db_state() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db.clone()).await.unwrap();
        assert_eq!(graph.node_count(), 0);

        db.upsert_entity(&make_entity("NewEntity", EntityType::Concept))
            .await
            .unwrap();
        graph.reload().await.unwrap();
        assert_eq!(graph.node_count(), 1);
    }

    #[tokio::test]
    async fn test_node_count_and_edge_count() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();
        let entities: Vec<Entity> = (0..4)
            .map(|i| make_entity(&format!("Node{i}"), EntityType::Concept))
            .collect();
        let ids: Vec<Uuid> = entities.iter().map(|e| e.id).collect();
        for e in &entities {
            graph.add_node(e);
        }
        graph.add_edge(&make_relation(ids[0], ids[1], "r"));
        graph.add_edge(&make_relation(ids[1], ids[2], "r"));
        graph.add_edge(&make_relation(ids[2], ids[3], "r"));
        assert_eq!(graph.node_count(), 4);
        assert_eq!(graph.edge_count(), 3);
    }

    #[tokio::test]
    async fn test_get_neighbors_with_cycle() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();
        let a = make_entity("CycleA", EntityType::Concept);
        let b = make_entity("CycleB", EntityType::Concept);
        let c = make_entity("CycleC", EntityType::Concept);
        let (a_id, b_id, c_id) = (a.id, b.id, c.id);
        graph.add_node(&a);
        graph.add_node(&b);
        graph.add_node(&c);
        graph.add_edge(&make_relation(a_id, b_id, "next"));
        graph.add_edge(&make_relation(b_id, c_id, "next"));
        graph.add_edge(&make_relation(c_id, a_id, "next")); // creates cycle

        let neighbors = graph.get_neighbors(a_id, 3);
        let names: Vec<&str> = neighbors.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"CycleB"), "expected CycleB: {:?}", names);
        assert!(names.contains(&"CycleC"), "expected CycleC: {:?}", names);
        // BFS visited-set prevents infinite loop; test completes = no hang
    }

    #[tokio::test]
    async fn test_get_subgraph_entities_deduplicates() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();
        let a = make_entity("DedupA", EntityType::Concept);
        let b = make_entity("DedupB", EntityType::Concept);
        let shared = make_entity("Shared", EntityType::Concept);
        let (a_id, b_id, shared_id) = (a.id, b.id, shared.id);
        graph.add_node(&a);
        graph.add_node(&b);
        graph.add_node(&shared);
        graph.add_edge(&make_relation(a_id, shared_id, "links"));
        graph.add_edge(&make_relation(b_id, shared_id, "links"));

        let result = graph.get_subgraph_entities(&[a_id, b_id], 1);
        let shared_count = result.iter().filter(|n| n.name == "Shared").count();
        assert_eq!(shared_count, 1, "shared neighbor should appear exactly once");
    }

    #[tokio::test]
    async fn test_ingest_skips_relation_with_unknown_entity() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db.clone()).await.unwrap();
        let chunk = make_chunk("test ingest skip");
        db.insert_chunk(&chunk).await.unwrap();

        let extraction = ExtractionResult {
            entities: vec![
                make_extracted_entity("KnownA", EntityType::Concept),
                make_extracted_entity("KnownB", EntityType::Concept),
            ],
            relations: vec![
                make_extracted_relation("KnownA", "GHOST_ENTITY", "references"), // skipped
                make_extracted_relation("KnownA", "KnownB", "connects"),          // valid
            ],
            summary: None,
        };

        let response = graph.ingest(&chunk, &extraction, &db).await.unwrap();
        assert_eq!(response.entities_extracted, 2);
        assert_eq!(response.relations_extracted, 1, "only valid relation should be inserted");
        assert_eq!(graph.edge_count(), 1);
    }

    #[tokio::test]
    async fn test_large_graph_neighbors() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db).await.unwrap();

        let entities: Vec<Entity> = (0..1000)
            .map(|i| make_entity(&format!("Chain{:04}", i), EntityType::Concept))
            .collect();
        let ids: Vec<Uuid> = entities.iter().map(|e| e.id).collect();
        for e in &entities {
            graph.add_node(e);
        }
        for i in 0..999 {
            graph.add_edge(&make_relation(ids[i], ids[i + 1], "next"));
        }

        let start = std::time::Instant::now();
        let neighbors = graph.get_neighbors(ids[0], 3);
        let elapsed = start.elapsed();

        assert_eq!(neighbors.len(), 3, "expected exactly 3 neighbors at depth 3 in chain");
        assert!(elapsed.as_millis() < 100, "get_neighbors on large graph took {:?}", elapsed);
    }

    #[tokio::test]
    async fn test_reload_clears_old_nodes() {
        let db = Arc::new(Database::new_in_memory().await.unwrap());
        let mut graph = KnowledgeGraph::new(db.clone()).await.unwrap();

        let entity = make_entity("TempNode", EntityType::Concept);
        graph.add_node(&entity); // only in-memory graph, not persisted to DB
        assert_eq!(graph.node_count(), 1);

        graph.reload().await.unwrap(); // reloads from DB which has no entities
        assert_eq!(graph.node_count(), 0, "TempNode should be gone after reload from empty DB");
    }
}
