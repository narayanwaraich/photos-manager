#[cfg(windows)]
use std::path::Path;

use crate::domain::VolumeKind;
#[cfg(windows)]
use crate::domain::{RootError, VolumeInfo};

/// Pure mapping from Windows GetDriveTypeW return values to VolumeKind (spec §5.2, D29e).
pub fn parse_drive_type(drive_type: u32) -> VolumeKind {
    match drive_type {
        3 /* DRIVE_FIXED */ | 6 /* DRIVE_RAMDISK */ => VolumeKind::Internal,
        2 /* DRIVE_REMOVABLE */ | 5 /* DRIVE_CDROM */ => VolumeKind::Removable,
        4 /* DRIVE_REMOTE */ => VolumeKind::Network,
        _ => VolumeKind::Unknown,
    }
}

/// Inspects a filesystem path on Windows using Win32 storage system calls (D29e).
#[cfg(windows)]
pub fn describe_volume_windows(path: &Path) -> Result<VolumeInfo, RootError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();

    let mut volume_mount_point = [0u16; 512];
    // SAFETY: GetVolumePathNameW receives valid wide character pointers and buffer lengths.
    let res = unsafe {
        GetVolumePathNameW(
            wide_path.as_ptr(),
            volume_mount_point.as_mut_ptr(),
            volume_mount_point.len() as u32,
        )
    };
    if res == 0 {
        return Err(RootError::VolumeUnavailable(format!(
            "GetVolumePathNameW failed with error {}",
            std::io::Error::last_os_error()
        )));
    }

    // SAFETY: GetDriveTypeW receives a valid null-terminated wide string pointer for the root path.
    let drive_type = unsafe { GetDriveTypeW(volume_mount_point.as_ptr()) };
    let kind = parse_drive_type(drive_type);

    let mut volume_name = [0u16; 512];
    let mut serial_number = 0u32;
    let mut fs_name = [0u16; 512];
    // SAFETY: GetVolumeInformationW receives valid pointers to output buffers and lengths.
    let vol_info_res = unsafe {
        GetVolumeInformationW(
            volume_mount_point.as_ptr(),
            volume_name.as_mut_ptr(),
            volume_name.len() as u32,
            &mut serial_number,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    };

    let (label, fs_type) = if vol_info_res != 0 {
        let name_len = volume_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(volume_name.len());
        let lbl = String::from_utf16_lossy(&volume_name[..name_len])
            .trim()
            .to_string();
        let fs_len = fs_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(fs_name.len());
        let fst = String::from_utf16_lossy(&fs_name[..fs_len])
            .trim()
            .to_string();
        (
            if lbl.is_empty() { None } else { Some(lbl) },
            if fst.is_empty() { None } else { Some(fst) },
        )
    } else {
        (None, None)
    };

    let mut guid_name = [0u16; 512];
    // SAFETY: GetVolumeNameForVolumeMountPointW receives valid pointers to root path and output buffer.
    let guid_res = unsafe {
        GetVolumeNameForVolumeMountPointW(
            volume_mount_point.as_ptr(),
            guid_name.as_mut_ptr(),
            guid_name.len() as u32,
        )
    };

    let fs_uuid = if guid_res != 0 {
        let guid_len = guid_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(guid_name.len());
        let guid_str = String::from_utf16_lossy(&guid_name[..guid_len])
            .trim()
            .to_string();
        if guid_str.is_empty() {
            if serial_number != 0 {
                Some(format!("{:08X}", serial_number))
            } else {
                None
            }
        } else {
            Some(guid_str)
        }
    } else if serial_number != 0 {
        Some(format!("{:08X}", serial_number))
    } else {
        None
    };

    Ok(VolumeInfo {
        kind,
        label,
        fs_uuid,
        fs_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_drive_type_mapping_table() {
        assert_eq!(parse_drive_type(0), VolumeKind::Unknown);
        assert_eq!(parse_drive_type(1), VolumeKind::Unknown);
        assert_eq!(parse_drive_type(2), VolumeKind::Removable);
        assert_eq!(parse_drive_type(3), VolumeKind::Internal);
        assert_eq!(parse_drive_type(4), VolumeKind::Network);
        assert_eq!(parse_drive_type(5), VolumeKind::Removable);
        assert_eq!(parse_drive_type(6), VolumeKind::Internal);
        assert_eq!(parse_drive_type(99), VolumeKind::Unknown);
    }
}
