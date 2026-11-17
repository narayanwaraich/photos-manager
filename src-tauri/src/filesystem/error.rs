use thiserror::Error;

/// Errors arising from path validation, relative path derivation,
/// and native byte conversions (spec §5.1, §6.4, D26).
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum PathError {
    #[error("Path is not valid Unicode: not valid UTF-8 or contains unpaired surrogate")]
    NotUnicode,

    #[error("Path is not inside library root")]
    NotInsideRoot,

    #[error("Path contains forbidden '..' (parent directory) component")]
    ParentComponent,

    #[error("Path contains forbidden '.' (current directory) component")]
    CurrentDirComponent,

    #[error("Path contains forbidden absolute or prefix component")]
    AbsoluteComponent,

    #[error("Path contains empty component")]
    EmptyComponent,

    #[error("Path contains NUL byte")]
    NulByte,

    #[error("Path contains path separator within a single component")]
    InvalidSeparator,

    #[error("Windows verbatim (\\\\?\\) prefix mismatch between root and target")]
    VerbatimMismatch,

    #[error("Invalid native path byte length (odd byte length for UTF-16LE)")]
    InvalidNativeLength,
}

impl PathError {
    /// Returns the stable error code string across boundaries.
    pub fn code(&self) -> &'static str {
        match self {
            PathError::NotUnicode => "path_not_unicode",
            PathError::NotInsideRoot => "path_not_inside_root",
            PathError::ParentComponent => "path_parent_component",
            PathError::CurrentDirComponent => "path_current_dir_component",
            PathError::AbsoluteComponent => "path_absolute_component",
            PathError::EmptyComponent => "path_empty_component",
            PathError::NulByte => "path_nul_byte",
            PathError::InvalidSeparator => "path_invalid_separator",
            PathError::VerbatimMismatch => "path_verbatim_mismatch",
            PathError::InvalidNativeLength => "path_invalid_native_length",
        }
    }
}
