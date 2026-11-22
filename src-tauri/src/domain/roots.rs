use std::path::PathBuf;
use thiserror::Error;

use crate::domain::{CatalogueError, RootId, RootStatus, VolumeId, VolumeKind};

/// Warning conditions returned when adding a library root (spec §5.2, §6.1, D29).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootWarning {
    CaseSensitivityUnknown,
    NetworkVolume,
}

impl RootWarning {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CaseSensitivityUnknown => "case_sensitivity_unknown",
            Self::NetworkVolume => "network_volume",
        }
    }
}

/// Outcome of successfully adding a library root.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRootOutcome {
    pub root_id: RootId,
    pub volume_id: VolumeId,
    pub warnings: Vec<RootWarning>,
}

/// Summary of a registered library root for display in lists (spec §12.1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootSummary {
    pub id: RootId,
    pub label: String,
    pub path_display: String,
    pub volume_kind: VolumeKind,
    pub volume_label: Option<String>,
    pub volume_case_sensitive: bool,
    pub status: RootStatus,
    pub reachable: bool,
    pub file_count: i64,
    pub folder_count: i64,
}

/// Preview of a library root removal (spec §5.2, D29).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveRootPreview {
    pub root_id: RootId,
    pub file_count: i64,
    pub folder_count: i64,
    pub asset_count: i64,
}

/// Information describing a storage volume (spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    pub kind: VolumeKind,
    pub label: Option<String>,
    pub fs_uuid: Option<String>,
    pub fs_type: Option<String>,
}

/// Outcome of the read-only case sensitivity probe (spec §5.1, D27c, D29d).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseProbe {
    CaseInsensitive,
    CaseSensitive,
    Unknown,
}

/// Domain error type for library root operations (spec §12.1).
#[derive(Debug, Error)]
pub enum RootError {
    #[error("Folder path is not absolute: {0}")]
    PathNotAbsolute(PathBuf),

    #[error("Folder path does not exist: {0}")]
    PathNotFound(PathBuf),

    #[error("Path is not a directory: {0}")]
    PathNotDirectory(PathBuf),

    #[error("Path is not valid Unicode")]
    PathNotUnicode,

    #[error("Path overlaps existing root '{existing}': {path}")]
    Overlaps { path: String, existing: String },

    #[error("Path is a protected system folder: {path} ({reason})")]
    Protected { path: String, reason: String },

    #[error("Root label is invalid: {label} ({reason})")]
    LabelInvalid { label: String, reason: &'static str },

    #[error("Root not found: {id}")]
    NotFound { id: String },

    #[error("Root contents changed since preview: expected {expected} files, found {actual}")]
    ChangedSincePreview { expected: i64, actual: i64 },

    #[error("Volume unavailable: {0}")]
    VolumeUnavailable(String),

    #[error("Catalogue error: {0}")]
    Catalogue(#[from] CatalogueError),
}

impl RootError {
    /// Returns the stable error code string across boundary layers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::PathNotAbsolute(_) | Self::PathNotFound(_) | Self::PathNotDirectory(_) => {
                "root_path_invalid"
            }
            Self::PathNotUnicode => "root_not_unicode",
            Self::Overlaps { .. } => "root_overlaps",
            Self::Protected { .. } => "root_protected",
            Self::LabelInvalid { .. } => "root_label_invalid",
            Self::NotFound { .. } => "root_not_found",
            Self::ChangedSincePreview { .. } => "root_changed_since_preview",
            Self::VolumeUnavailable(_) => "volume_unavailable",
            Self::Catalogue(err) => err.code(),
        }
    }
}
