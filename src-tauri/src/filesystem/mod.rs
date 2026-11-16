pub mod error;
pub mod native_path;
pub mod policy;
pub mod rel_path;

pub use error::PathError;
pub use native_path::NativePath;
pub use policy::PathKeyPolicy;
pub use rel_path::RelPath;

use std::path::{Path, PathBuf};

/// Converts a native Path to BLOB bytes according to spec §5.1.
pub fn native_to_blob(path: &Path) -> Vec<u8> {
    NativePath::from_os_str(path.as_os_str()).into_bytes()
}

/// Restores a native PathBuf from BLOB bytes without loss.
pub fn blob_to_native(blob: &[u8]) -> PathBuf {
    NativePath::from_bytes(blob.to_vec())
        .to_path_buf()
        .unwrap_or_else(|_| PathBuf::from(String::from_utf8_lossy(blob).as_ref()))
}

/// Produces a human-readable display string for UI purposes.
pub fn native_to_display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// Derives a normalized comparison key (path_key) from a relative path
/// to avoid phantom duplicates across case-sensitivity and Unicode normalization forms.
pub fn derive_path_key(path: &Path, case_sensitive: bool) -> Vec<u8> {
    let mut comps = Vec::new();
    for comp in path.components() {
        if let std::path::Component::Normal(s) = comp {
            comps.push(s.to_string_lossy().to_string());
        }
    }
    let rel = RelPath::from_components(comps).unwrap_or_else(|_| RelPath::empty());
    rel.path_key(PathKeyPolicy { case_sensitive })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::ffi::OsString;

    // G.1: Native round-trip property test
    #[cfg(unix)]
    proptest! {
        #[test]
        fn test_proptest_native_roundtrip_unix(raw_bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            use std::os::unix::ffi::OsStringExt;
            let os_str = OsString::from_vec(raw_bytes.clone());
            let native = NativePath::from_os_str(&os_str);
            assert_eq!(native.as_bytes(), &raw_bytes[..]);
            let restored = native.to_os_string().expect("Unix to_os_string should not fail");
            assert_eq!(restored, os_str);
        }
    }

    #[cfg(windows)]
    proptest! {
        #[test]
        fn test_proptest_native_roundtrip_windows(wide in proptest::collection::vec(any::<u16>(), 0..256)) {
            use std::os::windows::ffi::OsStringExt;
            let os_str = OsString::from_wide(&wide);
            let native = NativePath::from_os_str(&os_str);
            assert_eq!(native.len(), wide.len() * 2);
            let restored = native.to_os_string().expect("Windows to_os_string should not fail with even length");
            assert_eq!(restored, os_str);
        }

        #[test]
        fn test_windows_odd_byte_length_rejected(mut bytes in proptest::collection::vec(any::<u8>(), 0..255)) {
            if bytes.len().is_multiple_of(2) {
                bytes.push(0x42);
            }
            let native = NativePath::from_bytes(bytes);
            let result = native.to_os_string();
            assert_eq!(result, Err(PathError::InvalidNativeLength));
        }
    }

    // G.2: Key rules property tests
    proptest! {
        #[test]
        fn test_proptest_key_rules_case_policy(name in "[a-zA-Z0-9_-]{1,30}") {
            let upper = name.to_uppercase();
            let lower = name.to_lowercase();
            let rel_upper = RelPath::from_components([&upper]).unwrap();
            let rel_lower = RelPath::from_components([&lower]).unwrap();

            let key_upper_ci = rel_upper.path_key(PathKeyPolicy { case_sensitive: false });
            let key_lower_ci = rel_lower.path_key(PathKeyPolicy { case_sensitive: false });
            prop_assert_eq!(key_upper_ci, key_lower_ci);

            if upper != lower {
                let key_upper_cs = rel_upper.path_key(PathKeyPolicy { case_sensitive: true });
                let key_lower_cs = rel_lower.path_key(PathKeyPolicy { case_sensitive: true });
                prop_assert_ne!(key_upper_cs, key_lower_cs);
            }
        }

        #[test]
        fn test_proptest_display_and_separator_independence(
            comps in proptest::collection::vec("[a-zA-Z0-9_-]{1,15}", 1..8)
        ) {
            let rel = RelPath::from_components(&comps).unwrap();
            let display = rel.display();
            let expected_display = comps.join("/");
            prop_assert_eq!(&display, &expected_display);

            // Path key never contains backslashes
            let key = rel.path_key(PathKeyPolicy { case_sensitive: false });
            prop_assert!(!key.contains(&b'\\'));
        }

        #[test]
        fn test_proptest_valid_components_never_contain_forbidden(
            comps in proptest::collection::vec("[a-zA-Z0-9_.-]{1,15}", 1..6)
        ) {
            // Filter components to avoid pure '.' or '..' or empty
            if comps.iter().all(|c| c != "." && c != ".." && !c.contains('/') && !c.contains('\\') && !c.is_empty()) {
                let rel = RelPath::from_components(&comps);
                if let Ok(r) = rel {
                    for c in r.components() {
                        prop_assert_ne!(c, "..");
                        prop_assert_ne!(c, ".");
                        prop_assert!(!c.contains('\0'));
                        prop_assert!(!c.contains('/'));
                        prop_assert!(!c.contains('\\'));
                    }
                }
            }
        }
    }

    // NFC and NFD spellings produce identical key
    #[test]
    fn test_nfc_vs_nfd_key_equality() {
        // NFC: 'é' is U+00E9 ([0xC3, 0xA9])
        let nfc_name = "caf\u{00E9}.jpg";
        // NFD: 'e' + U+0301 ([0x65, 0xCC, 0x81])
        let nfd_name = "cafe\u{0301}.jpg";

        assert_ne!(nfc_name.as_bytes(), nfd_name.as_bytes());

        let rel_nfc = RelPath::from_components([nfc_name]).unwrap();
        let rel_nfd = RelPath::from_components([nfd_name]).unwrap();

        let key_nfc = rel_nfc.path_key(PathKeyPolicy {
            case_sensitive: false,
        });
        let key_nfd = rel_nfd.path_key(PathKeyPolicy {
            case_sensitive: false,
        });
        assert_eq!(key_nfc, key_nfd);

        let key_nfc_cs = rel_nfc.path_key(PathKeyPolicy {
            case_sensitive: true,
        });
        let key_nfd_cs = rel_nfd.path_key(PathKeyPolicy {
            case_sensitive: true,
        });
        assert_eq!(key_nfc_cs, key_nfd_cs);
    }

    // G.3: Table-driven rejections
    #[test]
    fn test_rejection_table_driven() {
        struct TestCase {
            name: &'static str,
            components: &'static [&'static str],
            expected_error: PathError,
        }

        let cases = [
            TestCase {
                name: "empty component",
                components: &["photos", "", "img.jpg"],
                expected_error: PathError::EmptyComponent,
            },
            TestCase {
                name: "cur dir dot",
                components: &["photos", ".", "img.jpg"],
                expected_error: PathError::CurrentDirComponent,
            },
            TestCase {
                name: "parent dir dot-dot",
                components: &["photos", "..", "img.jpg"],
                expected_error: PathError::ParentComponent,
            },
            TestCase {
                name: "nul byte",
                components: &["photos", "bad\0name.jpg"],
                expected_error: PathError::NulByte,
            },
            TestCase {
                name: "forward slash in component",
                components: &["photos", "sub/folder", "img.jpg"],
                expected_error: PathError::InvalidSeparator,
            },
            TestCase {
                name: "backslash in component",
                components: &["photos", "sub\\folder", "img.jpg"],
                expected_error: PathError::InvalidSeparator,
            },
            TestCase {
                name: "drive prefix in component",
                components: &["C:", "photos"],
                expected_error: PathError::AbsoluteComponent,
            },
        ];

        for tc in cases {
            let res = RelPath::from_components(tc.components);
            assert_eq!(
                res.err(),
                Some(tc.expected_error.clone()),
                "Failed test case: {}",
                tc.name
            );
        }
    }

    #[test]
    fn test_from_absolute_not_inside_root() {
        let root = Path::new("/var/photos");
        let abs = Path::new("/var/music/song.mp3");
        let res = RelPath::from_absolute(root, abs);
        assert_eq!(res.err(), Some(PathError::NotInsideRoot));
    }

    #[test]
    fn test_from_absolute_traversal_in_abs() {
        let root = Path::new("/var/photos");
        let abs = Path::new("/var/photos/../secret.txt");
        let res = RelPath::from_absolute(root, abs);
        assert_eq!(res.err(), Some(PathError::ParentComponent));
    }

    #[test]
    fn test_from_absolute_root_itself() {
        let root = Path::new("/var/photos");
        let res = RelPath::from_absolute(root, root).unwrap();
        assert!(res.is_empty());
        assert_eq!(res.display(), "");
        assert_eq!(res.file_name(), "");
        assert_eq!(
            res.path_key(PathKeyPolicy {
                case_sensitive: false
            }),
            Vec::<u8>::new()
        );
    }

    // G.10: Platform-specific tests
    #[test]
    #[cfg(unix)]
    fn test_unix_not_unicode_rejection() {
        use std::os::unix::ffi::OsStringExt;
        // Construct non-UTF8 byte path: 0xFF is invalid UTF-8
        let raw = b"/var/photos/invalid_\xFF_name.jpg".to_vec();
        let os_str = std::ffi::OsString::from_vec(raw);
        let abs = PathBuf::from(os_str);
        let root = Path::new("/var/photos");

        let res = RelPath::from_absolute(root, &abs);
        assert_eq!(res.err(), Some(PathError::NotUnicode));
    }

    #[test]
    #[cfg(windows)]
    fn test_windows_verbatim_support_and_rejections() {
        // Verbatim root and verbatim target
        let root = Path::new(r"\\?\C:\Photos");
        let abs = Path::new(r"\\?\C:\Photos\vacation\img.jpg");
        let res = RelPath::from_absolute(root, abs).unwrap();
        assert_eq!(res.display(), "vacation/img.jpg");

        // Mixed verbatim root and standard target
        let abs_std = Path::new(r"C:\Photos\vacation\img.jpg");
        let res_mismatch = RelPath::from_absolute(root, abs_std);
        assert_eq!(res_mismatch.err(), Some(PathError::VerbatimMismatch));

        // Mixed standard root and verbatim target
        let root_std = Path::new(r"C:\Photos");
        let res_mismatch2 = RelPath::from_absolute(root_std, abs);
        assert_eq!(res_mismatch2.err(), Some(PathError::VerbatimMismatch));
    }

    #[test]
    #[cfg(windows)]
    fn test_windows_unpaired_surrogate_rejected() {
        use std::os::windows::ffi::OsStringExt;
        // 0xD800 is an isolated high surrogate (invalid Unicode)
        let wide = [
            'C' as u16,
            ':' as u16,
            '\\' as u16,
            'P' as u16,
            'h' as u16,
            'o' as u16,
            't' as u16,
            'o' as u16,
            's' as u16,
            '\\' as u16,
            0xD800,
            '.' as u16,
            'j' as u16,
            'p' as u16,
            'g' as u16,
        ];
        let os_str = OsString::from_wide(&wide);
        let abs = PathBuf::from(os_str);
        let root = Path::new(r"C:\Photos");

        let res = RelPath::from_absolute(root, &abs);
        assert_eq!(res.err(), Some(PathError::NotUnicode));
    }
}
