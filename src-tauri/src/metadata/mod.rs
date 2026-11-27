use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT_BYTES: usize = 512 * 1024; // 512 KiB stdout cap
const MAX_STDERR_BYTES: usize = 64 * 1024; // 64 KiB stderr buffer cap

/// Process-wide concurrency limit for spawned ExifTool processes (TASK-1G decision h).
/// `tokio::sync::Semaphore::acquire()` is cancel-safe: dropping a waiting future consumes no permit.
static EXIFTOOL_SEMAPHORE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Builds the fixed ExifTool argument vector for the target OS (spec §3.3, TASK-1G decision e).
///
/// Never invokes a shell and never places file paths in CLI arguments (paths travel on stdin via `-@ -`).
/// `-api WindowsLongPath=1` is passed only on Windows (proven a no-op on macOS against fixtures).
pub fn exiftool_args_for_os(is_windows: bool) -> Vec<&'static str> {
    let mut args = vec![
        "-j",
        "-n",
        "-m",
        "-charset",
        "utf8",
        "-charset",
        "filename=utf8",
    ];
    if is_windows {
        args.push("-api");
        args.push("WindowsLongPath=1");
    }
    args.push("-@");
    args.push("-");
    args
}

/// Builds the fixed ExifTool argument vector for the current OS.
pub fn exiftool_args() -> Vec<&'static str> {
    exiftool_args_for_os(cfg!(windows))
}

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("ExifTool executable was not found")]
    ExecutableNotFound,
    #[error("Input path must be absolute")]
    NonAbsolutePath,
    #[error("Input file was not found")]
    FileNotFound,
    #[error("ExifTool command timed out after {0:?}")]
    Timeout(Duration),
    #[error("ExifTool output exceeded maximum size limit of {0} bytes")]
    OutputSizeExceeded(usize),
    #[error("ExifTool execution failed with exit status {code:?}")]
    ExecutionFailed { code: Option<i32> },
    #[error("ExifTool returned non-UTF-8 output")]
    InvalidUtf8,
    #[error("Failed to parse ExifTool JSON output")]
    ParseError,
    #[error("I/O error during ExifTool execution: {0:?}")]
    Io(std::io::ErrorKind),
}

