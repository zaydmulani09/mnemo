use crate::error::{MnemoError, Result};
use crate::models::*;
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(db_path: &str) -> Result<Self> {
        let url = format!("sqlite://{}?mode=rwc", db_path);
        let pool = SqlitePool::connect(&url).await?;
        Self::apply_pragmas_and_migrate(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn new_in_memory() -> Result<Self> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        Self::apply_pragmas_and_migrate(&pool).await?;
        Ok(Self { pool })
    }

    async fn apply_pragmas_and_migrate(pool: &SqlitePool) -> Result<()> {
        sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
        sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;
        sqlx::query("PRAGMA busy_timeout=5000").execute(pool).await?;
        sqlx::migrate!()
            .run(pool)
            .await
            .map_err(|e| MnemoError::Config(e.to_string()))?;
        Ok(())
    }

    // ── Entity methods ──────────────────────────────────────────────────────

    pub async fn upsert_entity(&self, entity: &Entity) -> Result<()> {
        let entity_type_str = serde_json::to_string(&entity.entity_type)?;

        let existing_row = sqlx::query(
            "SELECT id FROM entities WHERE name = ? AND entity_type = ? LIMIT 1",
        )
        .bind(&entity.name)
        .bind(&entity_type_str)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = existing_row {
            let existing_id: String = row.try_get("id")?;
            let existing_id_uuid = Uuid::parse_str(&existing_id)
                .map_err(|e| MnemoError::Graph(e.to_string()))?;

            let existing = self
                .get_entity_by_id(existing_id_uuid)
                .await?
                .ok_or_else(|| MnemoError::NotFound(existing_id.clone()))?;

            let mut merged_aliases = existing.aliases.clone();
            for alias in &entity.aliases {
                if !merged_aliases.contains(alias) {
                    merged_aliases.push(alias.clone());
                }
            }
            let merged_aliases_str = serde_json::to_string(&merged_aliases)?;

            let merged_attrs = match (existing.attributes.as_object(), entity.attributes.as_object()) {
                (Some(base), Some(overlay)) => {
                    let mut merged = base.clone();
                    for (k, v) in overlay {
                        merged.insert(k.clone(), v.clone());
                    }
                    serde_json::Value::Object(merged)
                }
                _ => entity.attributes.clone(),
            };
            let merged_attrs_str = serde_json::to_string(&merged_attrs)?;
            let updated_at_str = entity.updated_at.to_rfc3339();

            sqlx::query(
                "UPDATE entities
                 SET source_count = source_count + 1,
                     updated_at   = ?,
                     aliases      = ?,
                     attributes   = ?
                 WHERE id = ?",
            )
            .bind(&updated_at_str)
            .bind(&merged_aliases_str)
            .bind(&merged_attrs_str)
            .bind(&existing_id)
            .execute(&self.pool)
            .await?;
        } else {
            let aliases_str = serde_json::to_string(&entity.aliases)?;
            let attributes_str = serde_json::to_string(&entity.attributes)?;

            sqlx::query(
                "INSERT INTO entities
                     (id, name, entity_type, aliases, attributes, confidence, source_count, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(entity.id.to_string())
            .bind(&entity.name)
            .bind(&entity_type_str)
            .bind(&aliases_str)
            .bind(&attributes_str)
            .bind(entity.confidence as f64)
            .bind(entity.source_count)
            .bind(entity.created_at.to_rfc3339())
            .bind(entity.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn get_entity_by_id(&self, id: Uuid) -> Result<Option<Entity>> {
        let row = sqlx::query("SELECT * FROM entities WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_entity(&r)).transpose()
    }

    pub async fn get_entity_by_name(&self, name: &str) -> Result<Option<Entity>> {
        let row = sqlx::query("SELECT * FROM entities WHERE name = ? LIMIT 1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_entity(&r)).transpose()
    }

    pub async fn list_entities(&self, limit: i64, offset: i64) -> Result<Vec<Entity>> {
        let rows =
            sqlx::query("SELECT * FROM entities ORDER BY created_at DESC LIMIT ? OFFSET ?")
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(|r| row_to_entity(r)).collect()
    }

    pub async fn search_entities_by_name(&self, query: &str, limit: i64) -> Result<Vec<Entity>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query("SELECT * FROM entities WHERE name LIKE ? LIMIT ?")
            .bind(&pattern)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| row_to_entity(r)).collect()
    }

    pub async fn delete_entity(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM entities WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn count_entities(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM entities")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("count")?)
    }

    // ── Relation methods ────────────────────────────────────────────────────

    pub async fn upsert_relation(&self, relation: &Relation) -> Result<()> {
        let attributes_str = serde_json::to_string(&relation.attributes)?;

        sqlx::query(
            "INSERT INTO relations
                 (id, from_entity_id, to_entity_id, relation_type, weight, attributes, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(from_entity_id, to_entity_id, relation_type) DO UPDATE SET
                 weight     = MIN(1.0, weight + 0.1),
                 updated_at = excluded.updated_at",
        )
        .bind(relation.id.to_string())
        .bind(relation.from_entity_id.to_string())
        .bind(relation.to_entity_id.to_string())
        .bind(&relation.relation_type)
        .bind(relation.weight as f64)
        .bind(&attributes_str)
        .bind(relation.created_at.to_rfc3339())
        .bind(relation.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_relations_from(&self, entity_id: Uuid) -> Result<Vec<Relation>> {
        let rows = sqlx::query("SELECT * FROM relations WHERE from_entity_id = ?")
            .bind(entity_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| row_to_relation(r)).collect()
    }

    pub async fn get_relations_to(&self, entity_id: Uuid) -> Result<Vec<Relation>> {
        let rows = sqlx::query("SELECT * FROM relations WHERE to_entity_id = ?")
            .bind(entity_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| row_to_relation(r)).collect()
    }

    pub async fn get_relations_between(&self, from_id: Uuid, to_id: Uuid) -> Result<Vec<Relation>> {
        let rows = sqlx::query(
            "SELECT * FROM relations WHERE from_entity_id = ? AND to_entity_id = ?",
        )
        .bind(from_id.to_string())
        .bind(to_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|r| row_to_relation(r)).collect()
    }

    pub async fn delete_relation(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM relations WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Chunk methods ───────────────────────────────────────────────────────

    pub async fn insert_chunk(&self, chunk: &MemoryChunk) -> Result<()> {
        let embedding_str = chunk
            .embedding
            .as_ref()
            .map(|e| serde_json::to_string(e))
            .transpose()?;
        let metadata_str = serde_json::to_string(&chunk.metadata)?;

        sqlx::query(
            "INSERT INTO memory_chunks
                 (id, content, source, session_id, embedding, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(chunk.id.to_string())
        .bind(&chunk.content)
        .bind(&chunk.source)
        .bind(&chunk.session_id)
        .bind(&embedding_str)
        .bind(&metadata_str)
        .bind(chunk.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_chunk_by_id(&self, id: Uuid) -> Result<Option<MemoryChunk>> {
        let row = sqlx::query("SELECT * FROM memory_chunks WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| row_to_chunk(&r)).transpose()
    }

    pub async fn list_chunks(&self, limit: i64, offset: i64) -> Result<Vec<MemoryChunk>> {
        let rows = sqlx::query(
            "SELECT * FROM memory_chunks ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|r| row_to_chunk(r)).collect()
    }

    pub async fn list_chunks_by_session(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<MemoryChunk>> {
        let rows = sqlx::query(
            "SELECT * FROM memory_chunks WHERE session_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|r| row_to_chunk(r)).collect()
    }

    pub async fn search_chunks_by_content(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MemoryChunk>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query("SELECT * FROM memory_chunks WHERE content LIKE ? LIMIT ?")
            .bind(&pattern)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| row_to_chunk(r)).collect()
    }

    pub async fn delete_chunk(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM memory_chunks WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn count_chunks(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM memory_chunks")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("count")?)
    }

    // ── Join table methods ──────────────────────────────────────────────────

    pub async fn link_chunk_entity(&self, link: &MemoryChunkEntity) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO memory_chunk_entities
                 (chunk_id, entity_id, mention_text, confidence)
             VALUES (?, ?, ?, ?)",
        )
        .bind(link.chunk_id.to_string())
        .bind(link.entity_id.to_string())
        .bind(&link.mention_text)
        .bind(link.confidence as f64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_entities_for_chunk(&self, chunk_id: Uuid) -> Result<Vec<Entity>> {
        let rows = sqlx::query(
            "SELECT e.* FROM entities e
             INNER JOIN memory_chunk_entities mce ON mce.entity_id = e.id
             WHERE mce.chunk_id = ?",
        )
        .bind(chunk_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|r| row_to_entity(r)).collect()
    }

    pub async fn get_chunks_for_entity(
        &self,
        entity_id: Uuid,
        limit: i64,
    ) -> Result<Vec<MemoryChunk>> {
        let rows = sqlx::query(
            "SELECT mc.* FROM memory_chunks mc
             INNER JOIN memory_chunk_entities mce ON mce.chunk_id = mc.id
             WHERE mce.entity_id = ?
             ORDER BY mc.created_at DESC
             LIMIT ?",
        )
        .bind(entity_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|r| row_to_chunk(r)).collect()
    }

    // ── Utility ─────────────────────────────────────────────────────────────

    pub async fn wipe_all(&self) -> Result<()> {
        sqlx::query("DELETE FROM memory_chunk_entities")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM memory_chunks")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM relations")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM entities")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn health_check(&self) -> Result<bool> {
        sqlx::query("SELECT 1").fetch_one(&self.pool).await?;
        Ok(true)
    }
}

// ── Row parsers ──────────────────────────────────────────────────────────────

fn row_to_entity(row: &sqlx::sqlite::SqliteRow) -> Result<Entity> {
    let id_str: String = row.try_get("id")?;
    let id = Uuid::parse_str(&id_str).map_err(|e| MnemoError::Graph(e.to_string()))?;

    let entity_type_str: String = row.try_get("entity_type")?;
    let entity_type: EntityType = serde_json::from_str(&entity_type_str)?;

    let aliases_str: String = row.try_get("aliases")?;
    let aliases: Vec<String> = serde_json::from_str(&aliases_str)?;

    let attributes_str: String = row.try_get("attributes")?;
    let attributes: serde_json::Value = serde_json::from_str(&attributes_str)?;

    let confidence_f64: f64 = row.try_get("confidence")?;
    let source_count: i64 = row.try_get("source_count")?;

    let created_at_str: String = row.try_get("created_at")?;
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| MnemoError::Config(e.to_string()))?
        .with_timezone(&Utc);

    let updated_at_str: String = row.try_get("updated_at")?;
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map_err(|e| MnemoError::Config(e.to_string()))?
        .with_timezone(&Utc);

    Ok(Entity {
        id,
        name: row.try_get("name")?,
        entity_type,
        aliases,
        attributes,
        confidence: confidence_f64 as f32,
        source_count,
        created_at,
        updated_at,
    })
}

fn row_to_relation(row: &sqlx::sqlite::SqliteRow) -> Result<Relation> {
    let id_str: String = row.try_get("id")?;
    let id = Uuid::parse_str(&id_str).map_err(|e| MnemoError::Graph(e.to_string()))?;

    let from_str: String = row.try_get("from_entity_id")?;
    let from_entity_id = Uuid::parse_str(&from_str).map_err(|e| MnemoError::Graph(e.to_string()))?;

    let to_str: String = row.try_get("to_entity_id")?;
    let to_entity_id = Uuid::parse_str(&to_str).map_err(|e| MnemoError::Graph(e.to_string()))?;

    let weight_f64: f64 = row.try_get("weight")?;

    let attributes_str: String = row.try_get("attributes")?;
    let attributes: serde_json::Value = serde_json::from_str(&attributes_str)?;

    let created_at_str: String = row.try_get("created_at")?;
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| MnemoError::Config(e.to_string()))?
        .with_timezone(&Utc);

    let updated_at_str: String = row.try_get("updated_at")?;
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map_err(|e| MnemoError::Config(e.to_string()))?
        .with_timezone(&Utc);

    Ok(Relation {
        id,
        from_entity_id,
        to_entity_id,
        relation_type: row.try_get("relation_type")?,
        weight: weight_f64 as f32,
        attributes,
        created_at,
        updated_at,
    })
}

fn row_to_chunk(row: &sqlx::sqlite::SqliteRow) -> Result<MemoryChunk> {
    let id_str: String = row.try_get("id")?;
    let id = Uuid::parse_str(&id_str).map_err(|e| MnemoError::Graph(e.to_string()))?;

    let embedding_str: Option<String> = row.try_get("embedding")?;
    let embedding = embedding_str
        .map(|s| serde_json::from_str::<Vec<f32>>(&s))
        .transpose()?;

    let metadata_str: String = row.try_get("metadata")?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata_str)?;

    let created_at_str: String = row.try_get("created_at")?;
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| MnemoError::Config(e.to_string()))?
        .with_timezone(&Utc);

    Ok(MemoryChunk {
        id,
        content: row.try_get("content")?,
        source: row.try_get("source")?,
        session_id: row.try_get("session_id")?,
        embedding,
        metadata,
        created_at,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

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
            attributes: json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_chunk(content: &str, session: Option<&str>) -> MemoryChunk {
        MemoryChunk {
            id: Uuid::new_v4(),
            content: content.to_string(),
            source: "test".to_string(),
            session_id: session.map(|s| s.to_string()),
            embedding: None,
            metadata: json!({}),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_upsert_and_get_entity() {
        let db = Database::new_in_memory().await.unwrap();
        let entity = make_entity("Alice", EntityType::Person);
        let id = entity.id;
        db.upsert_entity(&entity).await.unwrap();

        let fetched = db.get_entity_by_id(id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Alice");
        assert_eq!(fetched.source_count, 1);
        assert_eq!(fetched.id, id);
    }

    #[tokio::test]
    async fn test_entity_upsert_increments_source_count() {
        let db = Database::new_in_memory().await.unwrap();
        let entity = make_entity("Bob", EntityType::Person);
        db.upsert_entity(&entity).await.unwrap();
        db.upsert_entity(&entity).await.unwrap();

        let fetched = db.get_entity_by_name("Bob").await.unwrap().unwrap();
        assert_eq!(fetched.source_count, 2);
    }

    #[tokio::test]
    async fn test_search_entities_by_name() {
        let db = Database::new_in_memory().await.unwrap();
        db.upsert_entity(&make_entity("Rust Language", EntityType::Tool))
            .await
            .unwrap();
        db.upsert_entity(&make_entity("Python Language", EntityType::Tool))
            .await
            .unwrap();
        db.upsert_entity(&make_entity("Carol Smith", EntityType::Person))
            .await
            .unwrap();

        let results = db.search_entities_by_name("Language", 10).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.name.contains("Language")));
    }

    #[tokio::test]
    async fn test_upsert_and_get_relation() {
        let db = Database::new_in_memory().await.unwrap();
        let e1 = make_entity("Dave", EntityType::Person);
        let e2 = make_entity("Anthropic", EntityType::Organization);
        let e1_id = e1.id;
        let e2_id = e2.id;
        db.upsert_entity(&e1).await.unwrap();
        db.upsert_entity(&e2).await.unwrap();

        let rel = make_relation(e1_id, e2_id, "works_at");
        db.upsert_relation(&rel).await.unwrap();

        let rels = db.get_relations_from(e1_id).await.unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation_type, "works_at");
        assert_eq!(rels[0].to_entity_id, e2_id);
    }

    #[tokio::test]
    async fn test_relation_weight_increases_on_duplicate() {
        let db = Database::new_in_memory().await.unwrap();
        let e1 = make_entity("Eve", EntityType::Person);
        let e2 = make_entity("OpenAI", EntityType::Organization);
        let e1_id = e1.id;
        let e2_id = e2.id;
        db.upsert_entity(&e1).await.unwrap();
        db.upsert_entity(&e2).await.unwrap();

        let rel = make_relation(e1_id, e2_id, "uses");
        db.upsert_relation(&rel).await.unwrap();
        db.upsert_relation(&rel).await.unwrap();

        let rels = db.get_relations_from(e1_id).await.unwrap();
        assert_eq!(rels.len(), 1);
        assert!(rels[0].weight > 0.8 + f32::EPSILON);
    }

    #[tokio::test]
    async fn test_insert_and_get_chunk() {
        let db = Database::new_in_memory().await.unwrap();
        let chunk = make_chunk("Alice works at Mozilla on Rust.", Some("sess-1"));
        let id = chunk.id;
        db.insert_chunk(&chunk).await.unwrap();

        let fetched = db.get_chunk_by_id(id).await.unwrap().unwrap();
        assert_eq!(fetched.content, chunk.content);
        assert_eq!(fetched.session_id, chunk.session_id);
        assert_eq!(fetched.id, id);
    }

    #[tokio::test]
    async fn test_list_chunks_by_session() {
        let db = Database::new_in_memory().await.unwrap();
        db.insert_chunk(&make_chunk("chunk A", Some("sess-A")))
            .await
            .unwrap();
        db.insert_chunk(&make_chunk("chunk B", Some("sess-A")))
            .await
            .unwrap();
        db.insert_chunk(&make_chunk("chunk C", Some("sess-B")))
            .await
            .unwrap();

        let sess_a = db.list_chunks_by_session("sess-A", 10).await.unwrap();
        assert_eq!(sess_a.len(), 2);

        let sess_b = db.list_chunks_by_session("sess-B", 10).await.unwrap();
        assert_eq!(sess_b.len(), 1);
    }

    #[tokio::test]
    async fn test_search_chunks_by_content() {
        let db = Database::new_in_memory().await.unwrap();
        db.insert_chunk(&make_chunk("Rust is a systems language", None))
            .await
            .unwrap();
        db.insert_chunk(&make_chunk("Python is great for ML", None))
            .await
            .unwrap();
        db.insert_chunk(&make_chunk("TypeScript adds types to JS", None))
            .await
            .unwrap();

        let results = db.search_chunks_by_content("systems", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("systems"));
    }

    #[tokio::test]
    async fn test_link_chunk_entity_and_retrieve() {
        let db = Database::new_in_memory().await.unwrap();
        let entity = make_entity("Frank", EntityType::Person);
        let chunk = make_chunk("Frank wrote a paper.", None);
        let entity_id = entity.id;
        let chunk_id = chunk.id;

        db.upsert_entity(&entity).await.unwrap();
        db.insert_chunk(&chunk).await.unwrap();

        let link = MemoryChunkEntity {
            chunk_id,
            entity_id,
            mention_text: "Frank".to_string(),
            confidence: 0.95,
        };
        db.link_chunk_entity(&link).await.unwrap();

        let entities = db.get_entities_for_chunk(chunk_id).await.unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Frank");
    }

    #[tokio::test]
    async fn test_get_chunks_for_entity() {
        let db = Database::new_in_memory().await.unwrap();
        let entity = make_entity("Grace", EntityType::Person);
        let entity_id = entity.id;
        db.upsert_entity(&entity).await.unwrap();

        for i in 0..3 {
            let chunk = make_chunk(&format!("Grace did thing {}", i), None);
            let chunk_id = chunk.id;
            db.insert_chunk(&chunk).await.unwrap();
            db.link_chunk_entity(&MemoryChunkEntity {
                chunk_id,
                entity_id,
                mention_text: "Grace".to_string(),
                confidence: 0.9,
            })
            .await
            .unwrap();
        }

        let chunks = db.get_chunks_for_entity(entity_id, 10).await.unwrap();
        assert_eq!(chunks.len(), 3);
    }

    #[tokio::test]
    async fn test_delete_entity_cascades() {
        let db = Database::new_in_memory().await.unwrap();
        let e1 = make_entity("Henry", EntityType::Person);
        let e2 = make_entity("ACME Corp", EntityType::Organization);
        let e1_id = e1.id;
        let e2_id = e2.id;
        db.upsert_entity(&e1).await.unwrap();
        db.upsert_entity(&e2).await.unwrap();

        db.upsert_relation(&make_relation(e1_id, e2_id, "employed_by"))
            .await
            .unwrap();

        let chunk = make_chunk("Henry works at ACME.", None);
        let chunk_id = chunk.id;
        db.insert_chunk(&chunk).await.unwrap();
        db.link_chunk_entity(&MemoryChunkEntity {
            chunk_id,
            entity_id: e1_id,
            mention_text: "Henry".to_string(),
            confidence: 0.9,
        })
        .await
        .unwrap();

        db.delete_entity(e1_id).await.unwrap();

        let rels = db.get_relations_from(e1_id).await.unwrap();
        assert!(rels.is_empty());

        let entities_for_chunk = db.get_entities_for_chunk(chunk_id).await.unwrap();
        assert!(entities_for_chunk.is_empty());

        assert!(db.get_entity_by_id(e1_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_wipe_all() {
        let db = Database::new_in_memory().await.unwrap();
        let e = make_entity("Iris", EntityType::Person);
        let e_id = e.id;
        db.upsert_entity(&e).await.unwrap();

        let chunk = make_chunk("Iris wrote code.", None);
        let chunk_id = chunk.id;
        db.insert_chunk(&chunk).await.unwrap();
        db.link_chunk_entity(&MemoryChunkEntity {
            chunk_id,
            entity_id: e_id,
            mention_text: "Iris".to_string(),
            confidence: 1.0,
        })
        .await
        .unwrap();

        db.wipe_all().await.unwrap();

        assert_eq!(db.count_entities().await.unwrap(), 0);
        assert_eq!(db.count_chunks().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_count_entities_and_chunks() {
        let db = Database::new_in_memory().await.unwrap();

        for i in 0..5 {
            db.upsert_entity(&make_entity(&format!("Entity{}", i), EntityType::Concept))
                .await
                .unwrap();
        }
        for i in 0..3 {
            db.insert_chunk(&make_chunk(&format!("chunk content {}", i), None))
                .await
                .unwrap();
        }

        assert_eq!(db.count_entities().await.unwrap(), 5);
        assert_eq!(db.count_chunks().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_health_check() {
        let db = Database::new_in_memory().await.unwrap();
        assert!(db.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn test_upsert_entity_merges_aliases_correctly() {
        let db = Database::new_in_memory().await.unwrap();
        let mut e1 = make_entity("AliasTest", EntityType::Concept);
        e1.aliases = vec!["a".to_string(), "b".to_string()];
        db.upsert_entity(&e1).await.unwrap();

        let mut e2 = make_entity("AliasTest", EntityType::Concept);
        e2.aliases = vec!["b".to_string(), "c".to_string()];
        db.upsert_entity(&e2).await.unwrap();

        let fetched = db.get_entity_by_name("AliasTest").await.unwrap().unwrap();
        let mut aliases = fetched.aliases.clone();
        aliases.sort();
        assert_eq!(aliases, vec!["a", "b", "c"], "merged aliases should be deduped union");
    }

    #[tokio::test]
    async fn test_upsert_relation_weight_cap() {
        let db = Database::new_in_memory().await.unwrap();
        let e1 = make_entity("WeightFrom", EntityType::Concept);
        let e2 = make_entity("WeightTo", EntityType::Concept);
        let (e1_id, e2_id) = (e1.id, e2.id);
        db.upsert_entity(&e1).await.unwrap();
        db.upsert_entity(&e2).await.unwrap();

        let rel = make_relation(e1_id, e2_id, "weight_test");
        for _ in 0..20 {
            db.upsert_relation(&rel).await.unwrap();
        }

        let rels = db.get_relations_from(e1_id).await.unwrap();
        assert_eq!(rels.len(), 1);
        assert!(
            rels[0].weight <= 1.0 + f32::EPSILON,
            "weight {} exceeds 1.0",
            rels[0].weight
        );
    }

    #[tokio::test]
    async fn test_list_entities_pagination() {
        let db = Database::new_in_memory().await.unwrap();
        for i in 0..25 {
            db.upsert_entity(&make_entity(&format!("PagEntity{:02}", i), EntityType::Concept))
                .await
                .unwrap();
        }

        let page1 = db.list_entities(10, 0).await.unwrap();
        let page2 = db.list_entities(10, 10).await.unwrap();
        assert_eq!(page1.len(), 10);
        assert_eq!(page2.len(), 10);

        let ids1: std::collections::HashSet<Uuid> = page1.iter().map(|e| e.id).collect();
        let ids2: std::collections::HashSet<Uuid> = page2.iter().map(|e| e.id).collect();
        assert!(ids1.is_disjoint(&ids2), "entity pagination pages should not overlap");
    }

    #[tokio::test]
    async fn test_list_chunks_pagination() {
        let db = Database::new_in_memory().await.unwrap();
        for i in 0..25 {
            db.insert_chunk(&make_chunk(&format!("pag chunk {}", i), None))
                .await
                .unwrap();
        }

        let page1 = db.list_chunks(10, 0).await.unwrap();
        let page2 = db.list_chunks(10, 10).await.unwrap();
        assert_eq!(page1.len(), 10);
        assert_eq!(page2.len(), 10);

        let ids1: std::collections::HashSet<Uuid> = page1.iter().map(|c| c.id).collect();
        let ids2: std::collections::HashSet<Uuid> = page2.iter().map(|c| c.id).collect();
        assert!(ids1.is_disjoint(&ids2), "chunk pagination pages should not overlap");
    }

    #[tokio::test]
    async fn test_get_chunks_for_entity_respects_limit() {
        let db = Database::new_in_memory().await.unwrap();
        let entity = make_entity("LimitEntity", EntityType::Concept);
        let entity_id = entity.id;
        db.upsert_entity(&entity).await.unwrap();

        for i in 0..10 {
            let chunk = make_chunk(&format!("chunk {}", i), None);
            let chunk_id = chunk.id;
            db.insert_chunk(&chunk).await.unwrap();
            db.link_chunk_entity(&MemoryChunkEntity {
                chunk_id,
                entity_id,
                mention_text: "LimitEntity".to_string(),
                confidence: 0.9,
            })
            .await
            .unwrap();
        }

        let chunks = db.get_chunks_for_entity(entity_id, 5).await.unwrap();
        assert_eq!(chunks.len(), 5, "should return exactly 5 chunks with limit=5");
    }

    #[tokio::test]
    async fn test_concurrent_upserts() {
        let db = std::sync::Arc::new(Database::new_in_memory().await.unwrap());

        let mut handles = Vec::new();
        for _ in 0..10 {
            let db_clone = db.clone();
            handles.push(tokio::spawn(async move {
                // Each task creates its own UUID to avoid conflicting INSERTs;
                // the upsert deduplicates by name+type so tasks may see each other's rows.
                let entity = make_entity("ConcurrentEntity", EntityType::Concept);
                let _ = db_clone.upsert_entity(&entity).await; // tolerate TOCTOU races
            }));
        }

        for handle in handles {
            handle.await.unwrap(); // assert no task panicked
        }

        // DB still functional and has at least one entity
        let count = db.count_entities().await.unwrap();
        assert!(count > 0, "at least one entity should exist after concurrent upserts");
    }

    #[tokio::test]
    async fn test_insert_chunk_with_embedding() {
        let db = Database::new_in_memory().await.unwrap();
        let embedding: Vec<f32> = (0..128).map(|i| i as f32 / 128.0).collect();
        let chunk = MemoryChunk {
            id: Uuid::new_v4(),
            content: "embedding test chunk".to_string(),
            source: "test".to_string(),
            session_id: None,
            embedding: Some(embedding.clone()),
            metadata: json!({}),
            created_at: Utc::now(),
        };
        let chunk_id = chunk.id;
        db.insert_chunk(&chunk).await.unwrap();

        let fetched = db.get_chunk_by_id(chunk_id).await.unwrap().unwrap();
        let fetched_embedding = fetched.embedding.expect("embedding should round-trip");
        assert_eq!(fetched_embedding.len(), 128);
        for (a, b) in embedding.iter().zip(fetched_embedding.iter()) {
            assert!((a - b).abs() < 1e-5, "embedding value mismatch: {} vs {}", a, b);
        }
    }

    #[tokio::test]
    async fn test_wipe_all_then_reinsert() {
        let db = Database::new_in_memory().await.unwrap();
        db.upsert_entity(&make_entity("WipeTest", EntityType::Concept)).await.unwrap();
        db.insert_chunk(&make_chunk("wipe test content", None)).await.unwrap();

        db.wipe_all().await.unwrap();
        assert_eq!(db.count_entities().await.unwrap(), 0);
        assert_eq!(db.count_chunks().await.unwrap(), 0);

        db.upsert_entity(&make_entity("Fresh", EntityType::Person)).await.unwrap();
        db.insert_chunk(&make_chunk("fresh content", None)).await.unwrap();

        assert_eq!(db.count_entities().await.unwrap(), 1, "should have 1 entity after reinsert");
        assert_eq!(db.count_chunks().await.unwrap(), 1, "should have 1 chunk after reinsert");
    }
}
