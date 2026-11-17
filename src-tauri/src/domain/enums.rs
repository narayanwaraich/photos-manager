use std::fmt;
use std::str::FromStr;

/// Error returned when parsing an unrecognized enum string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseEnumError {
    pub enum_name: &'static str,
    pub invalid_value: String,
}

impl fmt::Display for ParseEnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid variant '{}' for enum {}",
            self.invalid_value, self.enum_name
        )
    }
}

impl std::error::Error for ParseEnumError {}

macro_rules! define_catalogue_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident = $str_val:literal
            ),* $(,)?
        }
        default = $default_variant:ident
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum $name {
            $(
                $(#[$vmeta])*
                $variant,
            )*
        }

        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(
                        $name::$variant => $str_val,
                    )*
                }
            }

            pub const fn all_variants() -> &'static [Self] {
                &[
                    $(
                        $name::$variant,
                    )*
                ]
            }
        }

        impl Default for $name {
            fn default() -> Self {
                $name::$default_variant
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = ParseEnumError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $(
                        $str_val => Ok($name::$variant),
                    )*
                    _ => Err(ParseEnumError {
                        enum_name: stringify!($name),
                        invalid_value: s.to_string(),
                    }),
                }
            }
        }

        impl sqlx::Type<sqlx::Sqlite> for $name {
            fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
                <String as sqlx::Type<sqlx::Sqlite>>::type_info()
            }
        }

        impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for $name {
            fn encode_by_ref(
                &self,
                args: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
            ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
                args.push(sqlx::sqlite::SqliteArgumentValue::Text(self.as_str().into()));
                Ok(sqlx::encode::IsNull::No)
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for $name {
            fn decode(
                value: sqlx::sqlite::SqliteValueRef<'r>,
            ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
                let s = <&str as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
                Self::from_str(s).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            }
        }
    };
}

define_catalogue_enum! {
    /// Kind of storage volume (spec §5.2).
    pub enum VolumeKind {
        Internal = "internal",
        Removable = "removable",
        Network = "network",
        Unknown = "unknown",
    }
    default = Unknown
}

define_catalogue_enum! {
    /// Status of a registered library root (spec §5.2).
    pub enum RootStatus {
        Active = "active",
        Paused = "paused",
        Offline = "offline",
        Removed = "removed",
    }
    default = Active
}

define_catalogue_enum! {
    /// Status of a folder in the catalogue (spec §5.2).
    pub enum FolderStatus {
        Present = "present",
        Missing = "missing",
        Inaccessible = "inaccessible",
    }
    default = Present
}

define_catalogue_enum! {
    /// Status of a physical file in the catalogue (spec §5.2).
    pub enum FileStatus {
        Present = "present",
        Missing = "missing",
        Inaccessible = "inaccessible",
        Unsupported = "unsupported",
        Error = "error",
    }
    default = Present
}

define_catalogue_enum! {
    /// Kind of logical asset (spec §5.2).
    pub enum AssetKind {
        Image = "image",
        Video = "video",
    }
    default = Image
}

define_catalogue_enum! {
    /// Status of a logical asset in the catalogue (spec §5.2).
    pub enum AssetStatus {
        Present = "present",
        Missing = "missing",
        Unsupported = "unsupported",
        Error = "error",
    }
    default = Present
}
