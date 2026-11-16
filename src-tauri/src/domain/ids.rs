use std::fmt;

/// Generates a new time-ordered UUIDv7 string.
/// UUIDv7 is time-ordered, preventing SQLite B-tree index fragmentation at 100k+ rows.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Helper macro to generate strongly-typed string identifiers.
/// Each ID wraps a String and implements transparent SQLx encoding/decoding
/// so they can be bound directly into SQLite queries.
macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            sqlx::Type,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[sqlx(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Creates a new time-ordered UUIDv7 ID.
            pub fn new() -> Self {
                Self(new_id())
            }

            /// Borrows the inner string.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Unwraps the inner string.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

define_id!(VolumeId, "Strongly-typed identifier for a storage volume.");
define_id!(
    RootId,
    "Strongly-typed identifier for a library root directory."
);
define_id!(
    FolderId,
    "Strongly-typed identifier for a folder in the catalogue."
);
define_id!(
    FileId,
    "Strongly-typed identifier for a physical file in the catalogue."
);
define_id!(
    AssetId,
    "Strongly-typed identifier for a logical asset in the catalogue."
);