impl From<std::io::Error> for MetadataError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.kind())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractionStatus {
    Success,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedMetadata {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: Option<u32>,
    pub date_taken: Option<String>,
    pub date_taken_status: ExtractionStatus,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawExifItem {
    #[serde(rename = "ImageWidth")]
    image_width: Option<u32>,
    #[serde(rename = "ImageHeight")]
    image_height: Option<u32>,
    #[serde(rename = "Orientation")]
    orientation: Option<u32>,
    #[serde(rename = "Make")]
    make: Option<String>,
    #[serde(rename = "Model")]
    model: Option<String>,
    #[serde(rename = "DateTimeOriginal")]
    date_time_original: Option<String>,
    #[serde(rename = "CreateDate")]
    create_date: Option<String>,
    #[serde(rename = "ModifyDate")]
    modify_date: Option<String>,
    #[serde(rename = "Error")]
    error: Option<String>,
}

pub struct ExifToolAdapter {
    executable_path: PathBuf,
    #[cfg(test)]
    last_child_pid: Option<std::sync::Arc<std::sync::Mutex<Option<u32>>>>,
}

impl ExifToolAdapter {
    pub fn new() -> Result<Self, MetadataError> {
        // Locate exiftool in PATH or Homebrew/system paths
        let exe = which_exiftool().ok_or(MetadataError::ExecutableNotFound)?;
        Ok(Self {
            executable_path: exe,
            #[cfg(test)]
            last_child_pid: None,
        })
    }

    pub fn with_executable(path: PathBuf) -> Self {
        Self {
            executable_path: path,
            #[cfg(test)]
            last_child_pid: None,
        }
    }

    #[cfg(test)]
    pub fn with_pid_recorder(
        path: PathBuf,
        recorder: std::sync::Arc<std::sync::Mutex<Option<u32>>>,
    ) -> Self {
        Self {
            executable_path: path,
            last_child_pid: Some(recorder),
        }
    }

    /// Extract metadata for a single file using ExifTool invoked WITHOUT A SHELL.
    /// Strictly passes arguments as an array to `tokio::process::Command`, enforces
    /// a process-wide concurrency limit of 2, drains stderr concurrently into a
    /// 64 KiB capped buffer, and bounds spawn-to-exit under one overall timeout.
    pub async fn extract_metadata(
        &self,
        file_path: &Path,
        timeout_duration: Option<Duration>,
    ) -> Result<NormalizedMetadata, MetadataError> {
        if !file_path.is_absolute() {
            return Err(MetadataError::NonAbsolutePath);
        }

        if !file_path.exists() {
            return Err(MetadataError::FileNotFound);
        }

        let timeout = timeout_duration.unwrap_or(DEFAULT_TIMEOUT);

        // Acquire process-wide concurrency permit before spawning (cancel-safe).
        let _permit = EXIFTOOL_SEMAPHORE
            .acquire()
            .await
            .map_err(|_| MetadataError::ExecutionFailed { code: None })?;

        // Invoked directly without any shell wrapper (no sh, bash, cmd, powershell).
        // Paths travel over stdin via `-@ -` to avoid Windows CLI encoding limits.
        let mut child = Command::new(&self.executable_path)
            .args(exiftool_args())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(MetadataError::from)?;

        #[cfg(test)]
        if let Some(ref recorder) = self.last_child_pid {
            let mut guard = match recorder.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            *guard = child.id();
        }

        let mut stdin = child
            .stdin
            .take()
            .ok_or(MetadataError::ExecutionFailed { code: None })?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or(MetadataError::ExecutionFailed { code: None })?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or(MetadataError::ExecutionFailed { code: None })?;

        let mut path_bytes = file_path.to_string_lossy().into_owned().into_bytes();
        path_bytes.push(b'\n');

        // One overall deadline covers stdin write, concurrent stdout/stderr drain, and child exit.
        let io_and_wait = async {
            let write_stdin = async move {
                if let Err(err) = stdin.write_all(&path_bytes).await {
                    // Ignore BrokenPipe if child exited before reading stdin; exit status reports failure.
                    if err.kind() != std::io::ErrorKind::BrokenPipe {
                        return Err(MetadataError::from(err));
                    }
                }
                let _ = stdin.shutdown().await;
                drop(stdin);
                Ok::<(), MetadataError>(())
            };

            let read_stdout = async {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let bytes_read = stdout.read(&mut chunk).await?;
                    if bytes_read == 0 {
                        break;
                    }
                    if buffer.len() + bytes_read > MAX_OUTPUT_BYTES {
                        return Err(MetadataError::OutputSizeExceeded(MAX_OUTPUT_BYTES));
                    }
                    buffer.extend_from_slice(&chunk[..bytes_read]);
                }
                Ok::<Vec<u8>, MetadataError>(buffer)
            };

            let drain_stderr = async {
                let mut kept = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let bytes_read = stderr.read(&mut chunk).await?;
                    if bytes_read == 0 {
                        break;
                    }
                    // Keep at most MAX_STDERR_BYTES; continue draining the rest so child never blocks.
                    let remaining = MAX_STDERR_BYTES.saturating_sub(kept.len());
                    if remaining > 0 {
                        let to_copy = remaining.min(bytes_read);
                        kept.extend_from_slice(&chunk[..to_copy]);
                    }
                }
                Ok::<Vec<u8>, MetadataError>(kept)
            };

            let (_, stdout_bytes, stderr_bytes) =
                tokio::try_join!(write_stdin, read_stdout, drain_stderr)?;

            let status = child.wait().await.map_err(MetadataError::from)?;
            Ok::<(std::process::ExitStatus, Vec<u8>, Vec<u8>), MetadataError>((
                status,
                stdout_bytes,
                stderr_bytes,
            ))
        };

        let output_bytes = match tokio::time::timeout(timeout, io_and_wait).await {
            Ok(Ok((status, stdout_bytes, stderr_bytes))) => {
                if !status.success() && stdout_bytes.is_empty() {
                    if !stderr_bytes.is_empty() {
                        // Keep stderr text inside adapter logs only; never expose in MetadataError Display.
                        let err_preview = String::from_utf8_lossy(&stderr_bytes);
                        eprintln!(
                            "exiftool execution failed (status={:?}): {}",
                            status.code(),
                            err_preview.trim()
                        );
                    }
                    return Err(MetadataError::ExecutionFailed {
                        code: status.code(),
                    });
                }
                stdout_bytes
            }
            Ok(Err(e)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(e);
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(MetadataError::Timeout(timeout));
            }
        };

        self.parse_json_output(&output_bytes)
    }

