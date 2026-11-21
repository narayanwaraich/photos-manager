pub mod macos;
pub mod stub;
pub mod windows;

use std::path::Path;

use crate::domain::{RootError, VolumeInfo};

/// Dispatches volume description to the platform-specific implementation.
pub fn describe_volume(path: &Path) -> Result<VolumeInfo, RootError> {
    #[cfg(target_os = "macos")]
    {
        macos::describe_volume_macos(path)
    }
    #[cfg(target_os = "windows")]
    {
        windows::describe_volume_windows(path)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        stub::describe_volume_stub(path)
    }
}
