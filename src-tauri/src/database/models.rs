use crate::domain::{
    AssetId, AssetKind, AssetStatus, FileId, FileStatus, FolderId, FolderStatus, RootId,
    RootStatus, VolumeId, VolumeKind,
};

/// Row representation for `volumes` table (spec §5.2).
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeRow {
    pub id: VolumeId,
    pub kind: VolumeKind,
    pub label: Option<String>,
    pub fs_uuid: Option<String>,
    pub fs_type: Option<String>,
    pub case_sensitive: bool,
    pub last_seen_at_ms: Option<i64>,
}

/// Row representation for `library_roots` table (spec §5.2).
#[derive(Debug, Clone, PartialEq)]
pub struct RootRow {
    pub id: RootId,
    pub volume_id: Option<VolumeId>,
    pub label: String,
    pub path_display: String,
    pub path_native: Vec<u8>,
    pub status: RootStatus,
    pub follow_symlinks: bool,
    pub include_hidden: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_scan_at_ms: Option<i64>,
    pub last_scan_generation: Option<String>,
}

/// Row representation for `folders` table (spec §5.2, D27).
#[derive(Debug, Clone, PartialEq)]
pub struct FolderRow {
    pub id: FolderId,
    pub root_id: RootId,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub rel_path_display: String,
    pub rel_path_native: Vec<u8>,
    pub path_key: Vec<u8>,
    pub status: FolderStatus,
    pub modified_at_ms: Option<i64>,
    pub last_seen_generation: Option<String>,
}

/// Row representation for `files` table (spec §5.2, D27).
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    pub id: FileId,
    pub root_id: RootId,
    pub folder_id: FolderId,
    pub name: String,
    pub extension: Option<String>,
    pub rel_path_display: String,
    pub rel_path_native: Vec<u8>,
    pub path_key: Vec<u8>,
    pub size_bytes: i64,
    pub modified_at_ms: i64,
    pub created_at_ms: Option<i64>,
    pub file_identity: Option<String>,
    pub status: FileStatus,
    pub scan_generation: Option<String>,
    pub last_verified_at_ms: Option<i64>,
    pub created_in_catalog_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Row representation for `assets` table (spec §5.2, D27).
#[derive(Debug, Clone, PartialEq)]
pub struct AssetRow {
    pub id: AssetId,
    pub kind: AssetKind,
    pub primary_file_id: FileId,
    pub format: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub orientation: Option<i64>,
    pub date_taken_ms: Option<i64>,
    pub date_taken_source: Option<String>,
    pub date_taken_raw: Option<String>,
    pub tz_offset_min: Option<i64>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_model: Option<String>,
    pub iso: Option<i64>,
    pub aperture: Option<f64>,
    pub shutter_speed: Option<String>,
    pub focal_length: Option<f64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub is_favorite: bool,
    pub status: AssetStatus,
    pub created_in_catalog_at_ms: i64,
    pub updated_at_ms: i64,
}
