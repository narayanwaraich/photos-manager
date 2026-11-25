use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

use crate::database::validate_file_size;
use crate::filesystem::{PathError, RelPath};
use crate::scanner::classify::{classify, is_always_skipped_name, is_skipped_package_dir, Class};
use crate::scanner::types::{ScanCancel, ScanWarning};

/// A single discovered item from the filesystem walk.
pub enum WalkItem {
    Folder {
        rel_path: RelPath,
        modified_at_ms: Option<i64>,
    },
    File {
        rel_path: RelPath,
        size_bytes: i64,
        modified_at_ms: i64,
        created_at_ms: Option<i64>,
        file_identity: Option<String>,
        class: Class,
    },
    InaccessibleDir {
        rel_path: RelPath,
        warning: ScanWarning,
    },
    InaccessibleFile {
        rel_path: RelPath,
        warning: ScanWarning,
    },
    Warning(ScanWarning),
    SkippedByRule,
}

/// Converts a SystemTime to UTC Unix milliseconds.
/// Returns `None` if the timestamp is before 1970-01-01 or overflows i64.
pub fn system_time_to_unix_ms(time: SystemTime) -> Option<i64> {
    let dur = time.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let ms = dur.as_millis();
    if ms > i64::MAX as u128 {
        None
    } else {
        Some(ms as i64)
    }
}

