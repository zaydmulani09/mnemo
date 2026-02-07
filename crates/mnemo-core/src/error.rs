use thiserror::Error;

#[derive(Debug, Error)]
pub enum MnemoError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("LLM provider error: {0}")]
    Provider(String),

    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("Extraction error: {0}")]
    Extraction(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MnemoError>;
