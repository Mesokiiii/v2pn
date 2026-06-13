use thiserror::Error;

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),

    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("utf-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("subscription parse: {0}")]
    Parse(String),

    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),

    #[error("invalid config: {0}")]
    InvalidConfig(String),

    #[error("storage: {0}")]
    Storage(String),

    #[error("other: {0}")]
    Other(String),
}

impl From<anyhow::Error> for CoreError {
    fn from(value: anyhow::Error) -> Self {
        CoreError::Other(value.to_string())
    }
}
