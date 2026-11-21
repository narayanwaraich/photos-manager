use std::path::Path;

use crate::domain::{RootError, VolumeInfo};

/// Describes the storage volume containing the given filesystem path (spec §5.2, D29e).
pub fn describe_volume(path: &Path) -> Result<VolumeInfo, RootError> {
    crate::platform::describe_volume(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn volume_report() {
        let env_path = std::env::var("PM_VOLUME_PATH").unwrap_or_else(|_| "/".to_string());
        let path = Path::new(&env_path);
        println!(
            "=== PM_VOLUME_PATH Volume Report for '{}' ===",
            path.display()
        );
        match describe_volume(path) {
            Ok(vol) => {
                println!("Volume kind: {:?}", vol.kind);
                println!("Volume label: {:?}", vol.label);
                println!("Volume fs_uuid: {:?}", vol.fs_uuid);
                println!("Volume fs_type: {:?}", vol.fs_type);
            }
            Err(e) => {
                println!("describe_volume error: {:?}", e);
            }
        }
        let probe = crate::library::case_probe::probe_case_sensitivity(
            path,
            &crate::library::case_probe::RealFileIdentityLookup,
        );
        println!("Case probe: {:?}", probe);
        println!("==================================================");
    }

    #[test]
    fn test_describe_volume_on_current_system() {
        let temp = tempfile::tempdir().expect("tempdir");
        let vol = describe_volume(temp.path()).expect("describe_volume must succeed on temp dir");
        #[cfg(target_os = "macos")]
        {
            assert_eq!(vol.kind, crate::domain::VolumeKind::Internal);
            assert!(vol.fs_type.is_some());
            assert!(vol.fs_uuid.is_some());
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(vol.kind, crate::domain::VolumeKind::Internal);
            assert!(vol.fs_type.is_some());
        }
    }
}
