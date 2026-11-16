use std::path::{Component, Path};

use crate::filesystem::error::PathError;
use crate::filesystem::native_path::NativePath;
use crate::filesystem::policy::PathKeyPolicy;

/// A validated root-relative path consisting strictly of normal components (spec §5.1, §6.4, D26).
///
/// It never touches the filesystem and never canonicalises.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct RelPath {
    components: Vec<String>,
}

impl RelPath {
    /// Creates an empty relative path (representing the root directory itself, D27).
    pub fn empty() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    /// True if this relative path has no components (the root-folder row).
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    /// Borrows the normal components of this relative path.
    pub fn components(&self) -> &[String] {
        &self.components
    }

    /// Constructs a `RelPath` from a sequence of component strings.
    ///
    /// Rejects empty components, '.', '..', separators, prefixes, NUL bytes,
    /// or non-normal path components.
    pub fn from_components<I, S>(components: I) -> Result<Self, PathError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut comps = Vec::new();
        for item in components {
            let s = item.as_ref();
            validate_component_str(s)?;
            comps.push(s.to_string());
        }
        Ok(Self { components: comps })
    }

    /// Derives a validated `RelPath` from an absolute root and absolute target path.
    ///
    /// Rejects:
    /// - target not inside root
    /// - any '.' or '..' component in root or relative path
    /// - empty components or NUL bytes
    /// - non-Unicode names (invalid UTF-8 on Unix, unpaired surrogates on Windows)
    /// - Windows verbatim (\\?\) mismatch between root and target
    pub fn from_absolute(root: &Path, abs: &Path) -> Result<Self, PathError> {
        check_verbatim_compatibility(root, abs)?;

        // Ensure root contains no relative traversal or non-Unicode components
        for comp in root.components() {
            match comp {
                Component::ParentDir => return Err(PathError::ParentComponent),
                Component::CurDir => return Err(PathError::CurrentDirComponent),
                Component::Normal(os_str) => {
                    let s = os_str.to_str().ok_or(PathError::NotUnicode)?;
                    if s.contains('\0') {
                        return Err(PathError::NulByte);
                    }
                }
                _ => {}
            }
        }

        let mut root_iter = root.components();
        let mut abs_iter = abs.components();

        // Target must begin with the exact components of root
        for root_comp in root_iter.by_ref() {
            match abs_iter.next() {
                Some(abs_comp) if abs_comp == root_comp => {}
                _ => return Err(PathError::NotInsideRoot),
            }
        }

        // Process remaining target components
        let mut components = Vec::new();
        for comp in abs_iter {
            match comp {
                Component::Prefix(_) | Component::RootDir => {
                    return Err(PathError::AbsoluteComponent);
                }
                Component::CurDir => {
                    return Err(PathError::CurrentDirComponent);
                }
                Component::ParentDir => {
                    return Err(PathError::ParentComponent);
                }
                Component::Normal(os_str) => {
                    let s = os_str.to_str().ok_or(PathError::NotUnicode)?;
                    if s.is_empty() {
                        return Err(PathError::EmptyComponent);
                    }
                    if s.contains('\0') {
                        return Err(PathError::NulByte);
                    }
                    components.push(s.to_string());
                }
            }
        }

        Ok(Self { components })
    }

    /// Encodes the relative path into native bytes using the host OS separator.
    ///
    /// On Unix: components joined by '/', as raw bytes.
    /// On Windows: components joined by '\', as little-endian UTF-16 code units.
    pub fn native(&self) -> NativePath {
        if self.components.is_empty() {
            return NativePath::default();
        }
        #[cfg(windows)]
        let joined = self.components.join("\\");
        #[cfg(not(windows))]
        let joined = self.components.join("/");

        NativePath::from_os_str(std::ffi::OsStr::new(&joined))
    }

    /// Produces a human-readable display string for UI purposes.
    /// Always valid UTF-8, joined with forward slashes ('/').
    pub fn display(&self) -> String {
        self.components.join("/")
    }

    /// Returns the file name (last normal component), or empty string if empty.
    pub fn file_name(&self) -> &str {
        self.components.last().map(|s| s.as_str()).unwrap_or("")
    }

    /// Derives the normalized `path_key` according to D26:
    /// - Root-relative path
    /// - Every component NFC-normalised on every volume
    /// - Lower-cased when volume is case-insensitive (str::to_lowercase after NFC)
    /// - Components joined by '/'
    /// - Represented as UTF-8 bytes
    pub fn path_key(&self, policy: PathKeyPolicy) -> Vec<u8> {
        use unicode_normalization::UnicodeNormalization;

        if self.components.is_empty() {
            return Vec::new();
        }

        let mut processed = Vec::with_capacity(self.components.len());
        for comp in &self.components {
            let nfc_str: String = comp.nfc().collect();
            let key_str = if policy.case_sensitive {
                nfc_str
            } else {
                nfc_str.to_lowercase()
            };
            processed.push(key_str);
        }

        processed.join("/").into_bytes()
    }
}

/// Helper to validate an individual component string.
fn validate_component_str(s: &str) -> Result<(), PathError> {
    if s.is_empty() {
        return Err(PathError::EmptyComponent);
    }
    if s == "." {
        return Err(PathError::CurrentDirComponent);
    }
    if s == ".." {
        return Err(PathError::ParentComponent);
    }
    if s.contains('\0') {
        return Err(PathError::NulByte);
    }
    if s.contains('/') || s.contains('\\') {
        return Err(PathError::InvalidSeparator);
    }
    if s.ends_with(':') {
        return Err(PathError::AbsoluteComponent);
    }

    let p = Path::new(s);
    for comp in p.components() {
        match comp {
            Component::Prefix(_) | Component::RootDir => {
                return Err(PathError::AbsoluteComponent);
            }
            Component::CurDir => {
                return Err(PathError::CurrentDirComponent);
            }
            Component::ParentDir => {
                return Err(PathError::ParentComponent);
            }
            Component::Normal(_) => {}
        }
    }
    Ok(())
}

#[cfg(windows)]
fn check_verbatim_compatibility(root: &Path, abs: &Path) -> Result<(), PathError> {
    use std::path::Prefix;
    let root_verbatim = match root.components().next() {
        Some(Component::Prefix(p)) => matches!(
            p.kind(),
            Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _)
        ),
        _ => false,
    };
    let abs_verbatim = match abs.components().next() {
        Some(Component::Prefix(p)) => matches!(
            p.kind(),
            Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _)
        ),
        _ => false,
    };
    if root_verbatim != abs_verbatim {
        return Err(PathError::VerbatimMismatch);
    }
    Ok(())
}

#[cfg(not(windows))]
fn check_verbatim_compatibility(_root: &Path, _abs: &Path) -> Result<(), PathError> {
    Ok(())
}
