use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT_BYTES: usize = 512 * 1024; // 512 KB cap

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("ExifTool executable not found at: {0}")]
    ExecutableNotFound(String),
    #[error("Input path must be absolute: {0}")]
    NonAbsolutePath(String),
    #[error("Input file not found: {0}")]
    FileNotFound(String),
    #[error("ExifTool command timed out after {0:?}")]
    Timeout(Duration),
    #[error("ExifTool output exceeded maximum size limit of {0} bytes")]
    OutputSizeExceeded(usize),
    #[error("ExifTool execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Failed to parse ExifTool JSON output: {0}")]
    ParseError(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
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
}

impl ExifToolAdapter {
    pub fn new() -> Result<Self, MetadataError> {
        // Locate exiftool in PATH or Homebrew/system paths
        let exe = which_exiftool().ok_or_else(|| {
            MetadataError::ExecutableNotFound("exiftool not found in PATH or standard paths".into())
        })?;
        Ok(Self {
            executable_path: exe,
        })
    }

    pub fn with_executable(path: PathBuf) -> Self {
        Self {
            executable_path: path,
        }
    }

    /// Extract metadata for a single file using ExifTool invoked WITHOUT A SHELL.
    /// Strictly passes arguments as an array to `tokio::process::Command`.
    pub async fn extract_metadata(
        &self,
        file_path: &Path,
        timeout_duration: Option<Duration>,
    ) -> Result<NormalizedMetadata, MetadataError> {
        if !file_path.is_absolute() {
            return Err(MetadataError::NonAbsolutePath(
                file_path.to_string_lossy().to_string(),
            ));
        }

        if !file_path.exists() {
            return Err(MetadataError::FileNotFound(
                file_path.to_string_lossy().to_string(),
            ));
        }

        let timeout = timeout_duration.unwrap_or(DEFAULT_TIMEOUT);

        // Invoked directly without any shell wrapper (no sh, bash, cmd, powershell).
        // Argument array strictly defined. Stderr sent to Stdio::null(). Child killed on drop.
        let mut child = Command::new(&self.executable_path)
            .arg("-j")
            .arg("-n")
            .arg("-charset")
            .arg("filename=UTF8")
            .arg("-api")
            .arg("WindowsLongPath=1")
            .arg(file_path.as_os_str())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(MetadataError::Io)?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| MetadataError::ExecutionFailed("Failed to capture stdout".into()))?;

        // Read stdout with size cap
        let read_future = async {
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
            Ok(buffer)
        };

        // Explicitly kill and wait on child upon timeout or output-size overflow
        let output_bytes = match tokio::time::timeout(timeout, read_future).await {
            Ok(Ok(bytes)) => {
                let status = child.wait().await?;
                if !status.success() && bytes.is_empty() {
                    return Err(MetadataError::ExecutionFailed(format!(
                        "ExifTool exited with status: {}",
                        status
                    )));
                }
                bytes
            }
            Ok(Err(e @ MetadataError::OutputSizeExceeded(_))) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(e);
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
        let items: Vec<RawExifItem> = serde_json::from_slice(json_bytes).map_err(|e| {
            let raw_str = String::from_utf8_lossy(json_bytes);
            let truncated = if raw_str.len() > 200 {
                let prefix: String = raw_str.chars().take(200).collect();
                format!("{}... [truncated]", prefix)
            } else {
                raw_str.into_owned()
            };
            MetadataError::ParseError(format!("JSON decode error: {} on output: {}", e, truncated))
        })?;

        let first = items.into_iter().next().ok_or_else(|| {
            MetadataError::ParseError("ExifTool returned empty JSON array".into())
        })?;

        if let Some(err) = first.error {
            return Err(MetadataError::ParseError(format!(
                "ExifTool error tag: {}",
                err
            )));
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
            MetadataError::NonAbsolutePath(_) => (),
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
            MetadataError::FileNotFound(_) => (),
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
            Err(MetadataError::ParseError(msg)) => {
                // Ensure truncated error message when invalid output is long
                assert!(!msg.is_empty());
            }
            Err(e) => panic!("Expected Ok(empty) or ParseError, got: {:?}", e),
        }

        // Test explicit invalid JSON parse with truncation check
        let invalid_json = "X".repeat(300).into_bytes();
        let parse_err = adapter
            .parse_json_output(&invalid_json)
            .expect_err("Must fail on invalid JSON");
        match parse_err {
            MetadataError::ParseError(msg) => {
                assert!(
                    msg.contains("[truncated]"),
                    "Error message should contain [truncated], got: {}",
                    msg
                );
            }
            other => panic!("Expected ParseError, got: {:?}", other),
        }
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
