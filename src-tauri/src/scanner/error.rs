use crate::domain::CatalogueError;
use thiserror::Error;

/// Scanner error enum mapping to stable codes across IPC (spec §12.1, TASK-1D decision k).
#[derive(Debug, Error)]
pub enum ScanError {
    #[error("Scan already running for root '{0}'")]
    AlreadyRunning(String),

    #[error("Root '{path}' is unreachable or not a directory")]
    RootUnreachable { path: String },

    #[error("Scan option is unsupported: {reason}")]
    OptionUnsupported { reason: String },

    #[error("Database error: {0}")]
    Catalogue(#[from] CatalogueError),
}

impl ScanError {
    /// Returns the stable error code string.
    pub fn code(&self) -> &'static str {
        match self {
            ScanError::AlreadyRunning(_) => "scan_already_running",
            ScanError::RootUnreachable { .. } => "scan_root_unreachable",
            ScanError::OptionUnsupported { .. } => "scan_root_option_unsupported",
            ScanError::Catalogue(c) => c.code(),
        }
    }
}

impl From<sqlx::Error> for ScanError {
    fn from(err: sqlx::Error) -> Self {
        ScanError::Catalogue(CatalogueError::Database(err))
    }
}
