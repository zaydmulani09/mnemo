use crate::error::{MnemoError, Result};
use crate::models::{EntityType, ExtractedEntity, ExtractedRelation, ExtractionResult};
use crate::provider::{LlmConfig, LlmProvider};
use serde::Deserialize;
use serde_json::Value;

const SYSTEM_PROMPT: &str = "You are a knowledge extraction engine.\n\
From the input text:\n\
1. Extract all named entities (people, organizations, places, concepts, tools, events, documents, or other).\n\
2. Extract relationships between those entities.\n\
3. Generate a one-sentence summary of the text.\n\
\n\
Respond ONLY with valid JSON matching the exact schema provided. No explanation, no markdown, no code fences.";

const EXTRACTION_SCHEMA: &str = r#"Required JSON schema:
{
  "entities": [
    {
      "name": "string — canonical name",
      "entity_type": "Person|Organization|Place|Concept|Tool|Event|Document|Other",
      "aliases": ["string"],
      "attributes": {"key": "value"},
      "confidence": 0.0,
      "mention_text": "string — exact text from input"
    }
  ],
  "relations": [
    {
      "from_entity": "string — entity name",
      "to_entity": "string — entity name",
      "relation_type": "string — snake_case verb phrase e.g. works_at",
      "weight": 0.0,
      "attributes": {}
    }
  ],
  "summary": "string"
}"#;

pub struct Extractor {
    provider: LlmProvider,
}

impl Extractor {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            provider: LlmProvider::new(config),
        }
    }

    pub async fn extract(&self, text: &str) -> Result<ExtractionResult> {
        let user_prompt = format!("{}\n\nInput text:\n{}", EXTRACTION_SCHEMA, text);
        let raw = self.provider.complete(SYSTEM_PROMPT, &user_prompt).await?;
        parse_extraction_response(&raw)
    }

    pub async fn health_check(&self) -> bool {
        self.provider.health_check().await
    }

    pub async fn extract_with_fallback(&self, text: &str) -> ExtractionResult {
        self.extract(text).await.unwrap_or_else(|_| ExtractionResult {
            entities: vec![],
            relations: vec![],
            summary: Some("Extraction unavailable".to_string()),
        })
    }
}

