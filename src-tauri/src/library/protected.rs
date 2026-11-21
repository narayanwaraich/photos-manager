use std::path::{Path, PathBuf};

use crate::domain::RootError;
use crate::library::canonical::{extract_components, CanonicalRoot};
use crate::library::overlap::normalize_components;

/// Context containing protected folders that must never be registered as library roots (spec §3.5, D29c).
#[derive(Debug, Clone)]
pub struct ProtectedContext {
    pub app_data_dir: Vec<String>,
    pub home_dir: Option<Vec<String>>,
    pub home_parent: Option<Vec<String>>,
    pub volumes_dir: Option<Vec<String>>,
    pub system_folders: Vec<Vec<String>>,
    pub is_windows: bool,
}

impl ProtectedContext {
    /// Builds the real protected folder context once from the runtime environment (D29c).
    pub fn from_environment(app_data: &Path) -> Self {
        let is_windows = cfg!(windows);
        let app_data_canon =
            std::fs::canonicalize(app_data).unwrap_or_else(|_| app_data.to_path_buf());
        let app_data_dir = normalize_components(&extract_components(&app_data_canon));

        let home_path = if is_windows {
            std::env::var_os("USERPROFILE").map(PathBuf::from)
        } else {
            std::env::var_os("HOME").map(PathBuf::from)
        };

        let home_dir = home_path.as_ref().map(|p| {
            let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            normalize_components(&extract_components(&canon))
        });

        let home_parent = home_path.as_ref().and_then(|p| {
            p.parent().map(|parent| {
                let canon = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
                normalize_components(&extract_components(&canon))
            })
        });

        let volumes_dir = if !is_windows {
            Some(normalize_components(&extract_components(Path::new(
                "/Volumes",
            ))))
        } else {
            None
        };

        let mut system_folders = Vec::new();
        if is_windows {
            for var in &[
                "SystemRoot",
                "WINDIR",
                "ProgramFiles",
                "ProgramFiles(x86)",
                "ProgramData",
            ] {
                if let Some(val) = std::env::var_os(var) {
                    let p = PathBuf::from(val);
                    let canon = std::fs::canonicalize(&p).unwrap_or(p);
                    system_folders.push(normalize_components(&extract_components(&canon)));
                }
            }
        } else {
            let mac_sys = [
                "/System",
                "/Library",
                "/usr",
                "/bin",
                "/sbin",
                "/etc",
                "/var",
                "/private",
                "/dev",
                "/cores",
                "/Applications",
            ];
            for sys in &mac_sys {
                let p = PathBuf::from(sys);
                let canon = std::fs::canonicalize(&p).unwrap_or(p);
                system_folders.push(normalize_components(&extract_components(&canon)));
            }
        }

        Self {
            app_data_dir,
            home_dir,
            home_parent,
            volumes_dir,
            system_folders,
            is_windows,
        }
    }

    /// Synthetic Unix context helper for portable, cross-platform unit tests.
    pub fn synthetic_unix(
        app_data: &[&str],
        home: &[&str],
        home_parent: &[&str],
        volumes: &[&str],
        system_folders: &[&[&str]],
    ) -> Self {
        Self {
            app_data_dir: home_to_vec(app_data),
            home_dir: Some(home_to_vec(home)),
            home_parent: Some(home_to_vec(home_parent)),
            volumes_dir: Some(home_to_vec(volumes)),
            system_folders: system_folders.iter().map(|s| home_to_vec(s)).collect(),
            is_windows: false,
        }
    }

    /// Synthetic Windows context helper for portable, cross-platform unit tests.
    pub fn synthetic_windows(
        app_data: &[&str],
        home: &[&str],
        home_parent: &[&str],
        system_folders: &[&[&str]],
    ) -> Self {
        Self {
            app_data_dir: home_to_vec(app_data),
            home_dir: Some(home_to_vec(home)),
            home_parent: Some(home_to_vec(home_parent)),
            volumes_dir: None,
            system_folders: system_folders.iter().map(|s| home_to_vec(s)).collect(),
            is_windows: true,
        }
    }
}

