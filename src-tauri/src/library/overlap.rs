use unicode_normalization::UnicodeNormalization;

use crate::domain::{RootError, RootId, RootStatus};
use crate::library::canonical::CanonicalRoot;

/// Stored library root representation used for overlap checking (spec §3.5, D29b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRootPath {
    pub id: RootId,
    pub path_display: String,
    pub components: Vec<String>,
    pub status: RootStatus,
}

impl StoredRootPath {
    pub fn new(
        id: RootId,
        path_display: impl Into<String>,
        components: Vec<String>,
        status: RootStatus,
    ) -> Self {
        Self {
            id,
            path_display: path_display.into(),
            components,
            status,
        }
    }
}

/// Normalizes path components to NFC Unicode form and lowercases them (D29b).
///
/// This conservative policy ensures that:
/// - Canonical casing differences are caught (`/Photos` vs `/photos`).
/// - Unicode normalization differences are caught (`café` in NFC vs NFD).
/// - Case-sensitive disk differences over-reject rather than under-reject.
pub fn normalize_components(components: &[String]) -> Vec<String> {
    components
        .iter()
        .map(|c| c.nfc().collect::<String>().to_lowercase())
        .collect()
}

/// Checks whether a candidate root overlaps any stored root (decision b, D29).
///
/// Compares canonical component lists component-wise, never by string prefix.
/// Two roots overlap if:
/// 1. They are equal.
/// 2. The new root is nested inside an existing root.
/// 3. The new root contains an existing root.
///
/// Every stored root is checked regardless of status (active, paused, offline).
pub fn check_overlap(new: &CanonicalRoot, existing: &[StoredRootPath]) -> Result<(), RootError> {
    let new_norm = normalize_components(&new.components);

    for stored in existing {
        let stored_norm = normalize_components(&stored.components);

        let overlaps = if new_norm.len() == stored_norm.len() {
            new_norm == stored_norm
        } else if new_norm.len() < stored_norm.len() {
            // New root contains the stored root
            stored_norm.starts_with(&new_norm)
        } else {
            // New root is inside the stored root
            new_norm.starts_with(&stored_norm)
        };

        if overlaps {
            return Err(RootError::Overlaps {
                path: new.path_display.clone(),
                existing: stored.path_display.clone(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_canonical_root(display: &str, components: &[&str]) -> CanonicalRoot {
        CanonicalRoot {
            canonical_path: PathBuf::from(display),
            path_native: display.as_bytes().to_vec(),
            path_display: display.to_string(),
            components: components.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn make_stored_root(
        id: &str,
        display: &str,
        components: &[&str],
        status: RootStatus,
    ) -> StoredRootPath {
        StoredRootPath {
            id: RootId::from(id),
            path_display: display.to_string(),
            components: components.iter().map(|s| s.to_string()).collect(),
            status,
        }
    }

    #[test]
    fn test_overlap_rule_identical_roots_rejected() {
        let new_root = make_canonical_root("/Users/alice/photos", &["Users", "alice", "photos"]);
        let existing = vec![make_stored_root(
            "r1",
            "/Users/alice/photos",
            &["Users", "alice", "photos"],
            RootStatus::Active,
        )];

        let res = check_overlap(&new_root, &existing);
        assert!(matches!(res, Err(RootError::Overlaps { .. })));
    }

    #[test]
    fn test_overlap_rule_new_inside_existing_rejected() {
        let new_root = make_canonical_root(
            "/Users/alice/photos/vacation",
            &["Users", "alice", "photos", "vacation"],
        );
        let existing = vec![make_stored_root(
            "r1",
            "/Users/alice/photos",
            &["Users", "alice", "photos"],
            RootStatus::Active,
        )];

        let res = check_overlap(&new_root, &existing);
        assert!(matches!(res, Err(RootError::Overlaps { .. })));
    }

    #[test]
    fn test_overlap_rule_new_contains_existing_rejected() {
        let new_root = make_canonical_root("/Users/alice/photos", &["Users", "alice", "photos"]);
        let existing = vec![make_stored_root(
            "r1",
            "/Users/alice/photos/vacation",
            &["Users", "alice", "photos", "vacation"],
            RootStatus::Active,
        )];

        let res = check_overlap(&new_root, &existing);
        assert!(matches!(res, Err(RootError::Overlaps { .. })));
    }

    #[test]
    fn test_overlap_rule_siblings_with_common_string_prefix_allowed() {
        // Component-wise comparison: "photos" and "photos2" are different components
        let new_root = make_canonical_root("/Users/alice/photos2", &["Users", "alice", "photos2"]);
        let existing = vec![make_stored_root(
            "r1",
            "/Users/alice/photos",
            &["Users", "alice", "photos"],
            RootStatus::Active,
        )];

        let res = check_overlap(&new_root, &existing);
        assert!(
            res.is_ok(),
            "Sibling folders with common prefix must be allowed"
        );
    }

    #[test]
    fn test_overlap_rule_differing_only_in_case_rejected() {
        let new_root = make_canonical_root("/Users/alice/PHOTOS", &["Users", "alice", "PHOTOS"]);
        let existing = vec![make_stored_root(
            "r1",
            "/Users/alice/photos",
            &["Users", "alice", "photos"],
            RootStatus::Active,
        )];

        let res = check_overlap(&new_root, &existing);
        assert!(matches!(res, Err(RootError::Overlaps { .. })));
    }

    #[test]
    fn test_overlap_rule_differing_only_in_unicode_nfc_nfd_rejected() {
        // "café" with precomposed é (U+00E9) in new_root
        let new_root = make_canonical_root(
            "/Users/alice/caf\u{00E9}",
            &["Users", "alice", "caf\u{00E9}"],
        );
        // "café" with decomposed e + combining acute accent (U+0065 U+0301) in existing
        let existing = vec![make_stored_root(
            "r1",
            "/Users/alice/cafe\u{0301}",
            &["Users", "alice", "cafe\u{0301}"],
            RootStatus::Active,
        )];

        let res = check_overlap(&new_root, &existing);
        assert!(matches!(res, Err(RootError::Overlaps { .. })));
    }

    #[test]
    fn test_overlap_rule_offline_stored_root_still_rejected() {
        let new_root = make_canonical_root(
            "/Volumes/External/Photos",
            &["Volumes", "External", "Photos"],
        );
        let existing = vec![make_stored_root(
            "r1",
            "/Volumes/External/Photos",
            &["Volumes", "External", "Photos"],
            RootStatus::Offline,
        )];

        let res = check_overlap(&new_root, &existing);
        assert!(matches!(res, Err(RootError::Overlaps { .. })));
    }

    #[test]
    fn test_overlap_rule_many_roots_table_driven() {
        let existing = vec![
            make_stored_root("r1", "/a/b/c", &["a", "b", "c"], RootStatus::Active),
            make_stored_root("r2", "/x/y/z", &["x", "y", "z"], RootStatus::Paused),
            make_stored_root("r3", "/m/n", &["m", "n"], RootStatus::Offline),
        ];

        // Disjoint path -> Ok
        let r_disjoint = make_canonical_root("/a/b/d", &["a", "b", "d"]);
        assert!(check_overlap(&r_disjoint, &existing).is_ok());

        // Overlap with r2 (nested)
        let r_nested = make_canonical_root("/x/y/z/w", &["x", "y", "z", "w"]);
        assert!(check_overlap(&r_nested, &existing).is_err());

        // Overlap with r3 (contains)
        let r_contains = make_canonical_root("/m", &["m"]);
        assert!(check_overlap(&r_contains, &existing).is_err());
    }
}