    pub fn parse_json_output(
        &self,
        json_bytes: &[u8],
    ) -> Result<NormalizedMetadata, MetadataError> {
        // Non-UTF-8 output must fail explicitly rather than using lossy conversion (decision g).
        let json_str = std::str::from_utf8(json_bytes).map_err(|_| MetadataError::InvalidUtf8)?;

        let items: Vec<RawExifItem> = serde_json::from_str(json_str).map_err(|e| {
            eprintln!("exiftool JSON decode error: {e}");
            MetadataError::ParseError
        })?;

        let first = items.into_iter().next().ok_or(MetadataError::ParseError)?;

        if let Some(err) = first.error {
            // ExifTool item Error field may include file path; log internally, never expose via MetadataError.
            eprintln!("exiftool reported item error ({} bytes)", err.len());
            return Err(MetadataError::ParseError);
        }

        // Determine date taken according to priority (spec §7.3):
        // DateTimeOriginal -> CreateDate -> ModifyDate
        let (date_taken, date_taken_status) = if let Some(dto) = first.date_time_original {
            (Some(dto), ExtractionStatus::Success)
        } else if let Some(cd) = first.create_date {
            (Some(cd), ExtractionStatus::Partial)
        } else if let Some(md) = first.modify_date {
            (Some(md), ExtractionStatus::Partial)
        } else {
            (None, ExtractionStatus::Failed)
        };

        Ok(NormalizedMetadata {
            width: first.image_width,
            height: first.image_height,
            orientation: first.orientation,
            date_taken,
            date_taken_status,
            camera_make: first.make,
            camera_model: first.model,
        })
    }
}

