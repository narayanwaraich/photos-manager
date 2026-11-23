use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::commands::error::{fixed_message_for_code, IpcError};
use crate::domain::{RemoveRootPreview, RootId, RootSummary};
use crate::library::protected::ProtectedContext;

/// Shared state tracking currently in-flight reachability checks per root (Decision d).
#[derive(Clone, Default)]
pub struct ActiveReachableChecks(pub Arc<Mutex<HashSet<RootId>>>);

/// Shared state guarding against opening concurrent folder picker dialogs (Decision e).
#[derive(Clone, Default)]
pub struct DialogGuard(pub Arc<AtomicBool>);

/// RAII guard resetting the dialog busy flag when the operation completes or aborts.
struct DialogOpenGuard(Arc<AtomicBool>);

impl Drop for DialogOpenGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Outcome kind for the `add_root` command wire format (spec §12.1, Decision b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddRootOutcomeKind {
    Added,
    Cancelled,
}

/// DTO returned across the IPC boundary for the `add_root` command (Decision b).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRootResultDto {
    pub outcome: AddRootOutcomeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    pub warnings: Vec<String>,
}

/// Plain async function for listing library roots with reachability checks (Decision b, d).
pub async fn list_roots_impl(
    pool: &SqlitePool,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
) -> Result<Vec<RootSummary>, IpcError> {
    let summaries = crate::library::roots::list_roots_with_in_flight(pool, in_flight)
        .await
        .map_err(IpcError::from)?;
    Ok(summaries)
}

/// Plain async function implementing root addition behind a pluggable picker closure (Decision a, b, c, e).
pub async fn add_root_with_picker<P, Fut>(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    dialog_guard: &DialogGuard,
    label: Option<String>,
    picker: P,
) -> Result<AddRootResultDto, IpcError>
where
    P: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<PathBuf>>,
{
    // 1. Guard against concurrent open dialogs
    if dialog_guard
        .0
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(IpcError::new(
            "dialog_busy",
            fixed_message_for_code("dialog_busy"),
        ));
    }
    let _open_guard = DialogOpenGuard(Arc::clone(&dialog_guard.0));

    // 2. Open picker (mocked in tests, native rfd in production)
    let selected_path = picker().await;

    let path = match selected_path {
        Some(p) => p,
        None => {
            return Ok(AddRootResultDto {
                outcome: AddRootOutcomeKind::Cancelled,
                root_id: None,
                warnings: Vec::new(),
            });
        }
    };

    // 3. Delegate to domain library::roots::add_root
    let outcome = crate::library::roots::add_root(pool, ctx, &path, label.as_deref())
        .await
        .map_err(IpcError::from)?;

    Ok(AddRootResultDto {
        outcome: AddRootOutcomeKind::Added,
        root_id: Some(outcome.root_id.to_string()),
        warnings: outcome
            .warnings
            .into_iter()
            .map(|w| w.as_str().to_string())
            .collect(),
    })
}

/// Plain async function for previewing root removal (Decision b).
pub async fn preview_remove_root_impl(
    pool: &SqlitePool,
    root_id_str: &str,
) -> Result<RemoveRootPreview, IpcError> {
    let root_id = RootId::from(root_id_str.to_string());
    let preview = crate::library::roots::preview_remove_root(pool, &root_id)
        .await
        .map_err(IpcError::from)?;
    Ok(preview)
}

/// Plain async function for executing verified root removal (Decision b, h).
pub async fn remove_root_impl(
    pool: &SqlitePool,
    root_id_str: &str,
    expected_files: i64,
) -> Result<(), IpcError> {
    let root_id = RootId::from(root_id_str.to_string());
    crate::library::roots::remove_root(pool, &root_id, expected_files)
        .await
        .map_err(IpcError::from)?;
    Ok(())
}

//
// Tauri command wrappers
//

#[tauri::command]
pub async fn list_roots(
    pool: tauri::State<'_, SqlitePool>,
    in_flight: tauri::State<'_, ActiveReachableChecks>,
) -> Result<Vec<RootSummary>, IpcError> {
    list_roots_impl(&pool, &in_flight.0).await
}

