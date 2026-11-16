use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};

use crate::filesystem::error::PathError;

/// Lossless native path byte wrapper (spec §5.1, D26).
///
/// On Unix, native paths are stored as the raw OsStr bytes.
/// On Windows, native paths are stored as little-endian UTF-16 code units (2 bytes each).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct NativePath(Vec<u8>);

impl NativePath {
    /// Creates a NativePath directly from raw stored bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrows the raw bytes of this native path.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes the wrapper and returns the underlying byte vector.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True if the byte sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Converts an OsStr into a lossless native BLOB representation.
    ///
    /// On Unix: raw bytes of the OsStr.
    /// On Windows: UTF-16LE code units (2 bytes per code unit).
    pub fn from_os_str(s: &OsStr) -> Self {
        #[cfg(unix)]
        {
            Self(s.as_bytes().to_vec())
        }
        #[cfg(windows)]
        {
            let wide: Vec<u16> = s.encode_wide().collect();
            let mut bytes = Vec::with_capacity(wide.len() * 2);
            for word in wide {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            Self(bytes)
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self(s.to_string_lossy().as_bytes().to_vec())
        }
    }

    /// Restores an OsString from the native BLOB bytes without loss.
    ///
    /// On Windows: an odd byte length is rejected with `PathError::InvalidNativeLength`
    /// rather than panicking or dropping the trailing byte.
    pub fn to_os_string(&self) -> Result<OsString, PathError> {
        #[cfg(unix)]
        {
            Ok(OsString::from_vec(self.0.clone()))
        }
        #[cfg(windows)]
        {
            if !self.0.len().is_multiple_of(2) {
                return Err(PathError::InvalidNativeLength);
            }
            let (chunks, _) = self.0.as_chunks::<2>();
            let mut wide = Vec::with_capacity(chunks.len());
            for &[b0, b1] in chunks {
                let word = u16::from_le_bytes([b0, b1]);
                wide.push(word);
            }
            Ok(OsString::from_wide(&wide))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let s = String::from_utf8(self.0.clone()).map_err(|_| PathError::NotUnicode)?;
            Ok(OsString::from(s))
        }
    }

    /// Converts into a PathBuf.
    pub fn to_path_buf(&self) -> Result<PathBuf, PathError> {
        self.to_os_string().map(PathBuf::from)
    }
}

impl std::ops::Deref for NativePath {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