fn home_to_vec(slice: &[&str]) -> Vec<String> {
    slice.iter().map(|s| s.to_string()).collect()
}

/// Checks whether a candidate root violates protected folder rules (decision c, D29).
///
/// Rejection criteria:
/// 1. Filesystem root (`/`, `C:\`, UNC share root).
/// 2. User home folder itself (subfolders like `Pictures` are allowed).
/// 3. Parent of home (`/Users`, `C:\Users`) and `/Volumes`.
/// 4. Application data folder (equal to it, inside it, or containing it).
/// 5. OS system folders or subfolders.
/// 6. Windows: any component matching `System Volume Information` or `$RECYCLE.BIN`.
/// 7. macOS: any folder equal to or inside a `*.photoslibrary` package.
pub fn check_protected(new: &CanonicalRoot, ctx: &ProtectedContext) -> Result<(), RootError> {
    let new_norm = normalize_components(&new.components);

    // 1. Filesystem root:
    // On Unix, root "/" has 0 components.
    // On Windows, root "C:\" has 1 component ("c:").
    let is_fs_root = if ctx.is_windows {
        new_norm.len() <= 1
    } else {
        new_norm.is_empty()
    };
    if is_fs_root {
        return Err(RootError::Protected {
            path: new.path_display.clone(),
            reason: "Filesystem root folder cannot be added as a library root".to_string(),
        });
    }

    // 2. User home folder itself:
    if let Some(ref home) = ctx.home_dir {
        if new_norm == *home {
            return Err(RootError::Protected {
                path: new.path_display.clone(),
                reason: "User home folder itself cannot be added as a library root".to_string(),
            });
        }
    }

    // 3. Parent of home and /Volumes:
    if let Some(ref parent) = ctx.home_parent {
        if new_norm == *parent {
            return Err(RootError::Protected {
                path: new.path_display.clone(),
                reason: "Home parent folder cannot be added as a library root".to_string(),
            });
        }
    }
    if let Some(ref vol) = ctx.volumes_dir {
        if new_norm == *vol {
            return Err(RootError::Protected {
                path: new.path_display.clone(),
                reason: "/Volumes mount folder cannot be added as a library root".to_string(),
            });
        }
    }

    // 4. App-data folder: equal to it, inside it, or containing it:
    if new_norm == ctx.app_data_dir {
        return Err(RootError::Protected {
            path: new.path_display.clone(),
            reason: "Application data directory cannot be added as a library root".to_string(),
        });
    }
    if new_norm.len() > ctx.app_data_dir.len() && new_norm.starts_with(&ctx.app_data_dir) {
        return Err(RootError::Protected {
            path: new.path_display.clone(),
            reason: "Folder inside application data cannot be added as a library root".to_string(),
        });
    }
    if ctx.app_data_dir.len() > new_norm.len() && ctx.app_data_dir.starts_with(&new_norm) {
        return Err(RootError::Protected {
            path: new.path_display.clone(),
            reason: "Folder containing application data cannot be added as a library root"
                .to_string(),
        });
    }

    // 5. OS system folders:
    for sys in &ctx.system_folders {
        if new_norm == *sys || (new_norm.len() > sys.len() && new_norm.starts_with(sys)) {
            return Err(RootError::Protected {
                path: new.path_display.clone(),
                reason: "Operating system protected folder".to_string(),
            });
        }
    }

    // 6. Windows: System Volume Information or $RECYCLE.BIN
    if ctx.is_windows {
        for comp in &new_norm {
            if comp == "system volume information" || comp == "$recycle.bin" {
                return Err(RootError::Protected {
                    path: new.path_display.clone(),
                    reason: "Windows system volume or recycle bin folder".to_string(),
                });
            }
        }
    }

    // 7. macOS: *.photoslibrary package
    if !ctx.is_windows {
        for comp in &new.components {
            if comp.to_lowercase().ends_with(".photoslibrary") {
                return Err(RootError::Protected {
                    path: new.path_display.clone(),
                    reason: "Apple Photos library package cannot be added".to_string(),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_root(display: &str, comps: &[&str]) -> CanonicalRoot {
        CanonicalRoot {
            canonical_path: PathBuf::from(display),
            path_native: display.as_bytes().to_vec(),
            path_display: display.to_string(),
            components: comps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn test_protected_unix_filesystem_root_rejected() {
        let ctx = ProtectedContext::synthetic_unix(
            &[
                "users",
                "alice",
                "library",
                "application support",
                "photos-manager",
            ],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"], &["library"], &["usr"]],
        );

        let root = make_root("/", &[]);
        assert!(matches!(
            check_protected(&root, &ctx),
            Err(RootError::Protected { .. })
        ));
    }

    #[test]
    fn test_protected_windows_filesystem_root_rejected() {
        let ctx = ProtectedContext::synthetic_windows(
            &[
                "c:",
                "users",
                "alice",
                "appdata",
                "roaming",
                "photos-manager",
            ],
            &["c:", "users", "alice"],
            &["c:", "users"],
            &[&["c:", "windows"], &["c:", "program files"]],
        );

        let root = make_root(r"C:\", &["C:"]);
        assert!(matches!(
            check_protected(&root, &ctx),
            Err(RootError::Protected { .. })
        ));
    }

    #[test]
    fn test_protected_home_dir_itself_rejected() {
        let ctx = ProtectedContext::synthetic_unix(
            &[
                "users",
                "alice",
                "library",
                "application support",
                "photos-manager",
            ],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        );

        let home = make_root("/Users/alice", &["Users", "alice"]);
        assert!(matches!(
            check_protected(&home, &ctx),
            Err(RootError::Protected { .. })
        ));
    }

    #[test]
    fn test_protected_pictures_inside_home_allowed() {
        let ctx = ProtectedContext::synthetic_unix(
            &[
                "users",
                "alice",
                "library",
                "application support",
                "photos-manager",
            ],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        );

        let pictures = make_root("/Users/alice/Pictures", &["Users", "alice", "Pictures"]);
        assert!(check_protected(&pictures, &ctx).is_ok());
    }

    #[test]
    fn test_protected_folder_next_to_protected_allowed() {
        let ctx = ProtectedContext::synthetic_unix(
            &[
                "users",
                "alice",
                "library",
                "application support",
                "photos-manager",
            ],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        );

        let my_system = make_root("/System2", &["System2"]);
        assert!(check_protected(&my_system, &ctx).is_ok());
    }

    #[test]
    fn test_protected_home_parent_rejected() {
        let ctx = ProtectedContext::synthetic_unix(
            &[
                "users",
                "alice",
                "library",
                "application support",
                "photos-manager",
            ],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        );

        let users = make_root("/Users", &["Users"]);
        assert!(matches!(
            check_protected(&users, &ctx),
            Err(RootError::Protected { .. })
        ));
    }

    #[test]
    fn test_protected_volumes_mount_folder_rejected() {
        let ctx = ProtectedContext::synthetic_unix(
            &[
                "users",
                "alice",
                "library",
                "application support",
                "photos-manager",
            ],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        );

        let vol = make_root("/Volumes", &["Volumes"]);
        assert!(matches!(
            check_protected(&vol, &ctx),
            Err(RootError::Protected { .. })
        ));
    }

    #[test]
    fn test_protected_app_data_and_container_rejected() {
        let ctx = ProtectedContext::synthetic_unix(
            &[
                "users",
                "alice",
                "library",
                "application support",
                "photos-manager",
            ],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        );

        // Exact app data
        let exact = make_root(
            "/Users/alice/Library/Application Support/photos-manager",
            &[
                "Users",
                "alice",
                "Library",
                "Application Support",
                "photos-manager",
            ],
        );
        assert!(matches!(
            check_protected(&exact, &ctx),
            Err(RootError::Protected { .. })
        ));

        // Inside app data
        let inside = make_root(
            "/Users/alice/Library/Application Support/photos-manager/thumbnails",
            &[
                "Users",
                "alice",
                "Library",
                "Application Support",
                "photos-manager",
                "thumbnails",
            ],
        );
        assert!(matches!(
            check_protected(&inside, &ctx),
            Err(RootError::Protected { .. })
        ));

        // Folder containing app data (e.g. user selects Library)
        let container = make_root("/Users/alice/Library", &["Users", "alice", "Library"]);
        assert!(matches!(
            check_protected(&container, &ctx),
            Err(RootError::Protected { .. })
        ));
    }

    #[test]
    fn test_protected_os_system_folder_rejected() {
        let ctx = ProtectedContext::synthetic_unix(
            &[
                "users",
                "alice",
                "library",
                "application support",
                "photos-manager",
            ],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"], &["library"], &["usr"]],
        );

        let sys = make_root("/System", &["System"]);
        assert!(matches!(
            check_protected(&sys, &ctx),
            Err(RootError::Protected { .. })
        ));

        let sys_sub = make_root(
            "/System/Library/CoreServices",
            &["System", "Library", "CoreServices"],
        );
        assert!(matches!(
            check_protected(&sys_sub, &ctx),
            Err(RootError::Protected { .. })
        ));
    }

    #[test]
    fn test_protected_windows_system_volume_and_recycle_bin_rejected() {
        let ctx = ProtectedContext::synthetic_windows(
            &[
                "c:",
                "users",
                "alice",
                "appdata",
                "roaming",
                "photos-manager",
            ],
            &["c:", "users", "alice"],
            &["c:", "users"],
            &[&["c:", "windows"]],
        );

        let sys_vol = make_root(
            r"D:\System Volume Information",
            &["D:", "System Volume Information"],
        );
        assert!(matches!(
            check_protected(&sys_vol, &ctx),
            Err(RootError::Protected { .. })
        ));

        let recycle = make_root(r"D:\$RECYCLE.BIN", &["D:", "$RECYCLE.BIN"]);
        assert!(matches!(
            check_protected(&recycle, &ctx),
            Err(RootError::Protected { .. })
        ));
    }

    #[test]
    fn test_protected_photoslibrary_rejected_on_macos_allowed_on_windows() {
        let ctx_mac = ProtectedContext::synthetic_unix(
            &["users", "alice", "app-data"],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[],
        );

        let ctx_win = ProtectedContext::synthetic_windows(
            &["c:", "users", "alice", "app-data"],
            &["c:", "users", "alice"],
            &["c:", "users"],
            &[],
        );

        let mac_root = make_root(
            "/Users/alice/Pictures/Photos Library.photoslibrary",
            &["Users", "alice", "Pictures", "Photos Library.photoslibrary"],
        );
        assert!(
            matches!(
                check_protected(&mac_root, &ctx_mac),
                Err(RootError::Protected { .. })
            ),
            "Must be rejected on macOS"
        );

        let win_root = make_root(
            r"C:\Users\alice\Pictures\Photos Library.photoslibrary",
            &[
                "C:",
                "Users",
                "alice",
                "Pictures",
                "Photos Library.photoslibrary",
            ],
        );
        assert!(
            check_protected(&win_root, &ctx_win).is_ok(),
            "Must be allowed on Windows"
        );
    }

    #[test]
    fn test_protected_context_from_environment() {
        let temp = tempdir().expect("tempdir");
        let app_data = temp.path().join("test_app_data");
        std::fs::create_dir_all(&app_data).expect("create app data");

        let ctx = ProtectedContext::from_environment(&app_data);
        println!("=== ProtectedContext on runner ===");
        println!("App data: {:?}", ctx.app_data_dir);
        println!("User home: {:?}", ctx.home_dir);
        println!("User home parent: {:?}", ctx.home_parent);
        println!("System folders count: {}", ctx.system_folders.len());
        for f in &ctx.system_folders {
            println!("  Protected system folder: {:?}", f);
        }
        println!("=================================");
        assert!(
            !ctx.app_data_dir.is_empty(),
            "App data components must not be empty"
        );
        assert!(
            !ctx.system_folders.is_empty(),
            "System folders must not be empty"
        );
    }
}
