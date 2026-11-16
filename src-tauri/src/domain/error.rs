use crate::filesystem::PathError;
use thiserror::Error;

/// Core domain and catalogue error type (spec §5.1, §12.1).
#[derive(Debug, Error)]
pub enum CatalogueError {
    #[error("Path key collision in root '{root_id}' with path_key {path_key:?}")]
    PathKeyCollision { root_id: String, path_key: Vec<u8> },

    #[error("Catalogue platform mismatch: database encoding '{found}' does not match current platform '{expected}'")]
    PlatformMismatch { expected: String, found: String },

    #[error("Entity not found: {entity} with id '{id}'")]
    NotFound { entity: &'static str, id: String },

    #[error("File size {size} bytes exceeds maximum supported 63-bit integer (i64::MAX)")]
    SizeOutOfRange { size: u64 },

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Path error: {0}")]
    Path(#[from] PathError),

    #[error("Invalid enum value: {0}")]
    InvalidEnum(String),
}

impl CatalogueError {
    /// Returns the stable error code string across boundary layers.
    pub fn code(&self) -> &'static str {
        match self {
            CatalogueError::PathKeyCollision { .. } => "path_key_collision",
            CatalogueError::PlatformMismatch { .. } => "catalogue_platform_mismatch",
            CatalogueError::NotFound { .. } => "not_found",
            CatalogueError::SizeOutOfRange { .. } => "size_out_of_range",
            CatalogueError::Database(_) => "database",
            CatalogueError::Migration(_) => "database_migration",
            CatalogueError::Path(p) => p.code(),
            CatalogueError::InvalidEnum(_) => "invalid_enum",
        }
    }
}
