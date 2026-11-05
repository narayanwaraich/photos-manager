pub mod vips;

use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImageProcessorError {
    #[error("Image processing error: {0}")]
    Processing(String),
    #[error("Input file not found: {0}")]
    FileNotFound(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailResult {
    pub width: u32,
    pub height: u32,
}

/// ImageProcessor trait (spec §3.3) for thumbnails, resize, and orientation correction
pub trait ImageProcessor: Send + Sync {
    /// Reads input_path, applies EXIF orientation upright, and writes a WebP thumbnail
    /// to output_path where the longest edge does not exceed max_edge.
    fn generate_thumbnail(
        &self,
        input_path: &Path,
        output_path: &Path,
        max_edge: u32,
    ) -> Result<ThumbnailResult, ImageProcessorError>;

    /// Queries whether a particular loader operation (e.g. "jpegload", "pngload", "heifload") is supported.
    fn is_loader_supported(&self, loader: &str) -> bool;

    /// Returns list of supported standard image loader codecs at runtime.
    fn query_supported_loaders(&self) -> Vec<String>;
}
