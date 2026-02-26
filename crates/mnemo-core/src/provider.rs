use crate::error::{MnemoError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

// ── Provider type ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Ollama,
    #[serde(rename = "openai")]
    OpenAi,
    Anthropic,
    Custom,
}

impl ProviderType {
    pub fn default_base_url(&self) -> &str {
        match self {
            ProviderType::Ollama => "http://localhost:11434/v1",
            ProviderType::OpenAi => "https://api.openai.com/v1",
            ProviderType::Anthropic => "https://api.anthropic.com/v1",
            ProviderType::Custom => "http://localhost:8000/v1",
        }
    }

    pub fn default_model(&self) -> &str {
        match self {
            ProviderType::Ollama => "llama3",
            ProviderType::OpenAi => "gpt-4o-mini",
            ProviderType::Anthropic => "claude-haiku-4-5-20251001",
            ProviderType::Custom => "default",
        }
    }

    pub fn requires_api_key(&self) -> bool {
        matches!(self, ProviderType::OpenAi | ProviderType::Anthropic)
    }
}

impl Default for ProviderType {
    fn default() -> Self {
        ProviderType::Ollama
    }
}

// ── LlmConfig ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: ProviderType,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub max_tokens: u32,
    pub temperature: f32,
    pub system_prompt_prefix: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        let provider = ProviderType::Ollama;
        Self {
            base_url: provider.default_base_url().to_string(),
            model: provider.default_model().to_string(),
            provider,
            api_key: "ollama".to_string(),
            timeout_secs: 30,
            max_retries: 3,
            max_tokens: 2048,
            temperature: 0.1,
            system_prompt_prefix: None,
        }
    }
}

impl LlmConfig {
    pub fn ollama(model: impl Into<String>) -> Self {
        Self {
            provider: ProviderType::Ollama,
            base_url: ProviderType::Ollama.default_base_url().to_string(),
            model: model.into(),
            api_key: "ollama".to_string(),
            ..Default::default()
        }
    }

    pub fn openai(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: ProviderType::OpenAi,
            base_url: ProviderType::OpenAi.default_base_url().to_string(),
            model: model.into(),
            api_key: api_key.into(),
            ..Default::default()
        }
    }

    pub fn anthropic(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: ProviderType::Anthropic,
            base_url: ProviderType::Anthropic.default_base_url().to_string(),
            model: model.into(),
            api_key: api_key.into(),
            ..Default::default()
        }
    }

    pub fn from_env() -> Self {
        let provider_str = std::env::var("MNEMO_LLM_PROVIDER")
            .unwrap_or_else(|_| "ollama".into())
            .to_lowercase();
        let provider = match provider_str.as_str() {
            "openai" => ProviderType::OpenAi,
            "anthropic" => ProviderType::Anthropic,
            "custom" => ProviderType::Custom,
            _ => ProviderType::Ollama,
        };
        Self {
            base_url: std::env::var("MNEMO_LLM_BASE_URL")
                .unwrap_or_else(|_| provider.default_base_url().to_string()),
            model: std::env::var("MNEMO_LLM_MODEL")
                .unwrap_or_else(|_| provider.default_model().to_string()),
            api_key: std::env::var("MNEMO_LLM_API_KEY")
                .unwrap_or_else(|_| "ollama".into()),
            provider,
            ..Default::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.provider.requires_api_key() && self.api_key == "ollama" {
            return Err(MnemoError::Config(format!(
                "{:?} provider requires a real API key",
                self.provider
            )));
        }
        if self.base_url.is_empty() {
            return Err(MnemoError::Config("base_url cannot be empty".into()));
        }
        if self.model.is_empty() {
            return Err(MnemoError::Config("model cannot be empty".into()));
        }
        Ok(())
    }
}

// ── MnemoConfig (TOML-backed full config) ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MnemoConfig {
    pub db_path: String,
    pub port: u16,
    pub llm: LlmConfig,
}

impl Default for MnemoConfig {
    fn default() -> Self {
        Self {
            db_path: "mnemo.db".to_string(),
            port: 8080,
            llm: LlmConfig::default(),
        }
    }
}

