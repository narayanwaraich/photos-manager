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
                Some(crate::platform::format_serial_identity(serial_number))
            } else {
                None
            }
        } else {
            Some(guid_str)
        }
    } else if serial_number != 0 {
        Some(crate::platform::format_serial_identity(serial_number))
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

/// Retrieves the hardware file identity on Windows (volume serial number + 128-bit file ID) (spec §5.3, D27, TASK-1D decision g).
///
/// Returns `None` on any failure, as file identity is best-effort and must never abort a scan.
#[cfg(windows)]
pub fn file_identity_windows(path: &Path) -> Option<String> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };

    // Open file handle with read-only access and share-all (read, write, delete) permissions.
    // FILE_SHARE_READ (1) | FILE_SHARE_WRITE (2) | FILE_SHARE_DELETE (4) = 7
    let file = OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(path)
        .ok()?;

    let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    // SAFETY: FILE_ID_INFO contains only plain integer types (u64 and [u8; 16]), valid when zeroed.
    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };

    // SAFETY: GetFileInformationByHandleEx is passed a valid file handle and a correctly
    // sized pointer to FILE_ID_INFO matching the FileIdInfo class parameter.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            &mut info as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };

    if ok == 0 {
        return None;
    }

    let file_id_u128 = u128::from_be_bytes(info.FileId.Identifier);
    Some(format!("{:x}:{:x}", info.VolumeSerialNumber, file_id_u128))
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

    #[cfg(windows)]
    #[test]
    fn test_windows_file_identity_hardlink_and_distinct_files() {
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let file1 = dir.path().join("file1.txt");
        let link1 = dir.path().join("link1.txt");
        let file2 = dir.path().join("file2.txt");

        std::fs::write(&file1, b"hello world").expect("write file1");
        std::fs::hard_link(&file1, &link1).expect("create hard link");
        std::fs::write(&file2, b"distinct file").expect("write file2");

        let id_file1 = file_identity_windows(&file1);
        let id_link1 = file_identity_windows(&link1);
        let id_file2 = file_identity_windows(&file2);

        assert!(id_file1.is_some(), "file1 identity should be extracted");
        assert_eq!(
            id_file1, id_link1,
            "Two paths to one hard link must yield identical file identity"
        );
        assert_ne!(
            id_file1, id_file2,
            "Distinct files must yield distinct file identities"
        );
    }
}
