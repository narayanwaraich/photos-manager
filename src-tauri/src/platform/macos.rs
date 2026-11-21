#[cfg(target_os = "macos")]
use std::ffi::{CStr, CString};
#[cfg(target_os = "macos")]
use std::mem::MaybeUninit;
#[cfg(target_os = "macos")]
use std::path::Path;

use crate::domain::VolumeKind;
#[cfg(target_os = "macos")]
use crate::domain::{RootError, VolumeInfo};

/// Pure mapping from macOS mount flags to VolumeKind (spec §5.2, D29e).
pub fn parse_statfs_kind(flags: u32) -> VolumeKind {
    const MNT_LOCAL_FLAG: u32 = 0x00001000;
    const MNT_REMOVABLE_FLAG: u32 = 0x00000200;

    if (flags & MNT_LOCAL_FLAG) == 0 {
        VolumeKind::Network
    } else if (flags & MNT_REMOVABLE_FLAG) != 0 {
        VolumeKind::Removable
    } else {
        VolumeKind::Internal
    }
}

#[cfg(target_os = "macos")]
#[repr(C, packed(4))]
struct VolAttrBuf {
    length: u32,
    vol_name_ref: libc::attrreference_t,
    vol_uuid: [u8; 16],
    name_space: [u8; 512],
}

/// Inspects a filesystem path on macOS using native libc statfs and getattrlist system calls (D29e).
#[cfg(target_os = "macos")]
pub fn describe_volume_macos(path: &Path) -> Result<VolumeInfo, RootError> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|e| RootError::VolumeUnavailable(e.to_string()))?;

    // 1. Query statfs for filesystem type, mount flags, and mount path
    let mut stat: MaybeUninit<libc::statfs> = MaybeUninit::uninit();
    // SAFETY: CString provides a valid, null-terminated C string pointer. statfs_buf is allocated
    // as MaybeUninit<libc::statfs> and initialized upon return if statfs returns 0.
    let stat_res = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if stat_res != 0 {
        return Err(RootError::VolumeUnavailable(format!(
            "statfs failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: statfs succeeded (returned 0), so stat buffer is initialized.
    let stat = unsafe { stat.assume_init() };

    let fstype_raw = unsafe { CStr::from_ptr(stat.f_fstypename.as_ptr()) };
    let fs_type = Some(fstype_raw.to_string_lossy().to_string());
    let kind = parse_statfs_kind(stat.f_flags);

    // 2. Query getattrlist for ATTR_VOL_NAME and ATTR_VOL_UUID
    let mut attr_list: libc::attrlist = unsafe { std::mem::zeroed() };
    attr_list.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    attr_list.volattr = libc::ATTR_VOL_NAME | libc::ATTR_VOL_UUID;

    let mut attr_buf: MaybeUninit<VolAttrBuf> = MaybeUninit::uninit();
    // SAFETY: getattrlist receives a valid null-terminated path pointer, properly initialized
    // attrlist struct requesting ATTR_VOL_NAME | ATTR_VOL_UUID, and an aligned buffer sized to VolAttrBuf.
    // On success (return == 0), the kernel populates the buffer length, uuid bytes, and attribute offset.
    let attr_res = unsafe {
        libc::getattrlist(
            c_path.as_ptr(),
            &mut attr_list as *mut _ as *mut libc::c_void,
            attr_buf.as_mut_ptr() as *mut libc::c_void,
            std::mem::size_of::<VolAttrBuf>(),
            0,
        )
    };

    let (label, fs_uuid) = if attr_res == 0 {
        // SAFETY: getattrlist returned 0, buffer is initialized.
        let buf = unsafe { attr_buf.assume_init() };
        let uuid = uuid::Uuid::from_bytes(buf.vol_uuid);
        let fs_uuid_str = uuid.hyphenated().to_string();

        let name_ptr = (buf.vol_name_ref.attr_dataoffset as isize
            + &buf.vol_name_ref as *const _ as isize) as *const u8;
        let name_len = buf.vol_name_ref.attr_length as usize;
        let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
        let vol_name = String::from_utf8_lossy(name_bytes)
            .trim_matches('\0')
            .to_string();
        let label = if vol_name.is_empty() {
            None
        } else {
            Some(vol_name)
        };
        (label, Some(fs_uuid_str))
    } else {
        (None, None)
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
    fn test_parse_statfs_kind_mapping_table() {
        // MNT_LOCAL = 0x1000, MNT_REMOVABLE = 0x200
        assert_eq!(parse_statfs_kind(0x1000), VolumeKind::Internal);
        assert_eq!(parse_statfs_kind(0x1200), VolumeKind::Removable);
        assert_eq!(parse_statfs_kind(0x0000), VolumeKind::Network);
        assert_eq!(parse_statfs_kind(0x0200), VolumeKind::Network); // Not local -> network
        assert_eq!(parse_statfs_kind(0x4480d001), VolumeKind::Internal); // Typical local APFS flag
    }
}
