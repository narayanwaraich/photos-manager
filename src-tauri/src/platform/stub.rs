use std::path::Path;

use crate::domain::{RootError, VolumeInfo, VolumeKind};

/// Fallback volume description for non-macOS/non-Windows platforms (e.g. Linux CI runner).
pub fn describe_volume_stub(_path: &Path) -> Result<VolumeInfo, RootError> {
    Ok(VolumeInfo {
        kind: VolumeKind::Unknown,
        label: None,
        fs_uuid: None,
        fs_type: None,
    })
}
