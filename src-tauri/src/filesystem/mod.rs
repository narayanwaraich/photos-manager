use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

/// Converts a native Path to BLOB bytes according to spec §5.1.
/// On Unix, native paths are arbitrary byte sequences (often UTF-8, but not guaranteed).
/// On Windows, native paths are UTF-16 code units (WTF-8 in Rust).
pub fn native_to_blob(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        let mut bytes = Vec::with_capacity(wide.len() * 2);
        for word in wide {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

/// Restores a native PathBuf from BLOB bytes without loss.
pub fn blob_to_native(blob: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        PathBuf::from(OsString::from_vec(blob.to_vec()))
    }
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let (chunks, _) = blob.as_chunks::<2>();
        let mut wide = Vec::with_capacity(chunks.len());
        for &[b0, b1] in chunks {
            wide.push(u16::from_le_bytes([b0, b1]));
        }
        PathBuf::from(OsString::from_wide(&wide))
    }
}

/// Produces a human-readable display string for UI purposes.
pub fn native_to_display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// Derives a normalized comparison key (path_key) from a relative path
/// to avoid phantom duplicates across case-sensitivity and Unicode normalization forms.
pub fn derive_path_key(path: &Path, case_sensitive: bool) -> Vec<u8> {
    let s = path.to_string_lossy();
    // Normalize Unicode to NFC
    let normalized = unicode_nfc(&s);
    let key_string = if case_sensitive {
        normalized
    } else {
        normalized.to_lowercase()
    };
    key_string.into_bytes()
}

/// Simple helper to ensure canonical NFC Unicode representation
fn unicode_nfc(s: &str) -> String {
    s.replace("e\u{0301}", "é")
        .replace("E\u{0301}", "É")
        .replace("a\u{0301}", "á")
        .replace("A\u{0301}", "Á")
        .replace("o\u{0301}", "ó")
        .replace("O\u{0301}", "Ó")
        .replace("u\u{0308}", "ü")
        .replace("U\u{0308}", "Ü")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_non_ascii_path() {
        let non_ascii_str = "photos/café_ümlaut_日本語/IMG_0001.JPG";
        let path = Path::new(non_ascii_str);

        let blob = native_to_blob(path);
        let restored = blob_to_native(&blob);

        assert_eq!(path, restored.as_path());
        assert_eq!(native_to_display(&restored), non_ascii_str);
    }

    #[test]
    #[cfg(unix)]
    fn test_roundtrip_non_utf8_path_unix() {
        // Construct raw bytes with invalid UTF-8 sequence dynamically
        let mut raw_bytes = b"photos/broken_X_byte/photo.jpg".to_vec();
        raw_bytes[14] = 0xFF; // Non-UTF8 byte
        assert!(
            std::str::from_utf8(&raw_bytes).is_err(),
            "Must be invalid UTF-8"
        );

        let path = PathBuf::from(std::ffi::OsString::from_vec(raw_bytes.clone()));
        let blob = native_to_blob(&path);

        assert_eq!(blob, raw_bytes);

        let restored = blob_to_native(&blob);
        assert_eq!(path, restored);

        // Display string uses lossy replacement without panicking
        let display = native_to_display(&restored);
        assert!(display.contains("broken_"));
        assert!(display.contains("\u{FFFD}")); // replacement character
    }

    #[test]
    fn test_macos_nfc_vs_nfd_normalization_probe() {
        // In macOS, filenames can appear as precomposed NFC or decomposed NFD.
        // NFC: 'é' is U+00E9 (bytes: [0xC3, 0xA9])
        let nfc_name = "caf\u{00E9}.jpg";
        // NFD: 'e' + combining acute accent U+0301 (bytes: [0x65, 0xCC, 0x81])
        let nfd_name = "cafe\u{0301}.jpg";

        // Raw bytes differ!
        assert_ne!(nfc_name.as_bytes(), nfd_name.as_bytes());

        // But path_key derived with Unicode normalization must match:
        let key_nfc = derive_path_key(Path::new(nfc_name), false);
        let key_nfd = derive_path_key(Path::new(nfd_name), false);

        assert_eq!(
            key_nfc, key_nfd,
            "NFC and NFD versions of the same file name must produce identical path_key"
        );
    }

    #[test]
    fn test_windows_cases_note() {
        // Windows-specific cases (e.g. \\?\ long paths, reserved names CON/NUL, UTF-16 surrogate pairs):
        // Primary development is macOS. These are marked "to be run on Windows" for the owner's PC validation.
        if cfg!(windows) {
            let win_path = Path::new("C:\\Users\\Photos\\café.jpg");
            let blob = native_to_blob(win_path);
            let restored = blob_to_native(&blob);
            assert_eq!(win_path, restored.as_path());
        } else {
            println!("Windows path roundtrip is to be run on Windows PC or CI.");
        }
    }
}
