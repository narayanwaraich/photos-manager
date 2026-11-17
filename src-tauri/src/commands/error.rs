use crate::domain::CatalogueError;
use crate::StartupError;
use serde::{Deserialize, Serialize};

/// Fixed, stable error payload crossing the IPC boundary (spec §12.1).
///
/// Error messages are constant strings defined per code. They never leak
/// database error strings, paths, IDs, or OS error details across the boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub code: String,
    pub message: String,
}

impl IpcError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

pub fn fixed_message_for_code(code: &str) -> &'static str {
    match code {
        "path_key_collision" => {
            "A file or folder with an equivalent comparison key already exists in this root."
        }
        "catalogue_platform_mismatch" => {
            "The photo catalogue was created on a different operating system and cannot be opened directly."
        }
        "not_found" => "The requested entity was not found in the catalogue.",
        "size_out_of_range" => "The file size exceeds the maximum supported 63-bit range.",
        "database" => "A database error occurred.",
        "database_migration" => "A database migration error occurred.",
        "path_not_unicode" => "The path contains invalid Unicode or an unpaired surrogate.",
        "path_not_inside_root" => "The path is not inside the library root.",
        "path_parent_component" => "The path contains a forbidden '..' parent component.",
        "path_current_dir_component" => "The path contains a forbidden '.' current directory component.",
        "path_absolute_component" => "The path contains a forbidden absolute or prefix component.",
        "path_empty_component" => "The path contains an empty component.",
        "path_nul_byte" => "The path contains an illegal NUL byte.",
        "path_invalid_separator" => "The path contains a path separator within a component.",
        "path_verbatim_mismatch" => "Verbatim and non-verbatim paths cannot be combined on Windows.",
        "path_invalid_native_length" => "The native path encoding has an invalid byte length.",
        "invalid_enum" => "An invalid enum value was encountered.",
        "app_data_dir_not_usable" => {
            "The application data folder cannot be accessed or created. Please verify disk permissions."
        }
        "catalogue_open_failed" => {
            "The photo catalogue database could not be opened or migrated."
        }
        "libvips_init_failed" => {
            "The image processing engine (libvips) failed to initialize."
        }
        _ => "An internal application error occurred.",
    }
}

impl From<CatalogueError> for IpcError {
    fn from(err: CatalogueError) -> Self {
        let code = err.code().to_string();
        let message = fixed_message_for_code(&code).to_string();
        Self { code, message }
    }
}