impl MnemoConfig {
    pub fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| MnemoError::Config(format!("cannot read config file: {}", e)))?;
        toml::from_str(&content)
            .map_err(|e| MnemoError::Config(format!("invalid TOML config: {}", e)))
    }

    pub fn from_env_or_file(file_path: Option<&str>) -> Self {
        if let Some(path) = file_path {
            if std::path::Path::new(path).exists() {
                match Self::from_file(path) {
                    Ok(cfg) => return cfg,
                    Err(e) => tracing::warn!("config file error: {}, falling back to env", e),
                }
            }
        }
        Self {
            db_path: std::env::var("MNEMO_DB_PATH").unwrap_or_else(|_| "mnemo.db".into()),
            port: std::env::var("MNEMO_PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()
                .unwrap_or(8080),
            llm: LlmConfig::from_env(),
        }
    }

    pub fn to_example_toml() -> &'static str {
        r#"
db_path = "mnemo.db"
port = 8080

[llm]
provider = "ollama"
base_url = "http://localhost:11434/v1"
model = "llama3"
api_key = "ollama"
timeout_secs = 30
max_retries = 3
max_tokens = 2048
temperature = 0.1
"#
    }
}

// ── LlmProvider ──────────────────────────────────────────────────────────────

pub struct LlmProvider {
    config: LlmConfig,
    client: Client,
}

// ── Private serde types ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct ModelInfo {
    id: String,
}

impl LlmProvider {
    pub fn new(config: LlmConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self { config, client }
    }

