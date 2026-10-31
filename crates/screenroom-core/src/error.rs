pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{message}")]
    Library { code: &'static str, message: String },

    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),

    #[error("stored data is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl CoreError {
    pub fn library(code: &'static str, message: impl Into<String>) -> Self {
        Self::Library {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Library { code, .. } => code,
            Self::Sqlite(_) => "database_error",
            Self::Io(_) => "io_error",
            Self::Json(_) => "invalid_stored_json",
        }
    }

    pub fn is_availability(&self) -> bool {
        matches!(
            self.code(),
            "source_missing" | "source_disconnected" | "permission_lost" | "io_error"
        )
    }
}
