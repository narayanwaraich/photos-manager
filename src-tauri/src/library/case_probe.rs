use std::path::Path;

use crate::domain::CaseProbe;

/// Unique filesystem identity for a file or directory (dev+ino on Unix, serial+index on Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub dev_or_serial: u64,
    pub ino_or_index: u64,
}

/// Trait abstracting OS file identity lookup to allow injection in unit tests (D29d).
pub trait FileIdentityLookup {
    fn get_identity(&self, path: &Path) -> Result<Option<FileIdentity>, std::io::Error>;
}

/// Real OS filesystem identity lookup using standard library metadata (read-only).
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFileIdentityLookup;

impl FileIdentityLookup for RealFileIdentityLookup {
    fn get_identity(&self, path: &Path) -> Result<Option<FileIdentity>, std::io::Error> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            match std::fs::metadata(path) {
                Ok(meta) => Ok(Some(FileIdentity {
                    dev_or_serial: meta.dev(),
                    ino_or_index: meta.ino(),
                })),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
            use windows_sys::Win32::Storage::FileSystem::{
                CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
                FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
                OPEN_EXISTING,
            };

            let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: CreateFileW receives a valid null-terminated wide string.
            // Opening with 0 desired access and FILE_FLAG_BACKUP_SEMANTICS allows querying metadata on files and directories.
            let handle = unsafe {
                CreateFileW(
                    wide_path.as_ptr(),
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS,
                    std::ptr::null_mut(),
                )
            };

            if handle == INVALID_HANDLE_VALUE {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                return Err(err);
            }

            let mut info: std::mem::MaybeUninit<BY_HANDLE_FILE_INFORMATION> =
                std::mem::MaybeUninit::uninit();
            // SAFETY: handle is valid and info is properly allocated to hold BY_HANDLE_FILE_INFORMATION.
            let ok = unsafe { GetFileInformationByHandle(handle, info.as_mut_ptr()) };
            // SAFETY: handle was returned by CreateFileW and is closed here.
            unsafe { CloseHandle(handle) };

            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }

            // SAFETY: GetFileInformationByHandle succeeded (ok != 0), so info is initialized.
            let info = unsafe { info.assume_init() };
            let serial = info.dwVolumeSerialNumber as u64;
            let index = ((info.nFileIndexHigh as u64) << 32) | (info.nFileIndexLow as u64);
            Ok(Some(FileIdentity {
                dev_or_serial: serial,
                ino_or_index: index,
            }))
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Ok(None)
        }
    }
}

/// Returns true if the string contains at least one cased character.
pub fn has_cased_char(s: &str) -> bool {
    s.chars().any(|c| c.is_lowercase() || c.is_uppercase())
}