#[tauri::command]
pub async fn add_root(
    pool: tauri::State<'_, SqlitePool>,
    ctx: tauri::State<'_, ProtectedContext>,
    dialog_guard: tauri::State<'_, DialogGuard>,
    window: tauri::WebviewWindow,
    label: Option<String>,
) -> Result<AddRootResultDto, IpcError> {
    add_root_with_picker(&pool, &ctx, &dialog_guard, label, || async move {
        let handle = rfd::AsyncFileDialog::new()
            .set_parent(&window)
            .pick_folder()
            .await;
        handle.map(|h| h.path().to_path_buf())
    })
    .await
}

#[tauri::command]
pub async fn preview_remove_root(
    pool: tauri::State<'_, SqlitePool>,
    root_id: String,
) -> Result<RemoveRootPreview, IpcError> {
    preview_remove_root_impl(&pool, &root_id).await
}

#[tauri::command]
pub async fn remove_root(
    pool: tauri::State<'_, SqlitePool>,
    root_id: String,
    expected_files: i64,
) -> Result<(), IpcError> {
    remove_root_impl(&pool, &root_id, expected_files).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::init_pool;
    use crate::domain::{RootStatus, VolumeKind};
    use crate::library::canonical::extract_components;
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    fn test_context(app_data_path: &Path) -> ProtectedContext {
        let canon =
            std::fs::canonicalize(app_data_path).unwrap_or_else(|_| app_data_path.to_path_buf());
        let app_data_comps =
            crate::library::overlap::normalize_components(&extract_components(&canon));
        let app_data_refs: Vec<&str> = app_data_comps.iter().map(|s| s.as_str()).collect();
        ProtectedContext::synthetic_unix(
            &app_data_refs,
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        )
    }

    #[test]
    fn test_wire_format_root_summary_matches_literal_json() {
        let summary = RootSummary {
            id: RootId::from("root_0193498f3b4970008b8b000000000001".to_string()),
            label: "My Photos".to_string(),
            path_display: "/Users/alice/Photos".to_string(),
            volume_kind: VolumeKind::Internal,
            volume_label: Some("Macintosh HD".to_string()),
            volume_case_sensitive: false,
            status: RootStatus::Active,
            reachable: true,
            file_count: 42,
            folder_count: 7,
        };

        let json = serde_json::to_string(&summary).expect("serialize RootSummary");
        let expected_json = r#"{"id":"root_0193498f3b4970008b8b000000000001","label":"My Photos","pathDisplay":"/Users/alice/Photos","volumeKind":"Internal","volumeLabel":"Macintosh HD","volumeCaseSensitive":false,"status":"Active","reachable":true,"fileCount":42,"folderCount":7}"#;
        assert_eq!(json, expected_json);
    }

    #[test]
    fn test_wire_format_add_root_result_matches_literal_json() {
        // Added with warnings
        let added_res = AddRootResultDto {
            outcome: AddRootOutcomeKind::Added,
            root_id: Some("root_0193498f3b4970008b8b000000000001".to_string()),
            warnings: vec![
                "case_sensitivity_unknown".to_string(),
                "network_volume".to_string(),
            ],
        };
        let added_json = serde_json::to_string(&added_res).expect("serialize AddRootResultDto");
        let expected_added = r#"{"outcome":"added","rootId":"root_0193498f3b4970008b8b000000000001","warnings":["case_sensitivity_unknown","network_volume"]}"#;
        assert_eq!(added_json, expected_added);

        // Cancelled (rootId omitted)
        let cancelled_res = AddRootResultDto {
            outcome: AddRootOutcomeKind::Cancelled,
            root_id: None,
            warnings: Vec::new(),
        };
        let cancelled_json =
            serde_json::to_string(&cancelled_res).expect("serialize AddRootResultDto");
        let expected_cancelled = r#"{"outcome":"cancelled","warnings":[]}"#;
        assert_eq!(cancelled_json, expected_cancelled);
    }

    #[test]
    fn test_wire_format_remove_root_preview_matches_literal_json() {
        let preview = RemoveRootPreview {
            root_id: RootId::from("root_0193498f3b4970008b8b000000000001".to_string()),
            file_count: 42,
            folder_count: 7,
            asset_count: 42,
        };
        let json = serde_json::to_string(&preview).expect("serialize RemoveRootPreview");
        let expected_json = r#"{"rootId":"root_0193498f3b4970008b8b000000000001","fileCount":42,"folderCount":7,"assetCount":42}"#;
        assert_eq!(json, expected_json);
    }

    #[test]
    fn test_wire_format_ipc_error_matches_literal_json() {
        let err = IpcError::new("dialog_busy", fixed_message_for_code("dialog_busy"));
        let json = serde_json::to_string(&err).expect("serialize IpcError");
        let expected_json =
            r#"{"code":"dialog_busy","message":"A file or folder dialog is already open."}"#;
        assert_eq!(json, expected_json);
    }

    #[tokio::test]
    async fn test_add_root_fake_picker_success_returns_added_and_creates_root_row() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let photo_folder = temp.path().join("my_photos");
        std::fs::create_dir_all(&photo_folder).expect("create photo_folder");

        let res = add_root_with_picker(
            &pool,
            &ctx,
            &dialog_guard,
            Some("My Custom Label".to_string()),
            || async { Some(photo_folder.clone()) },
        )
        .await
        .expect("add_root_with_picker should succeed");

        assert_eq!(res.outcome, AddRootOutcomeKind::Added);
        let root_id = res.root_id.expect("root_id present");

        let roots = list_roots_impl(&pool, &Arc::new(Mutex::new(HashSet::new())))
            .await
            .expect("list_roots_impl");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id.as_str(), root_id);
        assert_eq!(roots[0].label, "My Custom Label");
        assert!(roots[0].reachable);
    }

    #[tokio::test]
    async fn test_add_root_fake_picker_cancelled_returns_cancelled_and_writes_nothing() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let res = add_root_with_picker(
            &pool,
            &ctx,
            &dialog_guard,
            Some("Ignored Label".to_string()),
            || async { None },
        )
        .await
        .expect("cancelled is not an error");

        assert_eq!(res.outcome, AddRootOutcomeKind::Cancelled);
        assert!(res.root_id.is_none());
        assert!(res.warnings.is_empty());

        let roots = list_roots_impl(&pool, &Arc::new(Mutex::new(HashSet::new())))
            .await
            .expect("list_roots_impl");
        assert!(roots.is_empty(), "No root row should be written on cancel");
    }

    #[tokio::test]
    async fn test_add_root_protected_folder_returns_root_protected_fixed_message_no_path() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        // The app data folder itself is strictly protected
        let protected_path = app_data.clone();

        let err = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(protected_path.clone())
        })
        .await
        .expect_err("protected folder must return error");

        assert_eq!(err.code, "root_protected");
        assert_eq!(err.message, fixed_message_for_code("root_protected"));
        let serialized_err = serde_json::to_string(&err).unwrap();
        assert!(
            !serialized_err.contains(&protected_path.to_string_lossy().to_string()),
            "Serialized error must never leak folder paths"
        );
    }

    #[tokio::test]
    async fn test_add_root_overlapping_folder_returns_root_overlaps_fixed_message_no_path() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let parent_folder = temp.path().join("parent_folder");
        let sub_folder = parent_folder.join("sub_folder");
        std::fs::create_dir_all(&sub_folder).expect("create dirs");

        // First add parent folder
        let res1 = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(parent_folder.clone())
        })
        .await
        .expect("add parent folder");
        assert_eq!(res1.outcome, AddRootOutcomeKind::Added);

        // Attempt to add subfolder (nested overlap)
        let err = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(sub_folder.clone())
        })
        .await
        .expect_err("overlapping folder must return error");

        assert_eq!(err.code, "root_overlaps");
        assert_eq!(err.message, fixed_message_for_code("root_overlaps"));
        let serialized_err = serde_json::to_string(&err).unwrap();
        assert!(
            !serialized_err.contains(&sub_folder.to_string_lossy().to_string()),
            "Serialized error must never leak folder paths"
        );
    }

    #[tokio::test]
    async fn test_add_root_missing_folder_returns_root_path_invalid_fixed_message_no_path() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let non_existent = temp.path().join("does_not_exist_anywhere");

        let err = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(non_existent.clone())
        })
        .await
        .expect_err("missing folder must return error");

        assert_eq!(err.code, "root_path_invalid");
        assert_eq!(err.message, fixed_message_for_code("root_path_invalid"));
        let serialized_err = serde_json::to_string(&err).unwrap();
        assert!(
            !serialized_err.contains(&non_existent.to_string_lossy().to_string()),
            "Serialized error must never leak folder paths"
        );
    }

    #[tokio::test]
    async fn test_add_root_label_too_long_returns_root_label_invalid() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let folder = temp.path().join("valid_folder");
        std::fs::create_dir_all(&folder).expect("create folder");

        let long_label = "a".repeat(256);
        let err = add_root_with_picker(&pool, &ctx, &dialog_guard, Some(long_label), || async {
            Some(folder.clone())
        })
        .await
        .expect_err("label > 255 chars must fail");

        assert_eq!(err.code, "root_label_invalid");
        assert_eq!(err.message, fixed_message_for_code("root_label_invalid"));
    }

    #[tokio::test]
    async fn test_add_root_blank_label_falls_back_to_folder_name() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let folder = temp.path().join("vacation_photos");
        std::fs::create_dir_all(&folder).expect("create folder");

        let res = add_root_with_picker(
            &pool,
            &ctx,
            &dialog_guard,
            Some("   ".to_string()), // blank whitespace label
            || async { Some(folder.clone()) },
        )
        .await
        .expect("blank label must fall back to folder name");

        assert_eq!(res.outcome, AddRootOutcomeKind::Added);
        let roots = list_roots_impl(&pool, &Arc::new(Mutex::new(HashSet::new())))
            .await
            .expect("list_roots_impl");
        assert_eq!(roots[0].label, "vacation_photos");
    }

    #[tokio::test]
    async fn test_preview_and_remove_root_with_matching_count_succeeds() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let folder = temp.path().join("remove_me");
        std::fs::create_dir_all(&folder).expect("create folder");

        let res = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(folder.clone())
        })
        .await
        .expect("add root");
        let root_id = res.root_id.unwrap();

        // Preview removal
        let preview = preview_remove_root_impl(&pool, &root_id)
            .await
            .expect("preview_remove_root");
        assert_eq!(preview.root_id.as_str(), root_id);
        assert_eq!(preview.file_count, 0);

        // Remove root with matching count
        remove_root_impl(&pool, &root_id, preview.file_count)
            .await
            .expect("remove_root_impl with matching count");

        // Verify root is removed from catalogue, but folder on disk remains untouched
        let roots = list_roots_impl(&pool, &Arc::new(Mutex::new(HashSet::new())))
            .await
            .expect("list_roots_impl");
        assert!(
            roots.is_empty(),
            "Removed root must be deleted from catalogue"
        );
        assert!(folder.exists(), "Removal must never delete folder on disk");
    }

    #[tokio::test]
    async fn test_remove_root_with_stale_count_fails_with_root_changed_since_preview_and_deletes_nothing(
    ) {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let folder = temp.path().join("stale_test");
        std::fs::create_dir_all(&folder).expect("create folder");

        let res = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(folder.clone())
        })
        .await
        .expect("add root");
        let root_id = res.root_id.unwrap();

        // Actual file count is 0, provide stale expected count of 5
        let err = remove_root_impl(&pool, &root_id, 5)
            .await
            .expect_err("stale count must fail");

        assert_eq!(err.code, "root_changed_since_preview");
        assert_eq!(
            err.message,
            fixed_message_for_code("root_changed_since_preview")
        );

        // Ensure root status is still active (nothing deleted / changed)
        let roots = list_roots_impl(&pool, &Arc::new(Mutex::new(HashSet::new())))
            .await
            .expect("list_roots_impl");
        assert_eq!(roots[0].status, RootStatus::Active);
    }

    #[tokio::test]
    async fn test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable(
    ) {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let folder = temp.path().join("hung_share");
        std::fs::create_dir_all(&folder).expect("create folder");

        add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(folder.clone())
        })
        .await
        .expect("add root");

        let in_flight = Arc::new(Mutex::new(HashSet::new()));

        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Arc::new(std::sync::Mutex::new(release_rx));
        let rx_clone = release_rx.clone();

        // Use a 50ms timeout with a checker that blocks until released through a channel
        let start = Instant::now();
        let summaries = crate::library::roots::list_roots_with_checker_and_timeout(
            &pool,
            &in_flight,
            Duration::from_millis(50),
            move |_| {
                if let Ok(rx) = rx_clone.lock() {
                    let _ = rx.recv();
                }
                true
            },
        )
        .await
        .expect("list_roots_with_checker_and_timeout");
        let elapsed = start.elapsed();

        assert_eq!(summaries.len(), 1);
        assert!(
            !summaries[0].reachable,
            "Timed-out check must report reachable = false"
        );
        // Completed within 50ms + slack (well under 500ms)
        assert!(
            elapsed < Duration::from_millis(500),
            "Expected completion within timeout plus slack, took {:?}",
            elapsed
        );

        // Release the blocking checker thread so it doesn't hold the test worker thread
        let _ = release_tx.send(());
    }

    #[tokio::test]
    async fn test_list_roots_second_call_while_blocked_does_not_start_second_check() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let folder = temp.path().join("concurrent_check");
        std::fs::create_dir_all(&folder).expect("create folder");

        add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(folder.clone())
        })
        .await
        .expect("add root");

        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // First call: times out in 50ms, while the thread sleeps for 2 seconds
        let count_clone = Arc::clone(&call_count);
        let summaries1 = crate::library::roots::list_roots_with_checker_and_timeout(
            &pool,
            &in_flight,
            Duration::from_millis(50),
            move |_| {
                count_clone.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(2000));
                true
            },
        )
        .await
        .expect("call 1");

        assert_eq!(summaries1[0].reachable, false);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Second call while the first thread is still sleeping inside spawn_blocking
        let count_clone2 = Arc::clone(&call_count);
        let summaries2 = crate::library::roots::list_roots_with_checker_and_timeout(
            &pool,
            &in_flight,
            Duration::from_millis(50),
            move |_| {
                count_clone2.fetch_add(1, Ordering::SeqCst);
                true
            },
        )
        .await
        .expect("call 2");

        assert_eq!(summaries2[0].reachable, false);
        // The second call must NOT have invoked the checker function
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "Second call while root is in-flight must not start a second check"
        );
    }

    #[tokio::test]
    async fn test_dialog_guard_rejects_concurrent_second_add_with_dialog_busy() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let ctx = ProtectedContext::from_environment(temp.path());
        let dialog_guard = DialogGuard::default();

        // Simulate a dialog that remains open until signalled
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();

        let pool_clone = pool.clone();
        let ctx_clone = ctx.clone();
        let guard_clone = dialog_guard.clone();

        let task1 = tokio::spawn(async move {
            add_root_with_picker(&pool_clone, &ctx_clone, &guard_clone, None, || async move {
                let _ = started_tx.send(());
                let _ = finish_rx.await;
                None
            })
            .await
        });

        // Wait until task1 has entered and acquired the guard
        let _ = started_rx.await;

        // Second call arrives while task1's dialog is open: must immediately return dialog_busy
        let err2 = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async { None })
            .await
            .expect_err("second add_root while dialog is open must fail");

        assert_eq!(err2.code, "dialog_busy");
        assert_eq!(err2.message, fixed_message_for_code("dialog_busy"));

        // Let task1 finish
        let _ = finish_tx.send(());
        let res1 = task1.await.unwrap().expect("task1 finishes cancelled");
        assert_eq!(res1.outcome, AddRootOutcomeKind::Cancelled);

        // Third call after task1 has closed should now succeed (guard was released)
        let res3 = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async { None })
            .await
            .expect("add_root after dialog closes must succeed");
        assert_eq!(res3.outcome, AddRootOutcomeKind::Cancelled);
    }
}
