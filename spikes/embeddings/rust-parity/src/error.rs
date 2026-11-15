use thiserror::Error;

/// Errors returned by the EmbeddingGemma 2 parity library.
#[derive(Debug, Error)]
pub enum ParityError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Tokenizer error: {0}")]
    Tokenizer(String),

    #[error("Image decode/preprocess error: {0}")]
    Image(String),

    #[error("ONNX Runtime dynamic library load error: {0}")]
    LoadDynamic(#[from] ort::LoadDynamicError),

    #[error("ONNX Runtime error: {0}")]
    Ort(#[from] ort::Error),

    #[error("Invalid configuration or input: {0}")]
    InvalidInput(String),

    #[error("Parity gate failed: {0}")]
    ParityGateFailed(String),
}

pub type Result<T> = std::result::Result<T, ParityError>;