/// Swaps the casing of all cased characters in a string (e.g. `Photos` -> `pHOTOS`).
pub fn swap_case(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_lowercase() {
                c.to_uppercase().collect::<String>()
            } else if c.is_uppercase() {
                c.to_lowercase().collect::<String>()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Probes volume case sensitivity completely read-only (decision d, D27c, D29d).
///
/// Method:
/// 1. Walks the canonical path upwards from the candidate root to find the first component
///    with a cased character.
/// 2. If no ancestor has a cased name (e.g. drive roots like `C:\` or `D:\`), takes the first
///    entry inside the path with a cased character (read-only `read_dir`, at most 64 entries).
/// 3. Builds a candidate path with that component's case swapped.
/// 4. Queries the injected file identity lookup for both paths.
///
/// Outcomes:
/// - Same identity: the volume folds case (`CaseProbe::CaseInsensitive`).
/// - Swapped name is absent or has a different identity: does not fold (`CaseProbe::CaseSensitive`).
/// - No cased component found, or lookup fails unexpectedly: `CaseProbe::Unknown`.
pub fn probe_case_sensitivity(path: &Path, lookup: &impl FileIdentityLookup) -> CaseProbe {
    for ancestor in path.ancestors() {
        if let Some(file_name) = ancestor.file_name() {
            if let Some(name_str) = file_name.to_str() {
                if has_cased_char(name_str) {
                    return test_cased_target(ancestor, name_str, lookup);
                }
            }
        }
    }

    // When no ancestor has a cased name (e.g. drive roots like `C:\` or `D:\`),
    // inspect the first cased entry directly inside the root (amended D29d).
    if let Some((child_path, child_name)) = find_cased_child_entry(path) {
        return test_cased_target(&child_path, &child_name, lookup);
    }

    CaseProbe::Unknown
}

/// Finds the first entry directly inside `dir` (bounded to at most 64 entries)
/// whose file name contains at least one cased character (spec §3.5, amended D29d).
///
/// Read-only directory listing. Returns `None` if listing fails, the directory is empty,
/// or no cased entry is encountered within the first 64 entries.
pub fn find_cased_child_entry(dir: &Path) -> Option<(std::path::PathBuf, String)> {
    let read_dir = std::fs::read_dir(dir).ok()?;
    for entry in read_dir.take(64) {
        if let Ok(entry) = entry {
            let file_name = entry.file_name();
            if let Some(name_str) = file_name.to_str() {
                if has_cased_char(name_str) {
                    return Some((entry.path(), name_str.to_string()));
                }
            }
        }
    }
    None
}

fn test_cased_target(target: &Path, name_str: &str, lookup: &impl FileIdentityLookup) -> CaseProbe {
    let swapped_name = swap_case(name_str);
    let swapped_path = target.with_file_name(swapped_name);

    let orig_id = match lookup.get_identity(target) {
        Ok(Some(id)) => id,
        _ => return CaseProbe::Unknown,
    };

    match lookup.get_identity(&swapped_path) {
        Ok(Some(swapped_id)) if swapped_id == orig_id => CaseProbe::CaseInsensitive,
        Ok(None) => CaseProbe::CaseSensitive,
        Ok(Some(_different_id)) => CaseProbe::CaseSensitive,
        Err(_) => CaseProbe::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::tempdir;

    struct FakeLookup {
        entries: HashMap<PathBuf, Option<FileIdentity>>,
        should_fail: bool,
    }

    impl FakeLookup {
        fn new() -> Self {
            Self {
                entries: HashMap::new(),
                should_fail: false,
            }
        }

        fn with_identity(mut self, path: &str, identity: FileIdentity) -> Self {
            self.entries.insert(PathBuf::from(path), Some(identity));
            self
        }

        fn with_missing(mut self, path: &str) -> Self {
            self.entries.insert(PathBuf::from(path), None);
            self
        }

        fn with_failure(mut self) -> Self {
            self.should_fail = true;
            self
        }
    }

    impl FileIdentityLookup for FakeLookup {
        fn get_identity(&self, path: &Path) -> Result<Option<FileIdentity>, std::io::Error> {
            if self.should_fail {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "simulated lookup error",
                ));
            }
            Ok(self.entries.get(path).copied().unwrap_or(None))
        }
    }

    #[test]
    fn test_case_probe_folds_case() {
        let lookup = FakeLookup::new()
            .with_identity(
                "/a/Photos",
                FileIdentity {
                    dev_or_serial: 1,
                    ino_or_index: 100,
                },
            )
            .with_identity(
                "/a/pHOTOS",
                FileIdentity {
                    dev_or_serial: 1,
                    ino_or_index: 100,
                },
            );

        let probe = probe_case_sensitivity(Path::new("/a/Photos"), &lookup);
        assert_eq!(probe, CaseProbe::CaseInsensitive);
    }

    #[test]
    fn test_case_probe_does_not_fold_case_swapped_is_absent() {
        let lookup = FakeLookup::new()
            .with_identity(
                "/a/Photos",
                FileIdentity {
                    dev_or_serial: 1,
                    ino_or_index: 100,
                },
            )
            .with_missing("/a/pHOTOS");

        let probe = probe_case_sensitivity(Path::new("/a/Photos"), &lookup);
        assert_eq!(probe, CaseProbe::CaseSensitive);
    }

    #[test]
    fn test_case_probe_does_not_fold_case_swapped_has_different_identity() {
        let lookup = FakeLookup::new()
            .with_identity(
                "/a/Photos",
                FileIdentity {
                    dev_or_serial: 1,
                    ino_or_index: 100,
                },
            )
            .with_identity(
                "/a/pHOTOS",
                FileIdentity {
                    dev_or_serial: 1,
                    ino_or_index: 200,
                },
            );

        let probe = probe_case_sensitivity(Path::new("/a/Photos"), &lookup);
        assert_eq!(probe, CaseProbe::CaseSensitive);
    }

    #[test]
    fn test_case_probe_no_cased_component_returns_unknown() {
        let lookup = FakeLookup::new().with_identity(
            "/123/456",
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 100,
            },
        );

        let probe = probe_case_sensitivity(Path::new("/123/456"), &lookup);
        assert_eq!(probe, CaseProbe::Unknown);
    }

    #[test]
    fn test_case_probe_lookup_failure_returns_unknown() {
        let lookup = FakeLookup::new().with_failure();
        let probe = probe_case_sensitivity(Path::new("/a/Photos"), &lookup);
        assert_eq!(probe, CaseProbe::Unknown);
    }

    #[test]
    fn test_case_probe_real_filesystem_read_only_and_correctness() {
        let temp = tempdir().expect("tempdir");
        let test_dir = temp.path().join("ProbeFolder");
        std::fs::create_dir_all(&test_dir).expect("create dir");

        // 1. Snapshot directory state before probe
        let read_snapshot = |dir: &Path| -> Vec<(String, u64, std::time::SystemTime)> {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(dir).expect("read_dir") {
                let entry = entry.expect("dir entry");
                let meta = entry.metadata().expect("metadata");
                let mtime = meta.modified().expect("mtime");
                entries.push((
                    entry.file_name().to_string_lossy().to_string(),
                    meta.len(),
                    mtime,
                ));
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            entries
        };

        let before_snapshot = read_snapshot(temp.path());
        let before_dir_mtime = std::fs::metadata(&test_dir)
            .expect("meta")
            .modified()
            .expect("mtime");

        // 2. Execute read-only probe
        let probe = probe_case_sensitivity(&test_dir, &RealFileIdentityLookup);

        // 3. Snapshot directory state after probe
        let after_snapshot = read_snapshot(temp.path());
        let after_dir_mtime = std::fs::metadata(&test_dir)
            .expect("meta")
            .modified()
            .expect("mtime");

        // 4. Assert complete read-only invariance (zero files created or modified)
        assert_eq!(
            before_snapshot, after_snapshot,
            "Directory contents must be identical"
        );
        assert_eq!(
            before_dir_mtime, after_dir_mtime,
            "Directory modification time must not change"
        );

        // 5. On macOS default APFS and Windows default NTFS, runners fold case
        println!("Real filesystem case probe result: {:?}", probe);
        assert_eq!(
            probe,
            CaseProbe::CaseInsensitive,
            "Default macOS/Windows volume folds case"
        );
    }

    #[test]
    fn test_find_cased_child_entry_on_temp_directory() {
        let temp = tempdir().expect("tempdir");

        // Empty directory returns None
        assert_eq!(find_cased_child_entry(temp.path()), None);

        // Directory with non-cased file returns None
        let uncased_file = temp.path().join("12345");
        std::fs::write(&uncased_file, b"content").expect("write file");
        assert_eq!(find_cased_child_entry(temp.path()), None);

        // Directory with cased file returns Some((path, name))
        let cased_file = temp.path().join("TestFile.jpg");
        std::fs::write(&cased_file, b"photo").expect("write cased");
        let found = find_cased_child_entry(temp.path());
        assert!(found.is_some(), "Must find cased child entry");
        let (found_path, found_name) = found.unwrap();
        assert_eq!(found_name, "TestFile.jpg");
        assert_eq!(found_path, cased_file);
    }

    #[cfg(windows)]
    #[test]
    fn test_case_probe_c_drive_on_windows_runner() {
        let lookup = RealFileIdentityLookup;
        let probe = probe_case_sensitivity(Path::new(r"C:\"), &lookup);
        assert_eq!(
            probe,
            CaseProbe::CaseInsensitive,
            "C:\\ on Windows runner must probe as CaseInsensitive via child entry fallback"
        );
    }
}