    fn build_request(&self, system: &str, user: &str) -> reqwest::RequestBuilder {
        let full_system = match &self.config.system_prompt_prefix {
            Some(prefix) => format!("{}\n\n{}", prefix, system),
            None => system.to_string(),
        };

        match self.config.provider {
            ProviderType::Anthropic => {
                let url = format!("{}/messages", self.config.base_url);
                let body = AnthropicRequest {
                    model: self.config.model.clone(),
                    max_tokens: self.config.max_tokens,
                    system: full_system,
                    messages: vec![ChatMessage {
                        role: "user".to_string(),
                        content: user.to_string(),
                    }],
                };
                self.client
                    .post(&url)
                    .header("x-api-key", &self.config.api_key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&body)
            }
            _ => {
                let url = format!("{}/chat/completions", self.config.base_url);
                let body = ChatRequest {
                    model: self.config.model.clone(),
                    messages: vec![
                        ChatMessage { role: "system".to_string(), content: full_system },
                        ChatMessage { role: "user".to_string(), content: user.to_string() },
                    ],
                    temperature: self.config.temperature,
                    max_tokens: self.config.max_tokens,
                };
                self.client
                    .post(&url)
                    .bearer_auth(&self.config.api_key)
                    .json(&body)
            }
        }
    }

    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let mut last_err: Option<reqwest::Error> = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                sleep(Duration::from_millis(500)).await;
            }
            match self.build_request(system, user).send().await {
                Ok(resp) => {
                    return match self.config.provider {
                        ProviderType::Anthropic => {
                            let r: AnthropicResponse = resp
                                .json()
                                .await
                                .map_err(|e| MnemoError::Provider(e.to_string()))?;
                            r.content
                                .into_iter()
                                .find(|c| c.content_type == "text")
                                .map(|c| c.text)
                                .ok_or_else(|| {
                                    MnemoError::Provider(
                                        "no text content in Anthropic response".to_string(),
                                    )
                                })
                        }
                        _ => {
                            let r: ChatResponse = resp
                                .json()
                                .await
                                .map_err(|e| MnemoError::Provider(e.to_string()))?;
                            r.choices
                                .into_iter()
                                .next()
                                .map(|c| c.message.content)
                                .ok_or_else(|| {
                                    MnemoError::Provider("no choices in response".to_string())
                                })
                        }
                    };
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(MnemoError::Provider(last_err.unwrap().to_string()))
    }

    pub async fn health_check(&self) -> bool {
        let url = format!("{}/models", self.config.base_url);
        self.client
            .get(&url)
            .bearer_auth(&self.config.api_key)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        if self.config.provider == ProviderType::Anthropic {
            return Ok(vec![
                "claude-opus-4-8".to_string(),
                "claude-sonnet-4-6".to_string(),
                "claude-haiku-4-5-20251001".to_string(),
            ]);
        }

        let url = format!("{}/models", self.config.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.config.api_key)
            .send()
            .await
            .map_err(|e| MnemoError::Provider(e.to_string()))?;

        let models: ModelsResponse = resp
            .json()
            .await
            .map_err(|e| MnemoError::Provider(e.to_string()))?;

        Ok(models.data.into_iter().map(|m| m.id).collect())
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_type_default_urls() {
        assert_eq!(
            ProviderType::Ollama.default_base_url(),
            "http://localhost:11434/v1"
        );
        assert_eq!(
            ProviderType::OpenAi.default_base_url(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            ProviderType::Anthropic.default_base_url(),
            "https://api.anthropic.com/v1"
        );
        assert_eq!(
            ProviderType::Custom.default_base_url(),
            "http://localhost:8000/v1"
        );
    }

    #[test]
    fn test_provider_type_requires_api_key() {
        assert!(!ProviderType::Ollama.requires_api_key());
        assert!(ProviderType::OpenAi.requires_api_key());
        assert!(ProviderType::Anthropic.requires_api_key());
        assert!(!ProviderType::Custom.requires_api_key());
    }

    #[test]
    fn test_llm_config_default_is_ollama() {
        let cfg = LlmConfig::default();
        assert_eq!(cfg.provider, ProviderType::Ollama);
        assert_eq!(cfg.base_url, "http://localhost:11434/v1");
        assert_eq!(cfg.model, "llama3");
        assert_eq!(cfg.api_key, "ollama");
    }

    #[test]
    fn test_llm_config_ollama_constructor() {
        let cfg = LlmConfig::ollama("mistral");
        assert_eq!(cfg.provider, ProviderType::Ollama);
        assert_eq!(cfg.model, "mistral");
        assert_eq!(cfg.base_url, "http://localhost:11434/v1");
    }

    #[test]
    fn test_llm_config_openai_constructor() {
        let cfg = LlmConfig::openai("sk-test", "gpt-4o-mini");
        assert_eq!(cfg.provider, ProviderType::OpenAi);
        assert_eq!(cfg.api_key, "sk-test");
        assert_eq!(cfg.model, "gpt-4o-mini");
        assert_eq!(cfg.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_llm_config_validate_rejects_ollama_key_for_openai() {
        let cfg = LlmConfig::openai("ollama", "gpt-4o-mini");
        let result = cfg.validate();
        assert!(result.is_err());
        match result.unwrap_err() {
            MnemoError::Config(_) => {}
            e => panic!("expected Config error, got {:?}", e),
        }
    }

    #[test]
    fn test_llm_config_validate_accepts_ollama_config() {
        let cfg = LlmConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_llm_config_from_env() {
        std::env::set_var("MNEMO_LLM_PROVIDER", "openai");
        std::env::set_var("MNEMO_LLM_BASE_URL", "https://custom.openai.com/v1");
        std::env::set_var("MNEMO_LLM_MODEL", "gpt-4");
        std::env::set_var("MNEMO_LLM_API_KEY", "sk-test123");

        let cfg = LlmConfig::from_env();

        std::env::remove_var("MNEMO_LLM_PROVIDER");
        std::env::remove_var("MNEMO_LLM_BASE_URL");
        std::env::remove_var("MNEMO_LLM_MODEL");
        std::env::remove_var("MNEMO_LLM_API_KEY");

        assert_eq!(cfg.provider, ProviderType::OpenAi);
        assert_eq!(cfg.base_url, "https://custom.openai.com/v1");
        assert_eq!(cfg.model, "gpt-4");
        assert_eq!(cfg.api_key, "sk-test123");
    }

    #[test]
    fn test_mnemo_config_default() {
        let cfg = MnemoConfig::default();
        assert_eq!(cfg.db_path, "mnemo.db");
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.llm.provider, ProviderType::Ollama);
    }

    #[test]
    fn test_mnemo_config_from_toml_string() {
        let toml = r#"
db_path = "test.db"
port = 9090

[llm]
provider = "openai"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
api_key = "sk-real"
timeout_secs = 30
max_retries = 3
max_tokens = 2048
temperature = 0.1
"#;
        let cfg: MnemoConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.db_path, "test.db");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.llm.provider, ProviderType::OpenAi);
        assert_eq!(cfg.llm.model, "gpt-4o-mini");
        assert_eq!(cfg.llm.api_key, "sk-real");
    }

    #[test]
    fn test_mnemo_config_from_file_missing() {
        let result = MnemoConfig::from_file("nonexistent_config_file_xyz.toml");
        assert!(result.is_err());
        match result.unwrap_err() {
            MnemoError::Config(_) => {}
            e => panic!("expected Config error, got {:?}", e),
        }
    }

    #[test]
    fn test_mnemo_config_example_toml_parses() {
        let result: std::result::Result<MnemoConfig, _> =
            toml::from_str(MnemoConfig::to_example_toml());
        assert!(result.is_ok(), "example TOML failed to parse: {:?}", result);
    }
}