/// Dispatches file identity extraction to the platform-specific implementation.
///
/// On Unix, extracts `<dev>:<ino>` safely via `std::os::unix::fs::MetadataExt`.
/// On Windows, queries `FILE_ID_INFO` via `crate::platform::windows::file_identity_windows`.
/// Returns `None` on any failure, as file identity is best-effort and must never abort a scan.
pub fn file_identity(path: &Path, metadata: &std::fs::Metadata) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Some(format!("{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        let _ = metadata;
        crate::platform::windows::file_identity_windows(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        let _ = metadata;
        None
    }
}

/// Parameters for running the filesystem walk.
pub struct WalkParams {
    pub root_path: PathBuf,
    pub include_hidden: bool,
    pub batch_size: usize,
    pub cancel: ScanCancel,
    #[cfg(test)]
    pub panic_after_batch: Option<usize>,
}

/// Executes the blocking directory traversal using `walkdir` (Decision b).
///
/// Sends batches of discovered items across `tx`. Bounded memory is guaranteed
/// because the channel has small capacity and batches are limited to `batch_size`.
pub fn run_walk(params: WalkParams, tx: tokio::sync::mpsc::Sender<Vec<WalkItem>>) {
    let mut it = WalkDir::new(&params.root_path)
        .follow_links(false)
        .same_file_system(false)
        .into_iter();

    let mut current_batch = Vec::with_capacity(params.batch_size);
    #[cfg(test)]
    let mut batches_sent: usize = 0;

    while let Some(entry_res) = it.next() {
        if params.cancel.is_cancelled() {
            break;
        }

        match entry_res {
            Ok(entry) => {
                let depth = entry.depth();
                let file_type = entry.file_type();
                let file_name_os = entry.file_name();

                // Skip symlinks (Decision d)
                if file_type.is_symlink() {
                    current_batch.push(WalkItem::SkippedByRule);
                    if current_batch.len() >= params.batch_size {
                        if tx
                            .blocking_send(std::mem::take(&mut current_batch))
                            .is_err()
                        {
                            return;
                        }
                        current_batch = Vec::with_capacity(params.batch_size);
                    }
                    continue;
                }

                // Root directory itself (depth == 0) already has a root-folder row (1C)
                if depth == 0 {
                    continue;
                }

                // Check Unicode in file name
                let file_name_str = match file_name_os.to_str() {
                    Some(s) => s,
                    None => {
                        let display = entry.path().to_string_lossy().to_string();
                        current_batch.push(WalkItem::Warning(ScanWarning {
                            code: "path_not_unicode".to_string(),
                            path_display: display,
                        }));
                        if file_type.is_dir() {
                            it.skip_current_dir();
                        }
                        continue;
                    }
                };

                // Check always-skipped names (case-insensitive on every OS, Decision d)
                if is_always_skipped_name(file_name_str) {
                    current_batch.push(WalkItem::SkippedByRule);
                    if file_type.is_dir() {
                        it.skip_current_dir();
                    }
                    continue;
                }

                // Check .photoslibrary packages (case-insensitive, Decision m)
                if file_type.is_dir() && is_skipped_package_dir(file_name_str) {
                    current_batch.push(WalkItem::SkippedByRule);
                    it.skip_current_dir();
                    continue;
                }

                // Check hidden entries unless include_hidden is enabled (Decision d)
                if !params.include_hidden {
                    let is_hidden_dot = file_name_str.starts_with('.');
                    #[allow(unused_mut)]
                    let mut is_hidden = is_hidden_dot;

                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        if let Ok(meta) = entry.metadata() {
                            if (meta.file_attributes() & 0x02) != 0 {
                                is_hidden = true;
                            }
                        }
                    }

                    if is_hidden {
                        current_batch.push(WalkItem::SkippedByRule);
                        if file_type.is_dir() {
                            it.skip_current_dir();
                        }
                        continue;
                    }
                }

                // Derive root-relative path
                let rel_path = match RelPath::from_absolute(&params.root_path, entry.path()) {
                    Ok(rp) => rp,
                    Err(PathError::NotUnicode) => {
                        current_batch.push(WalkItem::Warning(ScanWarning {
                            code: "path_not_unicode".to_string(),
                            path_display: entry.path().to_string_lossy().to_string(),
                        }));
                        if file_type.is_dir() {
                            it.skip_current_dir();
                        }
                        continue;
                    }
                    Err(pe) => {
                        current_batch.push(WalkItem::Warning(ScanWarning {
                            code: pe.code().to_string(),
                            path_display: entry.path().to_string_lossy().to_string(),
                        }));
                        if file_type.is_dir() {
                            it.skip_current_dir();
                        }
                        continue;
                    }
                };

                if file_type.is_dir() {
                    let mtime = entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(system_time_to_unix_ms);

                    current_batch.push(WalkItem::Folder {
                        rel_path,
                        modified_at_ms: mtime,
                    });
                } else if file_type.is_file() {
                    let class = classify(file_name_str);
                    if matches!(class, Class::Ignored) {
                        current_batch.push(WalkItem::SkippedByRule);
                        continue;
                    }

                    let meta = match entry.metadata() {
                        Ok(m) => m,
                        Err(_) => {
                            current_batch.push(WalkItem::InaccessibleFile {
                                rel_path: rel_path.clone(),
                                warning: ScanWarning {
                                    code: "entry_unreadable".to_string(),
                                    path_display: rel_path.display(),
                                },
                            });
                            continue;
                        }
                    };

                    let mtime_ms = match meta.modified().ok().and_then(system_time_to_unix_ms) {
                        Some(ms) => ms,
                        None => {
                            // Invalid timestamp: skip with warning (Decision f)
                            current_batch.push(WalkItem::Warning(ScanWarning {
                                code: "file_time_invalid".to_string(),
                                path_display: rel_path.display(),
                            }));
                            continue;
                        }
                    };

                    let created_ms = meta.created().ok().and_then(system_time_to_unix_ms);

                    let size_bytes = match validate_file_size(meta.len()) {
                        Ok(s) => s,
                        Err(err) => {
                            current_batch.push(WalkItem::Warning(ScanWarning {
                                code: err.code().to_string(),
                                path_display: rel_path.display(),
                            }));
                            continue;
                        }
                    };

                    let file_identity = file_identity(entry.path(), &meta);

                    current_batch.push(WalkItem::File {
                        rel_path,
                        size_bytes,
                        modified_at_ms: mtime_ms,
                        created_at_ms: created_ms,
                        file_identity,
                        class,
                    });
                }
            }
            Err(err) => {
                // Directory listing or stat failed (Decision i)
                if let Some(path) = err.path() {
                    if let Ok(rel) = RelPath::from_absolute(&params.root_path, path) {
                        let is_dir = path.is_dir();
                        if is_dir {
                            current_batch.push(WalkItem::InaccessibleDir {
                                rel_path: rel.clone(),
                                warning: ScanWarning {
                                    code: "entry_unreadable".to_string(),
                                    path_display: rel.display(),
                                },
                            });
                        } else {
                            current_batch.push(WalkItem::InaccessibleFile {
                                rel_path: rel.clone(),
                                warning: ScanWarning {
                                    code: "entry_unreadable".to_string(),
                                    path_display: rel.display(),
                                },
                            });
                        }
                    } else {
                        current_batch.push(WalkItem::Warning(ScanWarning {
                            code: "entry_unreadable".to_string(),
                            path_display: path.to_string_lossy().to_string(),
                        }));
                    }
                } else {
                    current_batch.push(WalkItem::Warning(ScanWarning {
                        code: "entry_unreadable".to_string(),
                        path_display: String::new(),
                    }));
                }
            }
        }

        if current_batch.len() >= params.batch_size {
            if tx
                .blocking_send(std::mem::take(&mut current_batch))
                .is_err()
            {
                return;
            }
            #[cfg(test)]
            {
                batches_sent += 1;
                if let Some(panic_after) = params.panic_after_batch {
                    if batches_sent >= panic_after {
                        panic!("test failure hook: walker thread panicked after batch");
                    }
                }
            }
            current_batch = Vec::with_capacity(params.batch_size);
        }
    }

    if !current_batch.is_empty() {
        let send_res = tx.blocking_send(current_batch);
        if send_res.is_ok() {
            #[cfg(test)]
            {
                batches_sent += 1;
                if let Some(panic_after) = params.panic_after_batch {
                    if batches_sent >= panic_after {
                        panic!("test failure hook: walker thread panicked after batch");
                    }
                }
            }
        }
    }
}
