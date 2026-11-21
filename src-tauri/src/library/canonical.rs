use std::path::{Component, Path, PathBuf};

use crate::domain::RootError;

/// Strips Windows verbatim prefix (`\\?\` or `\\?\UNC\`) for user display strings only (decision a, D29).
pub fn strip_verbatim_prefix(path_str: &str) -> String {
    if let Some(rest) = path_str.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = path_str.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path_str.to_string()
    }
}

/// Represents a validated, canonicalized library root path (spec §3.5, D29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalRoot {
    pub canonical_path: PathBuf,
    pub path_native: Vec<u8>,
    pub path_display: String,
    pub components: Vec<String>,
}

impl CanonicalRoot {
    /// Returns the last normal path component, or an empty string if none.
    pub fn last_component(&self) -> &str {
        self.components.last().map(|s| s.as_str()).unwrap_or("")
    }
}

/// Extracts component strings from a canonical path.
///
/// On Windows, includes the drive/prefix component (e.g. `C:`) so cross-volume roots
/// are never conflated.
pub fn extract_components(path: &Path) -> Vec<String> {
    let mut comps = Vec::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => {
                let s = p.as_os_str().to_string_lossy();
                comps.push(strip_verbatim_prefix(&s));
            }
            Component::RootDir => {
                // RootDir "/" or "\" is structural; components list holds segments
            }
            Component::Normal(s) => {
                comps.push(s.to_string_lossy().to_string());
            }
            Component::CurDir | Component::ParentDir => {
                // Canonical paths do not contain . or ..
            }
        }
    }
    comps
}

/// Canonicalizes and validates a candidate library root path (decision a, D29).
///
/// Ensures:
/// 1. Path is absolute.
/// 2. Path exists and resolves symlinks.
/// 3. Path references a directory.
/// 4. Path is valid Unicode according to D26 rules.
pub fn canonicalize_root(path: &Path) -> Result<CanonicalRoot, RootError> {
    if !path.is_absolute() {
        return Err(RootError::PathNotAbsolute(path.to_path_buf()));
    }

    let canonical = std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => RootError::PathNotFound(path.to_path_buf()),
        _ => RootError::PathNotFound(path.to_path_buf()),
    })?;

    if !canonical.is_dir() {
        return Err(RootError::PathNotDirectory(canonical));
    }

    // Validate Unicode and D26 skip rules
    if canonical.to_str().is_none() {
        return Err(RootError::PathNotUnicode);
    }

    #[cfg(unix)]
    for comp in canonical.components() {
        if let Component::Normal(os_name) = comp {
            let s = os_name.to_str().ok_or(RootError::PathNotUnicode)?;
            if s.contains('\\') || s.ends_with(':') {
                return Err(RootError::PathNotUnicode);
            }
        }
    }

    let raw_display = canonical.to_string_lossy().to_string();
    let path_display = strip_verbatim_prefix(&raw_display);
    let path_native = crate::filesystem::native_to_blob(&canonical);
    let components = extract_components(&canonical);

    Ok(CanonicalRoot {
        canonical_path: canonical,
        path_native,
        path_display,
        components,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_strip_verbatim_prefix() {
        assert_eq!(strip_verbatim_prefix(r"\\?\C:\Photos"), r"C:\Photos");
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\Photos"),
            r"\\server\share\Photos"
        );
        assert_eq!(
            strip_verbatim_prefix("/Users/alice/Photos"),
            "/Users/alice/Photos"
        );
        assert_eq!(strip_verbatim_prefix(r"C:\Photos"), r"C:\Photos");
    }

    #[test]
    fn test_canonicalize_root_relative_path_rejected() {
        let res = canonicalize_root(Path::new("relative/photos"));
        match res {
            Err(RootError::PathNotAbsolute(p)) => {
                assert_eq!(p, PathBuf::from("relative/photos"));
            }
            other => panic!("Expected PathNotAbsolute, got {other:?}"),
        }
    }

    #[test]
    fn test_canonicalize_root_missing_path_rejected() {
        let temp = tempdir().expect("tempdir");
        let missing = temp.path().join("does_not_exist");
        let res = canonicalize_root(&missing);
        match res {
            Err(RootError::PathNotFound(p)) => {
                assert_eq!(p, missing);
            }
            other => panic!("Expected PathNotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_canonicalize_root_file_instead_of_directory_rejected() {
        let temp = tempdir().expect("tempdir");
        let file_path = temp.path().join("a_file.txt");
        std::fs::write(&file_path, b"content").expect("write file");

        let res = canonicalize_root(&file_path);
        match res {
            Err(RootError::PathNotDirectory(p)) => {
                assert_eq!(p, std::fs::canonicalize(&file_path).unwrap());
            }
            other => panic!("Expected PathNotDirectory, got {other:?}"),
        }
    }

    #[test]
    fn test_canonicalize_root_directory_success() {
        let temp = tempdir().expect("tempdir");
        let dir = temp.path().join("my_photos");
        std::fs::create_dir_all(&dir).expect("create dir");

        let root = canonicalize_root(&dir).expect("canonicalize should succeed");
        assert_eq!(root.canonical_path, std::fs::canonicalize(&dir).unwrap());
        assert_eq!(root.last_component(), "my_photos");
        assert!(!root.path_display.is_empty());
        assert!(!root.path_native.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_canonicalize_root_symlink_resolves_to_target() {
        let temp = tempdir().expect("tempdir");
        let target_dir = temp.path().join("target_photos");
        std::fs::create_dir_all(&target_dir).expect("create target dir");

        let symlink_path = temp.path().join("symlink_photos");
        std::os::unix::fs::symlink(&target_dir, &symlink_path).expect("create symlink");

        let root = canonicalize_root(&symlink_path).expect("canonicalize symlink");
        let expected_canon = std::fs::canonicalize(&target_dir).unwrap();
        assert_eq!(root.canonical_path, expected_canon);
        assert_eq!(root.last_component(), "target_photos");
    }

    #[cfg(unix)]
    #[test]
    fn test_canonicalize_root_non_unicode_name_rejected() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temp = tempdir().expect("tempdir");
        let mut non_unicode = temp.path().as_os_str().to_os_string().into_vec();
        non_unicode.push(b'/');
        non_unicode.extend_from_slice(&[0xff, 0xfe, 0xfd]); // Invalid UTF-8 sequence
        let bad_path = PathBuf::from(OsString::from_vec(non_unicode));

        // Attempting canonicalize on non-unicode path: either rejected during validation or not found
        let res = canonicalize_root(&bad_path);
        assert!(res.is_err());
    }
}