fn strip_fences(raw: &str) -> String {
    raw.lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_entity_type(s: &str) -> EntityType {
    match s {
        "Person" => EntityType::Person,
        "Organization" => EntityType::Organization,
        "Place" => EntityType::Place,
        "Concept" => EntityType::Concept,
        "Tool" => EntityType::Tool,
        "Event" => EntityType::Event,
        "Document" => EntityType::Document,
        _ => EntityType::Other,
    }
}

fn clamp01(v: f32) -> f32 {
    v.max(0.0).min(1.0)
}

#[derive(Deserialize)]
struct RawExtractionResult {
    #[serde(default)]
    entities: Vec<RawEntity>,
    #[serde(default)]
    relations: Vec<RawRelation>,
    summary: Option<String>,
}

#[derive(Deserialize)]
struct RawEntity {
    name: String,
    entity_type: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    attributes: Value,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    mention_text: String,
}

#[derive(Deserialize)]
struct RawRelation {
    from_entity: String,
    to_entity: String,
    relation_type: String,
    #[serde(default)]
    weight: f32,
    #[serde(default)]
    attributes: Value,
}

fn parse_extraction_response(raw: &str) -> Result<ExtractionResult> {
    let cleaned = strip_fences(raw);
    let raw_result: RawExtractionResult = serde_json::from_str(&cleaned).map_err(|e| {
        MnemoError::Extraction(format!("JSON parse failed: {}. Raw: {}", e, raw))
    })?;

    let entities = raw_result
        .entities
        .into_iter()
        .map(|e| ExtractedEntity {
            name: e.name,
            entity_type: parse_entity_type(e.entity_type.as_deref().unwrap_or("")),
            aliases: e.aliases,
            attributes: if e.attributes.is_null() {
                serde_json::json!({})
            } else {
                e.attributes
            },
            confidence: clamp01(e.confidence),
            mention_text: e.mention_text,
        })
        .collect();

    let relations = raw_result
        .relations
        .into_iter()
        .map(|r| ExtractedRelation {
            from_entity: r.from_entity,
            to_entity: r.to_entity,
            relation_type: r.relation_type,
            weight: clamp01(r.weight),
            attributes: if r.attributes.is_null() {
                serde_json::json!({})
            } else {
                r.attributes
            },
        })
        .collect();

    Ok(ExtractionResult {
        entities,
        relations,
        summary: raw_result.summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_json() {
        let json = r#"{
            "entities": [
                {
                    "name": "Alice",
                    "entity_type": "Person",
                    "aliases": ["Al"],
                    "attributes": {"role": "engineer"},
                    "confidence": 0.9,
                    "mention_text": "Alice"
                },
                {
                    "name": "Mozilla",
                    "entity_type": "Organization",
                    "aliases": [],
                    "attributes": {},
                    "confidence": 0.85,
                    "mention_text": "Mozilla"
                }
            ],
            "relations": [
                {
                    "from_entity": "Alice",
                    "to_entity": "Mozilla",
                    "relation_type": "works_at",
                    "weight": 0.8,
                    "attributes": {}
                }
            ],
            "summary": "Alice works at Mozilla."
        }"#;
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.entities.len(), 2);
        assert_eq!(result.relations.len(), 1);
        assert_eq!(result.entities[0].name, "Alice");
        assert_eq!(result.entities[1].name, "Mozilla");
        assert_eq!(result.relations[0].relation_type, "works_at");
        assert_eq!(result.summary, Some("Alice works at Mozilla.".to_string()));
        assert!((result.entities[0].confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_strips_markdown_fences() {
        let json = "```json\n{\n    \"entities\": [{\"name\": \"Bob\", \"entity_type\": \"Person\", \"aliases\": [], \"attributes\": {}, \"confidence\": 0.7, \"mention_text\": \"Bob\"}],\n    \"relations\": [],\n    \"summary\": \"Bob exists.\"\n}\n```";
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].name, "Bob");
        assert_eq!(result.summary, Some("Bob exists.".to_string()));
    }

    #[test]
    fn test_parse_clamps_confidence() {
        let json = r#"{
            "entities": [
                {"name": "A", "entity_type": "Person", "aliases": [], "attributes": {}, "confidence": 1.5, "mention_text": "A"},
                {"name": "B", "entity_type": "Person", "aliases": [], "attributes": {}, "confidence": -0.2, "mention_text": "B"}
            ],
            "relations": [],
            "summary": null
        }"#;
        let result = parse_extraction_response(json).unwrap();
        assert!((result.entities[0].confidence - 1.0).abs() < f32::EPSILON);
        assert!((result.entities[1].confidence - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_unknown_entity_type() {
        let json = r#"{
            "entities": [
                {"name": "Enterprise", "entity_type": "Spaceship", "aliases": [], "attributes": {}, "confidence": 0.5, "mention_text": "Enterprise"}
            ],
            "relations": [],
            "summary": null
        }"#;
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.entities[0].entity_type, EntityType::Other);
    }

    #[test]
    fn test_parse_empty_entities() {
        let json = r#"{"entities": [], "relations": [], "summary": "Nothing here."}"#;
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.entities.len(), 0);
        assert_eq!(result.relations.len(), 0);
        assert_eq!(result.summary, Some("Nothing here.".to_string()));
    }

    #[test]
    fn test_parse_invalid_json_returns_error() {
        let result = parse_extraction_response("not json at all }{");
        assert!(result.is_err());
        match result.unwrap_err() {
            MnemoError::Extraction(_) => {}
            e => panic!("expected Extraction error, got {:?}", e),
        }
    }

    #[test]
    fn test_parse_missing_summary() {
        let json = r#"{"entities": [], "relations": []}"#;
        let result = parse_extraction_response(json).unwrap();
        assert!(result.summary.is_none());
    }

    #[test]
    fn test_parse_deeply_nested_attributes() {
        let json = r#"{
            "entities": [{
                "name": "DeepEntity",
                "entity_type": "Concept",
                "aliases": [],
                "attributes": {"level1": {"level2": {"level3": "deep_value"}}},
                "confidence": 0.8,
                "mention_text": "DeepEntity"
            }],
            "relations": [],
            "summary": null
        }"#;
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].name, "DeepEntity");
        assert!(result.entities[0].attributes["level1"]["level2"]["level3"].as_str() == Some("deep_value"));
    }

    #[test]
    fn test_parse_all_entity_types() {
        let json = r#"{
            "entities": [
                {"name":"A","entity_type":"Person","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"A"},
                {"name":"B","entity_type":"Organization","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"B"},
                {"name":"C","entity_type":"Place","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"C"},
                {"name":"D","entity_type":"Concept","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"D"},
                {"name":"E","entity_type":"Tool","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"E"},
                {"name":"F","entity_type":"Event","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"F"},
                {"name":"G","entity_type":"Document","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"G"},
                {"name":"H","entity_type":"Other","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"H"}
            ],
            "relations": [],
            "summary": null
        }"#;
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.entities.len(), 8);
        assert_eq!(result.entities[0].entity_type, EntityType::Person);
        assert_eq!(result.entities[1].entity_type, EntityType::Organization);
        assert_eq!(result.entities[2].entity_type, EntityType::Place);
        assert_eq!(result.entities[3].entity_type, EntityType::Concept);
        assert_eq!(result.entities[4].entity_type, EntityType::Tool);
        assert_eq!(result.entities[5].entity_type, EntityType::Event);
        assert_eq!(result.entities[6].entity_type, EntityType::Document);
        assert_eq!(result.entities[7].entity_type, EntityType::Other);
    }

    #[test]
    fn test_parse_relation_with_missing_weight() {
        // weight field absent → serde default → f32::default() = 0.0 → clamp01 = 0.0
        let json = r#"{
            "entities": [
                {"name":"X","entity_type":"Person","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"X"},
                {"name":"Y","entity_type":"Person","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"Y"}
            ],
            "relations": [{"from_entity":"X","to_entity":"Y","relation_type":"knows"}],
            "summary": null
        }"#;
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.relations.len(), 1);
        assert!((result.relations[0].weight - 0.0).abs() < f32::EPSILON,
            "missing weight should default to 0.0, got {}", result.relations[0].weight);
    }

    #[test]
    fn test_parse_empty_string_input() {
        let result = parse_extraction_response("");
        assert!(matches!(result, Err(MnemoError::Extraction(_))));
    }

    #[test]
    fn test_parse_json_array_instead_of_object() {
        let result = parse_extraction_response("[]");
        assert!(matches!(result, Err(MnemoError::Extraction(_))));
    }

    #[test]
    fn test_parse_entities_with_empty_name() {
        let json = r#"{
            "entities": [{"name":"","entity_type":"Concept","aliases":[],"attributes":{},"confidence":0.5,"mention_text":""}],
            "relations": [],
            "summary": null
        }"#;
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].name, "");
    }

    #[test]
    fn test_parse_very_long_content() {
        let long_name = "X".repeat(10_000);
        let json = format!(
            r#"{{"entities":[{{"name":"{}","entity_type":"Concept","aliases":[],"attributes":{{}},"confidence":0.9,"mention_text":"{}"}}],"relations":[],"summary":null}}"#,
            long_name, long_name
        );
        let result = parse_extraction_response(&json).unwrap();
        assert_eq!(result.entities[0].name.len(), 10_000, "long name should not be truncated");
    }

    #[test]
    fn test_parse_unicode_content() {
        let json = r#"{
            "entities": [{"name":"日本語テスト","entity_type":"Concept","aliases":[],"attributes":{},"confidence":0.9,"mention_text":"日本語テスト"}],
            "relations": [{"from_entity":"日本語テスト","to_entity":"日本語テスト","relation_type":"関係","weight":0.5,"attributes":{}}],
            "summary": null
        }"#;
        let result = parse_extraction_response(json).unwrap();
        assert_eq!(result.entities[0].name, "日本語テスト");
        assert_eq!(result.relations[0].relation_type, "関係");
    }
}
