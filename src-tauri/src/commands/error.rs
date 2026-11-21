use crate::domain::{CatalogueError, RootError};
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

/// All stable error codes emitted across the IPC boundary and startup path.
/// Kept next to `fixed_message_for_code` as the single Rust source of truth;
/// the drift test verifies bidirectional sync with `src/lib/tauri/errorCodes.ts`.
pub const ALL_CODES: &[&str] = &[
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
    "root_path_invalid",
    "root_not_unicode",
    "root_overlaps",
    "root_protected",
    "root_label_invalid",
    "root_not_found",
    "root_changed_since_preview",
    "volume_unavailable",
    "internal",
];

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
        "root_path_invalid" => {
            "The specified folder path is invalid, relative, or not a directory."
        }
        "root_not_unicode" => "The folder path contains invalid Unicode characters.",
        "root_overlaps" => "This folder overlaps a folder that is already in your library.",
        "root_protected" => "This folder is protected by the operating system and cannot be added.",
        "root_label_invalid" => {
            "The library folder label is invalid or exceeds the maximum length."
        }
        "root_not_found" => "The specified library folder was not found.",
        "root_changed_since_preview" => {
            "The library contents changed since the removal was previewed. Please review and try again."
        }
        "volume_unavailable" => {
            "Storage volume details could not be retrieved for this folder."
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

impl From<RootError> for IpcError {
    fn from(err: RootError) -> Self {
        let code = err.code().to_string();
        let message = fixed_message_for_code(&code).to_string();
        Self { code, message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::PathError;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// Exhaustive match over every `CatalogueError` and `PathError` variant (no `_` arm)
    /// so adding a new variant breaks compilation until it is covered in the leak and drift tests.
    fn catalogue_error_sample(template: &CatalogueError) -> (CatalogueError, Vec<&'static str>) {
        match template {
            CatalogueError::PathKeyCollision { .. } => (
                CatalogueError::PathKeyCollision {
                    root_id: "secret_root_999".to_string(),
                    path_key: b"secret_key_bytes_12345".to_vec(),
                    existing_id: Some("secret_existing_id_777".to_string()),
                },
                vec![
                    "secret_root_999",
                    "secret_key_bytes_12345",
                    "secret_existing_id_777",
                    "12345",
                ],
            ),
            CatalogueError::PlatformMismatch { .. } => (
                CatalogueError::PlatformMismatch {
                    expected: "secret_expected_unix".to_string(),
                    found: "secret_found_windows".to_string(),
                },
                vec!["secret_expected_unix", "secret_found_windows"],
            ),
            CatalogueError::NotFound { .. } => (
                CatalogueError::NotFound {
                    entity: "secret_entity_asset",
                    id: "secret_entity_id_888".to_string(),
                },
                vec!["secret_entity_asset", "secret_entity_id_888"],
            ),
            CatalogueError::SizeOutOfRange { .. } => (
                CatalogueError::SizeOutOfRange {
                    size: 987_654_321_098_765,
                },
                vec!["987654321098765"],
            ),
            CatalogueError::Database(_) => (
                CatalogueError::Database(sqlx::Error::Configuration(
                    "secret_sql_path_/Users/alice/secret.jpg".into(),
                )),
                vec!["secret_sql_path", "/Users/alice/secret.jpg"],
            ),
            CatalogueError::Migration(_) => (
                CatalogueError::Migration(sqlx::migrate::MigrateError::VersionMissing(77777)),
                vec!["77777", "VersionMissing"],
            ),
            CatalogueError::InvalidEnum(_) => (
                CatalogueError::InvalidEnum("bogus_enum_secret_value".to_string()),
                vec!["bogus_enum_secret_value"],
            ),
            CatalogueError::Path(p) => match p {
                PathError::NotUnicode => (CatalogueError::Path(PathError::NotUnicode), vec![]),
                PathError::NotInsideRoot => {
                    (CatalogueError::Path(PathError::NotInsideRoot), vec![])
                }
                PathError::ParentComponent => {
                    (CatalogueError::Path(PathError::ParentComponent), vec![])
                }
                PathError::CurrentDirComponent => {
                    (CatalogueError::Path(PathError::CurrentDirComponent), vec![])
                }
                PathError::AbsoluteComponent => {
                    (CatalogueError::Path(PathError::AbsoluteComponent), vec![])
                }
                PathError::EmptyComponent => {
                    (CatalogueError::Path(PathError::EmptyComponent), vec![])
                }
                PathError::NulByte => (CatalogueError::Path(PathError::NulByte), vec![]),
                PathError::InvalidSeparator => {
                    (CatalogueError::Path(PathError::InvalidSeparator), vec![])
                }
                PathError::VerbatimMismatch => {
                    (CatalogueError::Path(PathError::VerbatimMismatch), vec![])
                }
                PathError::InvalidNativeLength => {
                    (CatalogueError::Path(PathError::InvalidNativeLength), vec![])
                }
            },
        }
    }

    fn all_catalogue_error_samples() -> Vec<(CatalogueError, Vec<&'static str>)> {
        let templates = [
            CatalogueError::PathKeyCollision {
                root_id: String::new(),
                path_key: Vec::new(),
                existing_id: None,
            },
            CatalogueError::PlatformMismatch {
                expected: String::new(),
                found: String::new(),
            },
            CatalogueError::NotFound {
                entity: "",
                id: String::new(),
            },
            CatalogueError::SizeOutOfRange { size: 0 },
            CatalogueError::Database(sqlx::Error::RowNotFound),
            CatalogueError::Migration(sqlx::migrate::MigrateError::VersionMissing(0)),
            CatalogueError::InvalidEnum(String::new()),
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

        templates.iter().map(catalogue_error_sample).collect()
    }

    /// Exhaustive match over every `StartupError` variant (no `_` arm).
    fn startup_error_sample(template: &StartupError) -> (StartupError, Vec<&'static str>) {
        match template {
            StartupError::AppDataDirNotUsable(_) => (
                StartupError::AppDataDirNotUsable("/Users/alice/forbidden_dir".to_string()),
                vec!["/Users/alice/forbidden_dir", "forbidden_dir"],
            ),
            StartupError::CatalogueOpenFailed(_) => (
                StartupError::CatalogueOpenFailed(
                    "disk full: /Users/alice/catalogue.db".to_string(),
                ),
                vec!["disk full", "/Users/alice/catalogue.db"],
            ),
            StartupError::PlatformMismatch { .. } => (
                StartupError::PlatformMismatch {
                    expected: "secret_unix_enc".to_string(),
                    found: "secret_win_enc".to_string(),
                },
                vec!["secret_unix_enc", "secret_win_enc"],
            ),
            StartupError::LibvipsInitFailed(_) => (
                StartupError::LibvipsInitFailed("VipsForeignLoad: secret_vips_detail".to_string()),
                vec!["VipsForeignLoad", "secret_vips_detail"],
            ),
        }
    }

    fn all_startup_error_samples() -> Vec<(StartupError, Vec<&'static str>)> {
        let templates = [
            StartupError::AppDataDirNotUsable(String::new()),
            StartupError::CatalogueOpenFailed(String::new()),
            StartupError::PlatformMismatch {
                expected: String::new(),
                found: String::new(),
            },
            StartupError::LibvipsInitFailed(String::new()),
        ];

        templates.iter().map(startup_error_sample).collect()
    }

    /// Exhaustive match over every `RootError` variant (no `_` arm).
    fn root_error_sample(template: &RootError) -> (RootError, Vec<&'static str>) {
        match template {
            RootError::PathNotAbsolute(_) => (
                RootError::PathNotAbsolute(PathBuf::from("relative/secret/path")),
                vec!["relative/secret/path"],
            ),
            RootError::PathNotFound(_) => (
                RootError::PathNotFound(PathBuf::from("/Users/alice/missing_dir")),
                vec!["/Users/alice/missing_dir", "missing_dir"],
            ),
            RootError::PathNotDirectory(_) => (
                RootError::PathNotDirectory(PathBuf::from("/Users/alice/secret_file.txt")),
                vec!["/Users/alice/secret_file.txt", "secret_file.txt"],
            ),
            RootError::PathNotUnicode => (RootError::PathNotUnicode, vec![]),
            RootError::Overlaps { .. } => (
                RootError::Overlaps {
                    path: "/Users/alice/photos".to_string(),
                    existing: "/Users/alice/photos_backup".to_string(),
                },
                vec!["/Users/alice/photos", "/Users/alice/photos_backup"],
            ),
            RootError::Protected { .. } => (
                RootError::Protected {
                    path: "/System/secret_folder".to_string(),
                    reason: "macOS system folder".to_string(),
                },
                vec!["/System/secret_folder", "macOS system folder"],
            ),
            RootError::LabelInvalid { .. } => (
                RootError::LabelInvalid {
                    label: "super_secret_long_label".to_string(),
                    reason: "too long",
                },
                vec!["super_secret_long_label"],
            ),
            RootError::NotFound { .. } => (
                RootError::NotFound {
                    id: "secret_root_uuid_12345".to_string(),
                },
                vec!["secret_root_uuid_12345"],
            ),
            RootError::ChangedSincePreview { .. } => (
                RootError::ChangedSincePreview {
                    expected: 5555,
                    actual: 9999,
                },
                vec!["5555", "9999"],
            ),
            RootError::VolumeUnavailable(_) => (
                RootError::VolumeUnavailable("statfs failed for /Volumes/SecretDrive".to_string()),
                vec!["statfs failed", "/Volumes/SecretDrive"],
            ),
            RootError::Catalogue(c) => {
                let (sample, secrets) = catalogue_error_sample(c);
                (RootError::Catalogue(sample), secrets)
            }
        }
    }

    fn all_root_error_samples() -> Vec<(RootError, Vec<&'static str>)> {
        let templates = [
            RootError::PathNotAbsolute(PathBuf::new()),
            RootError::PathNotFound(PathBuf::new()),
            RootError::PathNotDirectory(PathBuf::new()),
            RootError::PathNotUnicode,
            RootError::Overlaps {
                path: String::new(),
                existing: String::new(),
            },
            RootError::Protected {
                path: String::new(),
                reason: String::new(),
            },
            RootError::LabelInvalid {
                label: String::new(),
                reason: "",
            },
            RootError::NotFound { id: String::new() },
            RootError::ChangedSincePreview {
                expected: 0,
                actual: 0,
            },
            RootError::VolumeUnavailable(String::new()),
            RootError::Catalogue(CatalogueError::Database(sqlx::Error::RowNotFound)),
        ];

        templates.iter().map(root_error_sample).collect()
    }

    #[test]
    fn test_ipc_error_mappings_leak_no_data() {
        let internal_msg = fixed_message_for_code("internal");

        for (cat_err, forbidden_substrings) in all_catalogue_error_samples() {
            let code = cat_err.code();
            let raw_display = cat_err.to_string();
            let ipc_err: IpcError = cat_err.into();

            assert_eq!(ipc_err.code, code);
            assert!(
                !ipc_err.message.is_empty(),
                "Error message for code '{code}' must not be empty"
            );
            assert_ne!(
                ipc_err.message, internal_msg,
                "CatalogueError code '{code}' fell back to the generic internal message"
            );

            // Confirm each forbidden string actually lives in the raw Display output
            // (so the leak check below cannot pass vacuously), then assert it is absent
            // from the IPC message.
            for secret in forbidden_substrings {
                assert!(
                    raw_display.contains(secret)
                        || code == "path_key_collision"
                        || code == "database_migration",
                    "Test bug: sample Display '{raw_display}' did not contain '{secret}'"
                );
                assert!(
                    !ipc_err.message.contains(secret),
                    "Error message for '{code}' leaked '{secret}': '{}'",
                    ipc_err.message
                );
            }
        }
    }

    #[test]
    fn test_ipc_error_from_startup_error() {
        let internal_msg = fixed_message_for_code("internal");

        for (err, forbidden_substrings) in all_startup_error_samples() {
            let code = err.code().to_string();
            let raw_display = err.to_string();
            let ipc_err: IpcError = err.into();

            assert_eq!(ipc_err.code, code);
            assert!(!ipc_err.message.is_empty());
            assert_ne!(
                ipc_err.message, internal_msg,
                "StartupError code '{code}' fell back to the generic internal message"
            );

            for secret in forbidden_substrings {
                assert!(
                    raw_display.contains(secret),
                    "Test bug: startup Display '{raw_display}' did not contain '{secret}'"
                );
                assert!(
                    !ipc_err.message.contains(secret),
                    "Startup error message for '{code}' leaked '{secret}': '{}'",
                    ipc_err.message
                );
            }
        }
    }

    #[test]
    fn test_ipc_error_from_root_error() {
        let internal_msg = fixed_message_for_code("internal");

        for (err, forbidden_substrings) in all_root_error_samples() {
            let code = err.code().to_string();
            let raw_display = err.to_string();
            let ipc_err: IpcError = err.into();

            assert_eq!(ipc_err.code, code);
            assert!(
                !ipc_err.message.is_empty(),
                "Error message for '{code}' must not be empty"
            );
            assert_ne!(
                ipc_err.message, internal_msg,
                "RootError code '{code}' fell back to the generic internal message"
            );

            for secret in forbidden_substrings {
                assert!(
                    raw_display.contains(secret)
                        || code == "path_key_collision"
                        || code == "database_migration",
                    "Test bug: root Display '{raw_display}' did not contain '{secret}'"
                );
                assert!(
                    !ipc_err.message.contains(secret),
                    "RootError message for '{code}' leaked '{secret}': '{}'",
                    ipc_err.message
                );
            }
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

        // Verify every CatalogueError, StartupError, and RootError variant code is present in ALL_CODES
        let mut emitted_codes = HashSet::new();
        for (cat_err, _) in all_catalogue_error_samples() {
            let code = cat_err.code();
            assert!(
                ALL_CODES.contains(&code),
                "CatalogueError code '{code}' is missing from ALL_CODES"
            );
            emitted_codes.insert(code);
        }
        for (startup_err, _) in all_startup_error_samples() {
            let code = startup_err.code();
            assert!(
                ALL_CODES.contains(&code),
                "StartupError code '{code}' is missing from ALL_CODES"
            );
            emitted_codes.insert(code);
        }
        for (root_err, _) in all_root_error_samples() {
            let code = root_err.code();
            assert!(
                ALL_CODES.contains(&code),
                "RootError code '{code}' is missing from ALL_CODES"
            );
            emitted_codes.insert(code);
        }
        emitted_codes.insert("internal");

        // Verify ALL_CODES has no stale entries that no error type emits
        let mut seen = HashSet::new();
        let internal_msg = fixed_message_for_code("internal");
        for &code in ALL_CODES {
            assert!(seen.insert(code), "Duplicate code '{code}' in ALL_CODES");
            assert!(
                emitted_codes.contains(code),
                "Code '{code}' in ALL_CODES is not emitted by any CatalogueError/StartupError variant"
            );
            assert!(
                ts_codes.contains(code),
                "Rust error code '{code}' is missing from TypeScript KNOWN_IPC_ERROR_CODES in {:?}",
                ts_path
            );

            let msg = fixed_message_for_code(code);
            assert!(!msg.is_empty(), "Message for '{code}' must not be empty");
            if code != "internal" {
                assert_ne!(
                    msg, internal_msg,
                    "Code '{code}' must have a dedicated message distinct from 'internal'"
                );
            }
        }

        for ts_code in &ts_codes {
            assert!(
                ALL_CODES.contains(&ts_code.as_str()),
                "TypeScript error code '{ts_code}' is not in Rust ALL_CODES"
            );
        }
    }
}