impl From<StartupError> for IpcError {
    fn from(err: StartupError) -> Self {
        let code = err.code().to_string();
        let message = err.user_message().to_string();
        Self { code, message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::PathError;
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[test]
    fn test_ipc_error_mappings_leak_no_data() {
        let secret_path = "/Users/alice/secret/vacation.jpg";
        let secret_key = b"secret_key_bytes_12345".to_vec();
        let secret_root = "root_super_secret_id";
        let secret_id = "entity_id_9999";

        let sample_errors: Vec<CatalogueError> = vec![
            CatalogueError::PathKeyCollision {
                root_id: secret_root.to_string(),
                path_key: secret_key.clone(),
                existing_id: Some(secret_id.to_string()),
            },
            CatalogueError::PlatformMismatch {
                expected: "unix-bytes".to_string(),
                found: "windows-utf16le".to_string(),
            },
            CatalogueError::NotFound {
                entity: "asset",
                id: secret_id.to_string(),
            },
            CatalogueError::SizeOutOfRange { size: u64::MAX },
            CatalogueError::Database(sqlx::Error::RowNotFound),
            CatalogueError::InvalidEnum("bogus_enum_value".to_string()),
            CatalogueError::Path(PathError::NotUnicode),
            CatalogueError::Path(PathError::NotInsideRoot),
            CatalogueError::Path(PathError::ParentComponent),
            CatalogueError::Path(PathError::CurrentDirComponent),
            CatalogueError::Path(PathError::AbsoluteComponent),
            CatalogueError::Path(PathError::EmptyComponent),
            CatalogueError::Path(PathError::NulByte),
            CatalogueError::Path(PathError::InvalidSeparator),
            CatalogueError::Path(PathError::VerbatimMismatch),
            CatalogueError::Path(PathError::InvalidNativeLength),
        ];

        for cat_err in sample_errors {
            let code = cat_err.code();
            let ipc_err: IpcError = cat_err.into();

            assert_eq!(ipc_err.code, code);
            assert!(
                !ipc_err.message.is_empty(),
                "Error message for code '{code}' must not be empty"
            );

            // Invariant: Message must never contain secret/variable data
            assert!(
                !ipc_err.message.contains(secret_path),
                "Error message for '{code}' leaked path!"
            );
            assert!(
                !ipc_err.message.contains(secret_root),
                "Error message for '{code}' leaked root_id!"
            );
            assert!(
                !ipc_err.message.contains(secret_id),
                "Error message for '{code}' leaked entity id!"
            );
            assert!(
                !ipc_err.message.contains("12345"),
                "Error message for '{code}' leaked key bytes!"
            );
        }
    }

    #[test]
    fn test_ipc_error_from_startup_error() {
        let startup_errors = vec![
            StartupError::AppDataDirNotUsable("/Users/alice/forbidden".to_string()),
            StartupError::CatalogueOpenFailed("disk full: write failed".to_string()),
            StartupError::PlatformMismatch {
                expected: "unix-bytes".to_string(),
                found: "windows-utf16le".to_string(),
            },
            StartupError::LibvipsInitFailed("VipsForeignLoad: error".to_string()),
        ];

        for err in startup_errors {
            let code = err.code().to_string();
            let ipc_err: IpcError = err.into();

            assert_eq!(ipc_err.code, code);
            assert!(!ipc_err.message.is_empty());
            assert!(!ipc_err.message.contains("/Users/alice"));
            assert!(!ipc_err.message.contains("disk full"));
            assert!(!ipc_err.message.contains("VipsForeignLoad"));
        }
    }

    #[test]
    fn test_error_codes_sync_with_typescript() {
        let ts_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("src")
            .join("lib")
            .join("tauri")
            .join("errorCodes.ts");

        assert!(
            ts_path.exists(),
            "TypeScript error codes file must exist at {:?}",
            ts_path
        );

        let content = std::fs::read_to_string(&ts_path).expect("read errorCodes.ts");

        // Parse strings inside KNOWN_IPC_ERROR_CODES = [ ... ]
        let mut ts_codes = HashSet::new();
        let in_array = content
            .split("KNOWN_IPC_ERROR_CODES = [")
            .nth(1)
            .expect("find array start")
            .split("] as const")
            .next()
            .expect("find array end");

        for line in in_array.lines() {
            let trimmed = line
                .trim()
                .trim_matches(|c| c == ',' || c == '"' || c == '\'');
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                ts_codes.insert(trimmed.to_string());
            }
        }

        // List of all stable codes emitted by CatalogueError, StartupError, and fallback
        let rust_codes = [
            "path_key_collision",
            "catalogue_platform_mismatch",
            "not_found",
            "size_out_of_range",
            "database",
            "database_migration",
            "path_not_unicode",
            "path_not_inside_root",
            "path_parent_component",
            "path_current_dir_component",
            "path_absolute_component",
            "path_empty_component",
            "path_nul_byte",
            "path_invalid_separator",
            "path_verbatim_mismatch",
            "path_invalid_native_length",
            "invalid_enum",
            "app_data_dir_not_usable",
            "catalogue_open_failed",
            "libvips_init_failed",
            "internal",
        ];

        for code in rust_codes {
            assert!(
                ts_codes.contains(code),
                "Rust error code '{code}' is missing from TypeScript KNOWN_IPC_ERROR_CODES in {:?}",
                ts_path
            );
            let msg = fixed_message_for_code(code);
            assert!(!msg.is_empty());
        }

        for ts_code in &ts_codes {
            assert!(
                rust_codes.contains(&ts_code.as_str()),
                "TypeScript error code '{ts_code}' is not recognized in Rust codes list"
            );
        }
    }
}