/// Attempts to locate a hermetic bundled ExifTool from application resources.
pub fn find_bundled_exiftool() -> Option<PathBuf> {
    // 1. Explicit override environment variable (used by tests / custom deployment)
    if let Ok(override_dir) = std::env::var("PHOTOS_RESOURCES_DIR") {
        let base = PathBuf::from(override_dir);
        for rel in [
            "exiftool/exiftool",
            "exiftool/exiftool.exe",
            "resources/exiftool/exiftool",
            "resources/exiftool/exiftool.exe",
        ] {
            let candidate = base.join(rel);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 2. Relative to current running executable
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            // macOS .app bundle structure:
            //   Contents/MacOS/<exe>
            //   Contents/Resources/resources/exiftool/exiftool or Contents/Resources/exiftool/exiftool
            if let Some(contents_dir) = parent.parent() {
                let resources = contents_dir.join("Resources");
                for rel in [
                    "resources/exiftool/exiftool",
                    "exiftool/exiftool",
                    "resources/exiftool/exiftool.exe",
                    "exiftool/exiftool.exe",
                ] {
                    let candidate = resources.join(rel);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }

            // Windows / Linux flat bundle structure (resources adjacent to executable)
            for rel in [
                "resources/exiftool/exiftool.exe",
                "resources/exiftool/exiftool",
                "exiftool/exiftool.exe",
                "exiftool/exiftool",
            ] {
                let candidate = parent.join(rel);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    // 3. Staged resources relative to CARGO_MANIFEST_DIR (for dev/test runners)
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let base = PathBuf::from(manifest_dir)
            .join("resources")
            .join("exiftool");
        for file in ["exiftool", "exiftool.exe"] {
            let candidate = base.join(file);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Locates the ExifTool executable.
/// In production release builds, this strictly resolves the bundled ExifTool.
/// In debug/development mode or when `PHOTOS_DEV_ALLOW_SYSTEM_EXIFTOOL` is set,
/// it falls back to system PATH and standard developer locations.
pub fn which_exiftool() -> Option<PathBuf> {
    // 1. Always prioritize hermetic bundled resource
    if let Some(bundled) = find_bundled_exiftool() {
        return Some(bundled);
    }

    // 2. Dev-only fallback: only in debug builds or when explicitly allowed via dev environment variable
    let allow_system =
        cfg!(debug_assertions) || std::env::var("PHOTOS_DEV_ALLOW_SYSTEM_EXIFTOOL").is_ok();

    if allow_system {
        // Check system PATH
        if let Ok(path_var) = std::env::var("PATH") {
            for entry in std::env::split_paths(&path_var) {
                let candidate = entry.join("exiftool");
                if candidate.is_file() {
                    return Some(candidate);
                }
                #[cfg(windows)]
                {
                    let candidate_exe = entry.join("exiftool.exe");
                    if candidate_exe.is_file() {
                        return Some(candidate_exe);
                    }
                }
            }
        }

        // Standard local development paths (e.g. Homebrew on macOS)
        for candidate in [
            "/opt/homebrew/bin/exiftool",
            "/usr/local/bin/exiftool",
            "/usr/bin/exiftool",
        ] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[cfg(windows)]
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tempfile::tempdir;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests")
            .join("fixtures")
            .join("generated")
            .canonicalize()
            .expect("canonicalize fixtures dir")
    }

    /// Compiles a standalone single-process native executable using `rustc` inside `dir`.
    /// Works identically on Unix and Windows without `cmd.exe`, shells, or grandchild processes.
    fn compile_native_fake(dir: &Path, name: &str, source: &str) -> PathBuf {
        let src_path = dir.join(format!("{name}.rs"));
        let exe_path = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&src_path, source).expect("write fake rs source");

        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let output = std::process::Command::new(rustc)
            .arg("--edition")
            .arg("2021")
            .arg("-C")
            .arg("debuginfo=0")
            .arg(&src_path)
            .arg("-o")
            .arg(&exe_path)
            .output()
            .expect("invoke rustc for test fake");

        assert!(
            output.status.success(),
            "rustc failed to compile test fake: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exe_path
    }

    #[test]
    fn exiftool_args_are_pinned() {
        let win_args = exiftool_args_for_os(true);
        assert_eq!(
            win_args,
            vec![
                "-j",
                "-n",
                "-m",
                "-charset",
                "utf8",
                "-charset",
                "filename=utf8",
                "-api",
                "WindowsLongPath=1",
                "-@",
                "-",
            ]
        );

        let unix_args = exiftool_args_for_os(false);
        assert_eq!(
            unix_args,
            vec![
                "-j",
                "-n",
                "-m",
                "-charset",
                "utf8",
                "-charset",
                "filename=utf8",
                "-@",
                "-",
            ]
        );

        for args in [&win_args, &unix_args] {
            assert!(args.contains(&"-m"), "must pass -m to ignore minor errors");
            assert!(!args.iter().any(|a| a.contains("sh") || a.contains("cmd")));
        }

        assert_eq!(exiftool_args(), exiftool_args_for_os(cfg!(windows)));
    }

    #[tokio::test]
    async fn stderr_flood_does_not_block_or_masquerade_as_timeout() {
        let temp_dir = tempdir().expect("tempdir");
        let fake_src = r##"
use std::io::{Read, Write};

fn main() {
    let mut stdin_buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_buf);
    let mut err = std::io::stderr().lock();
    let block = vec![b'E'; 8192];
    // Write 512 KiB to stderr (far above typical 64 KiB OS pipe buffers and the 64 KiB cap)
    for _ in 0..64 {
        err.write_all(&block).expect("write stderr");
    }
    err.flush().expect("flush stderr");
    println!("{}", r#"[{"ImageWidth": 800, "ImageHeight": 600, "Orientation": 1}]"#);
}
"##;
        let fake_exe = compile_native_fake(temp_dir.path(), "fake_stderr_flood", fake_src);
        let dummy_input = temp_dir.path().join("photo.jpg");
        std::fs::write(&dummy_input, b"dummy").expect("write dummy");

        let adapter = ExifToolAdapter::with_executable(fake_exe);
        let meta = adapter
            .extract_metadata(&dummy_input, Some(Duration::from_secs(5)))
            .await
            .expect("stderr flood must not block or time out");

        assert_eq!(meta.width, Some(800));
        assert_eq!(meta.height, Some(600));
        assert_eq!(meta.orientation, Some(1));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wait_after_stdout_close_is_inside_the_timeout() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempdir().expect("tempdir");
        let script_path = temp_dir.path().join("fake_close_stdout_then_hang.sh");
        // Close stdout immediately so stdout reader sees EOF, then keep process alive.
        std::fs::write(&script_path, "#!/bin/sh\nexec 1>&-\nexec /bin/sleep 30\n")
            .expect("write script");
        let mut perms = std::fs::metadata(&script_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("set_permissions");

        let dummy_input = temp_dir.path().join("dummy.jpg");
        std::fs::write(&dummy_input, b"dummy").expect("write dummy");

        let adapter = ExifToolAdapter::with_executable(script_path);
        let started = Instant::now();
        let err = adapter
            .extract_metadata(&dummy_input, Some(Duration::from_millis(300)))
            .await
            .expect_err("must time out waiting for child exit after stdout closes");

        assert!(
            matches!(err, MetadataError::Timeout(_)),
            "expected Timeout, got: {err:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "wait after stdout close must be bounded by timeout, elapsed: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn error_display_carries_no_stderr_or_path() {
        let temp_dir = tempdir().expect("tempdir");
        let secret_dir = temp_dir.path().join("secret_album_dir_999");
        std::fs::create_dir_all(&secret_dir).expect("create secret dir");
        let secret_file = secret_dir.join("private_family_photo_777.jpg");
        std::fs::write(&secret_file, b"dummy").expect("write secret file");

        // 1. NonAbsolutePath & FileNotFound must not leak path strings
        let rel_path = Path::new("secret_rel_folder/secret_photo.jpg");
        let adapter = ExifToolAdapter::with_executable(temp_dir.path().join("unused"));
        let non_abs = adapter
            .extract_metadata(rel_path, None)
            .await
            .expect_err("relative path must fail");
        assert!(!non_abs.to_string().contains("secret_rel_folder"));
        assert!(!non_abs.to_string().contains("secret_photo.jpg"));

        let missing = secret_dir.join("missing_secret_888.jpg");
        let not_found = adapter
            .extract_metadata(&missing, None)
            .await
            .expect_err("missing path must fail");
        assert!(!not_found.to_string().contains("secret_album_dir_999"));
        assert!(!not_found.to_string().contains("missing_secret_888.jpg"));

        // 2. Non-UTF-8 output must fail with InvalidUtf8, never lossy success
        let non_utf8_err = adapter
            .parse_json_output(b"\xff\xfe\xfd[{\"ImageWidth\": 10}]")
            .expect_err("non-UTF-8 output must be an error");
        assert!(matches!(non_utf8_err, MetadataError::InvalidUtf8));

        // 3. ParseError with ExifTool Error tag containing a path must not leak the path
        let json_with_path = br#"[{"Error": "File not found: /Users/alice/secret_album_dir_999/private_family_photo_777.jpg"}]"#;
        let parse_err = adapter
            .parse_json_output(json_with_path)
            .expect_err("error tag must fail");
        assert!(matches!(parse_err, MetadataError::ParseError));
        assert!(!parse_err.to_string().contains("secret_album_dir_999"));
        assert!(!parse_err.to_string().contains("private_family_photo_777"));

        // 4. Child failing with stderr text must not carry stderr in MetadataError Display
        let fake_fail_src = r#"
use std::io::Write;
fn main() {
    let mut err = std::io::stderr().lock();
    writeln!(err, "SECRET_STDERR_TOKEN_ABC /Users/alice/secret_album_dir_999/private.jpg").unwrap();
    std::process::exit(2);
}
"#;
        let fake_fail = compile_native_fake(temp_dir.path(), "fake_fail_stderr", fake_fail_src);
        let fail_adapter = ExifToolAdapter::with_executable(fake_fail);
        let exec_err = fail_adapter
            .extract_metadata(&secret_file, Some(Duration::from_secs(5)))
            .await
            .expect_err("non-zero exit with empty stdout must fail");
        assert!(matches!(
            exec_err,
            MetadataError::ExecutionFailed { code: Some(2) }
        ));
        let display = exec_err.to_string();
        assert!(!display.contains("SECRET_STDERR_TOKEN_ABC"));
        assert!(!display.contains("secret_album_dir_999"));
        assert!(!display.contains("private.jpg"));
    }

    #[tokio::test]
    async fn concurrency_limit_is_two() {
        let temp_dir = tempdir().expect("tempdir");
        let state_dir = temp_dir.path().join("state");
        std::fs::create_dir_all(&state_dir).expect("create state dir");

        // Escape backslashes for Windows path string literal inside generated Rust source.
        let state_dir_escaped = state_dir.to_string_lossy().replace('\\', "\\\\");
        let fake_src = format!(
            r##"
use std::io::Read;
use std::path::Path;
use std::time::Duration;

fn acquire_lock(lock_dir: &Path) {{
    while std::fs::create_dir(lock_dir).is_err() {{
        std::thread::sleep(Duration::from_millis(2));
    }}
}}

fn main() {{
    let mut stdin_buf = String::new();
    std::io::stdin().read_to_string(&mut stdin_buf).expect("read stdin");
    let input_path = stdin_buf.lines().next().expect("stdin path").trim();
    let stem = Path::new(input_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("stem")
        .to_string();

    let state_dir = Path::new("{state_dir_escaped}");
    let lock_dir = state_dir.join("lock");
    let cur_file = state_dir.join("cur.txt");
    let max_file = state_dir.join("max.txt");
    let active_marker = state_dir.join(format!("active_{{stem}}"));
    let release_marker = state_dir.join(format!("release_{{stem}}"));

    acquire_lock(&lock_dir);
    let cur: usize = std::fs::read_to_string(&cur_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
        + 1;
    let prev_max: usize = std::fs::read_to_string(&max_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    std::fs::write(&cur_file, cur.to_string()).expect("write cur");
    std::fs::write(&max_file, cur.max(prev_max).to_string()).expect("write max");
    std::fs::write(&active_marker, "1").expect("write active");
    let _ = std::fs::remove_dir(&lock_dir);

    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !release_marker.exists() && std::time::Instant::now() < deadline {{
        std::thread::sleep(Duration::from_millis(10));
    }}

    acquire_lock(&lock_dir);
    let cur: usize = std::fs::read_to_string(&cur_file)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .saturating_sub(1);
    std::fs::write(&cur_file, cur.to_string()).expect("write cur dec");
    let _ = std::fs::remove_file(&active_marker);
    let _ = std::fs::remove_dir(&lock_dir);

    println!("{{}}", r#"[{{"ImageWidth": 320, "ImageHeight": 240}}]"#);
}}
"##
        );

        let fake_exe = compile_native_fake(temp_dir.path(), "fake_concurrency", &fake_src);
        let inputs: Vec<PathBuf> = (0..3)
            .map(|i| {
                let p = temp_dir.path().join(format!("task_{i}.jpg"));
                std::fs::write(&p, b"dummy").expect("write input");
                p
            })
            .collect();

        let handles: Vec<_> = inputs
            .iter()
            .map(|input| {
                let exe = fake_exe.clone();
                let path = input.clone();
                tokio::spawn(async move {
                    let adapter = ExifToolAdapter::with_executable(exe);
                    adapter
                        .extract_metadata(&path, Some(Duration::from_secs(15)))
                        .await
                })
            })
            .collect();

        // Poll until 2 tasks are actively running inside the fake executable.
        let poll_deadline = Instant::now() + Duration::from_secs(10);
        let active_indices = loop {
            let active: Vec<usize> = (0..3)
                .filter(|i| state_dir.join(format!("active_task_{i}")).exists())
                .collect();
            if active.len() == 2 {
                break active;
            }
            assert!(
                Instant::now() < poll_deadline,
                "timed out waiting for 2 active tasks; saw {active:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        let waiting_idx = (0..3)
            .find(|i| !active_indices.contains(i))
            .expect("one task must still be waiting");
        assert!(
            !state_dir
                .join(format!("active_task_{waiting_idx}"))
                .exists(),
            "third task must wait while 2 permits are held"
        );

        // Release one of the two running tasks and prove the third starts only afterwards.
        let first_to_release = active_indices[0];
        let second_running = active_indices[1];
        std::fs::write(
            state_dir.join(format!("release_task_{first_to_release}")),
            "go",
        )
        .expect("release first task");

        let wait_third_deadline = Instant::now() + Duration::from_secs(10);
        while !state_dir
            .join(format!("active_task_{waiting_idx}"))
            .exists()
        {
            assert!(
                Instant::now() < wait_third_deadline,
                "third task never started after first task released its permit"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Release remaining tasks and collect all results.
        std::fs::write(
            state_dir.join(format!("release_task_{second_running}")),
            "go",
        )
        .expect("release second task");
        std::fs::write(state_dir.join(format!("release_task_{waiting_idx}")), "go")
            .expect("release third task");

        for h in handles {
            let meta = h
                .await
                .expect("task join")
                .expect("extract_metadata must succeed");
            assert_eq!(meta.width, Some(320));
            assert_eq!(meta.height, Some(240));
        }

        let max_overlap: usize = std::fs::read_to_string(state_dir.join("max.txt"))
            .expect("read max.txt")
            .trim()
            .parse()
            .expect("parse max");
        assert!(
            max_overlap <= 2,
            "max concurrent ExifTool processes must be <= 2, got {max_overlap}"
        );
        assert_eq!(max_overlap, 2, "expected 2 overlapping processes");
    }

    #[tokio::test]
    async fn test_exiftool_success() {
        let adapter = ExifToolAdapter::new().expect("ExifTool must be available for testing");
        let fixtures = fixtures_dir();
        let file = fixtures.join("image_orientation_6.jpg");

        let meta = adapter
            .extract_metadata(&file, None)
            .await
            .expect("Metadata extraction must succeed");

        assert_eq!(meta.width, Some(640));
        assert_eq!(meta.height, Some(480));
        assert_eq!(meta.orientation, Some(6));
        assert_eq!(meta.camera_make.as_deref(), Some("PhotoCorp"));
        assert_eq!(meta.camera_model.as_deref(), Some("CameraPro 1"));
        assert_eq!(meta.date_taken.as_deref(), Some("2026:06:15 14:30:00"));
        assert_eq!(meta.date_taken_status, ExtractionStatus::Success);
    }

    #[tokio::test]
    async fn test_exiftool_rejects_non_absolute_path() {
        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
        let rel_path = Path::new("tests/fixtures/generated/image_orientation_6.jpg");

        let err = adapter
            .extract_metadata(rel_path, None)
            .await
            .expect_err("Non-absolute path must be rejected");

        match err {
            MetadataError::NonAbsolutePath => (),
            other => panic!("Expected NonAbsolutePath, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_exiftool_missing_file() {
        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
        let temp_dir = tempdir().expect("tempdir");
        let missing = temp_dir.path().join("does_not_exist.jpg");

        let err = adapter
            .extract_metadata(&missing, None)
            .await
            .expect_err("Missing file must fail");

        match err {
            MetadataError::FileNotFound => (),
            other => panic!("Expected FileNotFound, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_exiftool_malformed_output() {
        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
        let fixtures = fixtures_dir();
        let txt_file = fixtures.join("not_an_image.txt");

        // Plain text file has no image width/height or EXIF tags, or returns an ExifTool error tag
        let result = adapter.extract_metadata(&txt_file, None).await;
        // Either ExifTool returns an error tag or returns empty/partial metadata without panicking
        match result {
            Ok(meta) => {
                // Should not have camera make/model
                assert_eq!(meta.camera_make, None);
                assert_eq!(meta.date_taken_status, ExtractionStatus::Failed);
            }
            Err(MetadataError::ParseError) => {}
            Err(e) => panic!("Expected Ok(empty) or ParseError, got: {:?}", e),
        }

        // Test explicit invalid JSON parse fails cleanly without leaking payload
        let invalid_json = "X".repeat(300).into_bytes();
        let parse_err = adapter
            .parse_json_output(&invalid_json)
            .expect_err("Must fail on invalid JSON");
        assert!(matches!(parse_err, MetadataError::ParseError));
        assert!(!parse_err.to_string().contains("XXX"));
    }

    #[tokio::test]
    async fn test_exiftool_timeout() {
        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
        let fixtures = fixtures_dir();
        let file = fixtures.join("image_orientation_6.jpg");

        // Set an impossible 1 nanosecond timeout
        let tiny_timeout = Duration::from_nanos(1);
        let err = adapter
            .extract_metadata(&file, Some(tiny_timeout))
            .await
            .expect_err("Should time out on 1ns timeout");

        match err {
            MetadataError::Timeout(_) => (),
            other => panic!("Expected Timeout error, got: {:?}", other),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_child_killed_after_timeout() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempdir().expect("tempdir");
        let script_path = temp_dir.path().join("fake_slow_exiftool.sh");
        let pid_file = temp_dir.path().join("child.pid");

        // Tiny script spawned directly (not through sh -c in product code).
        // It writes its own PID to a file, then executes sleep.
        let script_content = format!(
            "#!/bin/sh\necho $$ > \"{}\"\nexec /bin/sleep 30\n",
            pid_file.display()
        );
        std::fs::write(&script_path, script_content).expect("Failed to write script");
        let mut perms = std::fs::metadata(&script_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("set_permissions");

        let adapter = ExifToolAdapter::with_executable(script_path);
        let dummy_input = temp_dir.path().join("dummy.jpg");
        std::fs::write(&dummy_input, b"dummy content").expect("write dummy");

        let timeout = Duration::from_millis(1000);
        let err = adapter
            .extract_metadata(&dummy_input, Some(timeout))
            .await
            .expect_err("Expected timeout error");

        match err {
            MetadataError::Timeout(_) => (),
            other => panic!("Expected Timeout, got: {:?}", other),
        }

        // Read the PID written by the child script
        assert!(pid_file.exists(), "Child PID file must exist");
        let pid_str = std::fs::read_to_string(&pid_file).expect("read pid file");
        let pid: i32 = pid_str.trim().parse().expect("valid pid integer");

        // Declare kill from libc / libSystem to assert the child process no longer exists
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }

        // Wait briefly for OS cleanup
        tokio::time::sleep(Duration::from_millis(50)).await;

        let res = unsafe { kill(pid, 0) };
        assert_eq!(
            res, -1,
            "Child process {} must no longer exist after timeout and kill",
            pid
        );
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        // ESRCH (3) = No such process
        assert_eq!(errno, 3, "errno must be ESRCH (3), got: {}", errno);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_child_killed_after_timeout_windows() {
        let temp_dir = tempdir().expect("tempdir");
        let pid_file = temp_dir.path().join("win_child.pid");
        let pid_file_escaped = pid_file.to_string_lossy().replace('\\', "\\\\");

        // Native single-process Windows executable (never goes through cmd.exe, no grandchildren).
        let fake_src = format!(
            r#"
use std::time::Duration;
fn main() {{
    std::fs::write("{pid_file_escaped}", std::process::id().to_string()).expect("write pid");
    std::thread::sleep(Duration::from_secs(30));
}}
"#
        );
        let fake_exe = compile_native_fake(temp_dir.path(), "fake_slow_win_exiftool", &fake_src);

        let recorded_pid = Arc::new(Mutex::new(None));
        let adapter = ExifToolAdapter::with_pid_recorder(fake_exe, Arc::clone(&recorded_pid));
        let dummy_input = temp_dir.path().join("dummy.jpg");
        std::fs::write(&dummy_input, b"dummy content").expect("write dummy");

        let err = adapter
            .extract_metadata(&dummy_input, Some(Duration::from_millis(1000)))
            .await
            .expect_err("Expected timeout error on Windows");
        assert!(
            matches!(err, MetadataError::Timeout(_)),
            "Expected Timeout, got: {err:?}"
        );

        let hook_pid = recorded_pid
            .lock()
            .expect("pid lock")
            .expect("hook must capture spawned child PID");
        assert!(pid_file.exists(), "Child PID file must exist");
        let pid_str = std::fs::read_to_string(&pid_file).expect("read pid file");
        let file_pid: u32 = pid_str.trim().parse().expect("valid pid integer");
        assert_eq!(hook_pid, file_pid, "Hook PID and child PID file must match");

        // Prove through Windows OS (`tasklist`) that the process is gone.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let output = std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {file_pid}"), "/FO", "CSV", "/NH"])
                .output()
                .expect("run tasklist");
            assert!(output.status.success(), "tasklist failed");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let pid_token = format!("\"{file_pid}\"");
            if !stdout.contains(&pid_token) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Windows child process {file_pid} is still alive in tasklist output: {stdout}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[tokio::test]
    async fn test_exiftool_non_ascii_filename() {
        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
        let fixtures = fixtures_dir();
        let src_file = fixtures.join("image_orientation_6.jpg");
        assert!(src_file.exists());

        let temp_dir = tempdir().expect("tempdir");
        // Non-ASCII filename containing Cyrillic, Japanese, and accented characters
        let non_ascii_name = "фото_写真_café_2026.jpg";
        let target_file = temp_dir.path().join(non_ascii_name);
        std::fs::copy(&src_file, &target_file).expect("copy fixture to non-ascii path");

        let meta = adapter
            .extract_metadata(&target_file, None)
            .await
            .expect("Metadata extraction for non-ASCII path should succeed");

        assert_eq!(meta.orientation, Some(6));
        assert_eq!(meta.width, Some(640));
        assert_eq!(meta.height, Some(480));
    }

    #[tokio::test]
    async fn test_exiftool_long_path() {
        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
        let fixtures = fixtures_dir();
        let src_file = fixtures.join("image_orientation_6.jpg");
        assert!(src_file.exists());

        let temp_dir = tempdir().expect("tempdir");
        // Build a deeply nested directory path exceeding 260 characters
        let mut deep_dir = temp_dir.path().to_path_buf();
        for i in 0..10 {
            deep_dir = deep_dir.join(format!("long_path_segment_{:02}_abcdefghij", i));
        }
        std::fs::create_dir_all(&deep_dir).expect("create deep dirs");
        let long_file = deep_dir.join("image_deeply_nested_orientation_6.jpg");
        std::fs::copy(&src_file, &long_file).expect("copy to long path");

        // Verify total path length is indeed > 260 characters
        let path_len = long_file.to_string_lossy().len();
        assert!(
            path_len > 260,
            "Target path must exceed 260 characters, was: {}",
            path_len
        );

        let meta = adapter
            .extract_metadata(&long_file, None)
            .await
            .expect("Metadata extraction for long path (>260 chars) should succeed");

        assert_eq!(meta.orientation, Some(6));
    }
}
