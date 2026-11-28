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

/// Outcome kind for the `relocate_root` command wire format (TASK-1F decision f).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelocateRootOutcomeKind {
    Relocated,
    Cancelled,
}

/// DTO returned across the IPC boundary for the `relocate_root` command (TASK-1F decision f).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateRootResultDto {
    pub outcome: RelocateRootOutcomeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<RootSummary>,
}

/// Plain async function for listing library roots with reachability checks (Decision b, d).
pub async fn list_roots_impl(
    pool: &SqlitePool,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
) -> Result<Vec<RootSummary>, IpcError> {
    list_roots_with_active_jobs(pool, in_flight, |_| None).await
}

/// Lists library roots, applies definite live availability outcomes best-effort (TASK-1F decision d),
/// and populates `active_job_id` from the provided active job lookup (Decision j).
pub async fn list_roots_with_active_jobs(
    pool: &SqlitePool,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
    active_jobs: impl Fn(&RootId) -> Option<String>,
) -> Result<Vec<RootSummary>, IpcError> {
    list_roots_with_runtime_state(pool, in_flight, active_jobs, None).await
}

/// Lists library roots, applies definite live availability outcomes best-effort,
/// syncs per-root watchers (start for available Active, stop for Offline/Paused/Removed),
/// and populates `active_job_id`, `changes_detected`, and `watching`.
pub async fn list_roots_with_runtime_state(
    pool: &SqlitePool,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
    active_jobs: impl Fn(&RootId) -> Option<String>,
    watcher: Option<&crate::watcher::WatcherManager>,
) -> Result<Vec<RootSummary>, IpcError> {
    if let Some(wm) = watcher {
        wm.flush_debouncers(std::time::Instant::now());
    }

    let items = crate::library::roots::list_roots_with_availability(pool, in_flight)
        .await
        .map_err(IpcError::from)?;

    let mut summaries = Vec::with_capacity(items.len());
    for mut item in items {
        // Apply definite availability changes best-effort; errors are logged and never fail the list.
        match crate::library::apply_availability_result(
            pool,
            &item.summary.id,
            Some(item.summary.status),
            item.availability,
        )
        .await
        {
            Ok(updated_status) => {
                item.summary.status = updated_status;
            }
            Err(err) => {
                eprintln!(
                    "best-effort availability status update failed for root {}: {}",
                    item.summary.id,
                    err.code()
                );
            }
        }

        if let Some(wm) = watcher {
            if item.summary.status == crate::domain::RootStatus::Active
                && item.availability.is_available()
            {
                wm.ensure_watching(&item.summary.id, &item.native_path);
            } else if item.summary.status != crate::domain::RootStatus::Active {
                wm.stop_watching(&item.summary.id);
            }
            let (changes_detected, watching) = wm.status_for_root(&item.summary.id);
            item.summary.changes_detected =
                changes_detected && item.summary.last_scan_at_ms.is_some();
            item.summary.watching = watching;
        }

        item.summary.active_job_id = active_jobs(&item.summary.id);
        summaries.push(item.summary);
    }
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
    add_root_with_picker_and_watcher(pool, ctx, dialog_guard, label, picker, None).await
}

/// Plain async function implementing root addition with optional `WatcherManager` start (best-effort).
pub async fn add_root_with_picker_and_watcher<P, Fut>(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    dialog_guard: &DialogGuard,
    label: Option<String>,
    picker: P,
    watcher: Option<&crate::watcher::WatcherManager>,
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

    // 4. Start watching best-effort: watcher failure never fails add_root (TASK-1G decision m).
    if let Some(wm) = watcher {
        if let Ok(mut conn) = pool.acquire().await {
            if let Ok(Some(row)) =
                crate::database::roots::get_root(&mut conn, &outcome.root_id).await
            {
                let native_path = crate::filesystem::blob_to_native(&row.path_native);
                wm.ensure_watching(&outcome.root_id, &native_path);
            }
        }
    }

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

/// Plain async function implementing root relocation behind a pluggable picker closure (TASK-1F decision f).
pub async fn relocate_root_with_picker<P, Fut>(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    dialog_guard: &DialogGuard,
    root_id_str: &str,
    picker: P,
) -> Result<RelocateRootResultDto, IpcError>
where
    P: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<PathBuf>>,
{
    relocate_root_with_picker_and_describer(
        pool,
        ctx,
        dialog_guard,
        root_id_str,
        picker,
        crate::library::volume::describe_volume,
    )
    .await
}

/// Parameterized variant of `relocate_root_with_picker` allowing an injected volume describer in tests.
pub async fn relocate_root_with_picker_and_describer<P, Fut, D>(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    dialog_guard: &DialogGuard,
    root_id_str: &str,
    picker: P,
    describer: D,
) -> Result<RelocateRootResultDto, IpcError>
where
    P: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<PathBuf>>,
    D: Fn(&std::path::Path) -> Result<crate::domain::VolumeInfo, crate::domain::RootError>,
{
    relocate_root_with_deps(
        pool,
        ctx,
        dialog_guard,
        root_id_str,
        picker,
        describer,
        None,
    )
    .await
}

/// Full relocate helper accepting optional `WatcherManager` to start watching the relocated path.
pub async fn relocate_root_with_deps<P, Fut, D>(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    dialog_guard: &DialogGuard,
    root_id_str: &str,
    picker: P,
    describer: D,
    watcher: Option<&crate::watcher::WatcherManager>,
) -> Result<RelocateRootResultDto, IpcError>
where
    P: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<PathBuf>>,
    D: Fn(&std::path::Path) -> Result<crate::domain::VolumeInfo, crate::domain::RootError>,
{
    // 1. Guard against concurrent open dialogs (same DialogGuard as add_root)
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
            return Ok(RelocateRootResultDto {
                outcome: RelocateRootOutcomeKind::Cancelled,
                root: None,
            });
        }
    };

    // 3. Delegate to domain library::roots::relocate_root_with_deps
    let root_id = RootId::from(root_id_str.to_string());
    let mut summary =
        crate::library::roots::relocate_root_with_deps(pool, ctx, &root_id, &path, describer)
            .await
            .map_err(IpcError::from)?;

    if let Some(wm) = watcher {
        if let Ok(mut conn) = pool.acquire().await {
            if let Ok(Some(row)) = crate::database::roots::get_root(&mut conn, &root_id).await {
                let native_path = crate::filesystem::blob_to_native(&row.path_native);
                wm.ensure_watching(&root_id, &native_path);
            }
        }
        let (changes_detected, watching) = wm.status_for_root(&root_id);
        summary.changes_detected = changes_detected && summary.last_scan_at_ms.is_some();
        summary.watching = watching;
    }

    Ok(RelocateRootResultDto {
        outcome: RelocateRootOutcomeKind::Relocated,
        root: Some(summary),
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

/// Plain async function for executing verified root removal (Decision b, h, i).
pub async fn remove_root_impl(
    pool: &SqlitePool,
    root_id_str: &str,
    expected_files: i64,
) -> Result<(), IpcError> {
    remove_root_impl_with_watcher(pool, root_id_str, expected_files, None).await
}

/// Executes verified root removal and stops/cleans up any watcher for that root.
pub async fn remove_root_impl_with_watcher(
    pool: &SqlitePool,
    root_id_str: &str,
    expected_files: i64,
    watcher: Option<&crate::watcher::WatcherManager>,
) -> Result<(), IpcError> {
    let root_id = RootId::from(root_id_str.to_string());
    // Acquire ScanGuard to ensure no concurrent scan is active (Decision i)
    let _guard = crate::scanner::ScanGuard::acquire(&root_id)
        .map_err(|_| IpcError::new("root_busy", fixed_message_for_code("root_busy")))?;
    crate::library::roots::remove_root(pool, &root_id, expected_files)
        .await
        .map_err(IpcError::from)?;
    if let Some(wm) = watcher {
        wm.remove_root(&root_id);
    }
    Ok(())
}

//
// Tauri command wrappers
//

#[tauri::command]
pub async fn list_roots(
    pool: tauri::State<'_, SqlitePool>,
    in_flight: tauri::State<'_, ActiveReachableChecks>,
    manager: tauri::State<'_, Arc<crate::jobs::JobManager>>,
    watcher: tauri::State<'_, Arc<crate::watcher::WatcherManager>>,
) -> Result<Vec<RootSummary>, IpcError> {
    list_roots_with_runtime_state(
        &pool,
        &in_flight.0,
        |id| manager.active_job_for_root(id),
        Some(&watcher),
    )
    .await
}

#[tauri::command]
pub async fn add_root(
    pool: tauri::State<'_, SqlitePool>,
    ctx: tauri::State<'_, ProtectedContext>,
    dialog_guard: tauri::State<'_, DialogGuard>,
    watcher: tauri::State<'_, Arc<crate::watcher::WatcherManager>>,
    window: tauri::WebviewWindow,
    label: Option<String>,
) -> Result<AddRootResultDto, IpcError> {
    add_root_with_picker_and_watcher(
        &pool,
        &ctx,
        &dialog_guard,
        label,
        || async move {
            let handle = rfd::AsyncFileDialog::new()
                .set_parent(&window)
                .pick_folder()
                .await;
            handle.map(|h| h.path().to_path_buf())
        },
        Some(&watcher),
    )
    .await
}

#[tauri::command]
pub async fn relocate_root(
    pool: tauri::State<'_, SqlitePool>,
    ctx: tauri::State<'_, ProtectedContext>,
    dialog_guard: tauri::State<'_, DialogGuard>,
    watcher: tauri::State<'_, Arc<crate::watcher::WatcherManager>>,
    window: tauri::WebviewWindow,
    root_id: String,
) -> Result<RelocateRootResultDto, IpcError> {
    relocate_root_with_deps(
        &pool,
        &ctx,
        &dialog_guard,
        &root_id,
        || async move {
            let handle = rfd::AsyncFileDialog::new()
                .set_parent(&window)
                .pick_folder()
                .await;
            handle.map(|h| h.path().to_path_buf())
        },
        crate::library::volume::describe_volume,
        Some(&watcher),
    )
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
    watcher: tauri::State<'_, Arc<crate::watcher::WatcherManager>>,
    root_id: String,
    expected_files: i64,
) -> Result<(), IpcError> {
    remove_root_impl_with_watcher(&pool, &root_id, expected_files, Some(&watcher)).await
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
            last_scan_at_ms: Some(1732440000000),
            last_job: Some(crate::database::LastJobSummary {
                job_id: "job_0193498f3b4970008b8b000000000002".to_string(),
                status: "completed".to_string(),
                error_code: None,
                finished_at_ms: Some(1732440005000),
                trigger: "auto".to_string(),
            }),
            active_job_id: Some("job_0193498f3b4970008b8b000000000003".to_string()),
            changes_detected: true,
            watching: crate::domain::WatchingState::On,
        };

        let json = serde_json::to_string(&summary).expect("serialize RootSummary");
        let expected_json = r#"{"id":"root_0193498f3b4970008b8b000000000001","label":"My Photos","pathDisplay":"/Users/alice/Photos","volumeKind":"Internal","volumeLabel":"Macintosh HD","volumeCaseSensitive":false,"status":"Active","reachable":true,"fileCount":42,"folderCount":7,"lastScanAtMs":1732440000000,"lastJob":{"jobId":"job_0193498f3b4970008b8b000000000002","status":"completed","errorCode":null,"finishedAtMs":1732440005000,"trigger":"auto"},"activeJobId":"job_0193498f3b4970008b8b000000000003","changesDetected":true,"watching":"on"}"#;
        assert_eq!(json, expected_json);

        let summary_nulls = RootSummary {
            id: RootId::from("root_0193498f3b4970008b8b000000000001".to_string()),
            label: "My Photos".to_string(),
            path_display: "/Users/alice/Photos".to_string(),
            volume_kind: VolumeKind::Internal,
            volume_label: None,
            volume_case_sensitive: false,
            status: RootStatus::Active,
            reachable: true,
            file_count: 42,
            folder_count: 7,
            last_scan_at_ms: None,
            last_job: None,
            active_job_id: None,
            changes_detected: false,
            watching: crate::domain::WatchingState::Degraded,
        };

        let json_nulls =
            serde_json::to_string(&summary_nulls).expect("serialize RootSummary nulls");
        let expected_nulls = r#"{"id":"root_0193498f3b4970008b8b000000000001","label":"My Photos","pathDisplay":"/Users/alice/Photos","volumeKind":"Internal","volumeLabel":null,"volumeCaseSensitive":false,"status":"Active","reachable":true,"fileCount":42,"folderCount":7,"lastScanAtMs":null,"lastJob":null,"activeJobId":null,"changesDetected":false,"watching":"degraded"}"#;
        assert_eq!(json_nulls, expected_nulls);
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

    #[test]
    fn test_wire_format_relocate_root_result_matches_literal_json() {
        let cancelled = RelocateRootResultDto {
            outcome: RelocateRootOutcomeKind::Cancelled,
            root: None,
        };
        assert_eq!(
            serde_json::to_string(&cancelled).unwrap(),
            r#"{"outcome":"cancelled"}"#
        );

        let relocated = RelocateRootResultDto {
            outcome: RelocateRootOutcomeKind::Relocated,
            root: Some(RootSummary {
                id: RootId::from("root_01".to_string()),
                label: "Photos".to_string(),
                path_display: "/Users/alice/PhotosNew".to_string(),
                volume_kind: VolumeKind::Internal,
                volume_label: None,
                volume_case_sensitive: false,
                status: RootStatus::Active,
                reachable: true,
                file_count: 5,
                folder_count: 1,
                last_scan_at_ms: None,
                last_job: None,
                active_job_id: None,
                changes_detected: false,
                watching: crate::domain::WatchingState::Off,
            }),
        };
        assert_eq!(
            serde_json::to_string(&relocated).unwrap(),
            r#"{"outcome":"relocated","root":{"id":"root_01","label":"Photos","pathDisplay":"/Users/alice/PhotosNew","volumeKind":"Internal","volumeLabel":null,"volumeCaseSensitive":false,"status":"Active","reachable":true,"fileCount":5,"folderCount":1,"lastScanAtMs":null,"lastJob":null,"activeJobId":null,"changesDetected":false,"watching":"off"}}"#
        );
    }

    #[derive(Debug, PartialEq, sqlx::FromRow)]
    struct FileRowSnapshot {
        id: String,
        root_id: String,
        folder_id: String,
        name: String,
        extension: Option<String>,
        rel_path_display: String,
        rel_path_native: Vec<u8>,
        path_key: Vec<u8>,
        size_bytes: i64,
        modified_at_ms: i64,
        created_at_ms: Option<i64>,
        file_identity: Option<String>,
        status: String,
        scan_generation: Option<String>,
        last_verified_at_ms: Option<i64>,
        created_in_catalog_at_ms: i64,
        updated_at_ms: i64,
    }

    #[derive(Debug, PartialEq, sqlx::FromRow)]
    struct FolderRowSnapshot {
        id: String,
        root_id: String,
        parent_id: Option<String>,
        name: String,
        rel_path_display: String,
        rel_path_native: Vec<u8>,
        path_key: Vec<u8>,
        status: String,
        modified_at_ms: Option<i64>,
        last_seen_generation: Option<String>,
    }

    async fn snapshot_files_and_folders(
        pool: &SqlitePool,
        root_id: &str,
    ) -> (Vec<FileRowSnapshot>, Vec<FolderRowSnapshot>) {
        let files: Vec<FileRowSnapshot> = sqlx::query_as(
            "SELECT id, root_id, folder_id, name, extension, rel_path_display, rel_path_native,
                    path_key, size_bytes, modified_at_ms, created_at_ms, file_identity, status,
                    scan_generation, last_verified_at_ms, created_in_catalog_at_ms, updated_at_ms
             FROM files WHERE root_id = ? ORDER BY id ASC;",
        )
        .bind(root_id)
        .fetch_all(pool)
        .await
        .unwrap();

        let folders: Vec<FolderRowSnapshot> = sqlx::query_as(
            "SELECT id, root_id, parent_id, name, rel_path_display, rel_path_native,
                    path_key, status, modified_at_ms, last_seen_generation
             FROM folders WHERE root_id = ? ORDER BY id ASC;",
        )
        .bind(root_id)
        .fetch_all(pool)
        .await
        .unwrap();

        (files, folders)
    }

    #[tokio::test]
    async fn offline_round_trip_writes_no_file_or_folder_rows() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let root_dir = temp.path().join("library_round_trip");
        let sub_dir = root_dir.join("2026");
        std::fs::create_dir_all(&sub_dir).expect("create dirs");
        std::fs::write(root_dir.join("p1.jpg"), b"jpeg1").unwrap();
        std::fs::write(sub_dir.join("p2.jpg"), b"jpeg2").unwrap();

        let added = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(root_dir.clone())
        })
        .await
        .unwrap();
        let root_id_str = added.root_id.unwrap();
        let root_id = RootId::from(root_id_str.clone());

        crate::scanner::scan_root(
            &pool,
            &root_id,
            crate::scanner::ScanOptions::default(),
            crate::scanner::ScanCancel::new(),
            |_| {},
        )
        .await
        .unwrap();

        // Delete p2.jpg and rescan so we also have a 'missing' row in the snapshot
        std::fs::remove_file(sub_dir.join("p2.jpg")).unwrap();
        crate::scanner::scan_root(
            &pool,
            &root_id,
            crate::scanner::ScanOptions::default(),
            crate::scanner::ScanCancel::new(),
            |_| {},
        )
        .await
        .unwrap();

        let baseline_snap = snapshot_files_and_folders(&pool, &root_id_str).await;
        assert_eq!(baseline_snap.0.len(), 2);
        assert_eq!(baseline_snap.1.len(), 2);

        let aside_dir = temp.path().join("library_round_trip_aside");
        let in_flight = Arc::new(Mutex::new(HashSet::new()));

        // 1. Go offline
        std::fs::rename(&root_dir, &aside_dir).unwrap();
        let list1 = list_roots_impl(&pool, &in_flight).await.unwrap();
        assert_eq!(list1[0].status, RootStatus::Offline);
        assert!(!list1[0].reachable);
        assert_eq!(
            snapshot_files_and_folders(&pool, &root_id_str).await,
            baseline_snap
        );

        // 2. Come back online
        std::fs::rename(&aside_dir, &root_dir).unwrap();
        let list2 = list_roots_impl(&pool, &in_flight).await.unwrap();
        assert_eq!(list2[0].status, RootStatus::Active);
        assert!(list2[0].reachable);
        assert_eq!(
            snapshot_files_and_folders(&pool, &root_id_str).await,
            baseline_snap
        );

        // 3. Go offline again
        std::fs::rename(&root_dir, &aside_dir).unwrap();
        let list3 = list_roots_impl(&pool, &in_flight).await.unwrap();
        assert_eq!(list3[0].status, RootStatus::Offline);
        assert!(!list3[0].reachable);
        assert_eq!(
            snapshot_files_and_folders(&pool, &root_id_str).await,
            baseline_snap
        );
    }

    #[tokio::test]
    async fn relocate_root_success_updates_only_paths_status_and_volume_seen() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let old_dir = temp.path().join("photos_old");
        let sub_dir = old_dir.join("trip");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(old_dir.join("a.jpg"), b"a").unwrap();
        std::fs::write(sub_dir.join("b.jpg"), b"b").unwrap();

        let add_res = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(old_dir.clone())
        })
        .await
        .unwrap();
        let root_id_str = add_res.root_id.unwrap();
        let root_id = RootId::from(root_id_str.clone());

        crate::scanner::scan_root(
            &pool,
            &root_id,
            crate::scanner::ScanOptions::default(),
            crate::scanner::ScanCancel::new(),
            |_| {},
        )
        .await
        .unwrap();

        // Move folder to new path and run list_roots so status becomes Offline
        let new_dir = temp.path().join("photos_new");
        std::fs::rename(&old_dir, &new_dir).unwrap();
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let roots_off = list_roots_impl(&pool, &in_flight).await.unwrap();
        assert_eq!(roots_off[0].status, RootStatus::Offline);

        let before_snap = snapshot_files_and_folders(&pool, &root_id_str).await;
        let mut conn = pool.acquire().await.unwrap();
        let before_root = crate::database::roots::get_root(&mut conn, &root_id)
            .await
            .unwrap()
            .unwrap();
        let vol_id = before_root.volume_id.clone().unwrap();
        let before_vol = crate::database::volumes::get_volume(&mut conn, &vol_id)
            .await
            .unwrap()
            .unwrap();
        drop(conn);

        // Relocate with matching UUID (injected deterministic UUID so test passes on Linux too)
        let uuid_describer =
            |_: &Path| -> Result<crate::domain::VolumeInfo, crate::domain::RootError> {
                Ok(crate::domain::VolumeInfo {
                    kind: VolumeKind::Internal,
                    label: Some("Main".to_string()),
                    fs_uuid: before_vol
                        .fs_uuid
                        .clone()
                        .or(Some("uuid-fallback".to_string())),
                    fs_type: Some("apfs".to_string()),
                })
            };
        if before_vol.fs_uuid.is_none() {
            sqlx::query("UPDATE volumes SET fs_uuid = 'uuid-fallback' WHERE id = ?;")
                .bind(vol_id.as_str())
                .execute(&pool)
                .await
                .unwrap();
        }

        let reloc_res = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(new_dir.clone()) },
            uuid_describer,
        )
        .await
        .expect("relocate should succeed");

        assert_eq!(reloc_res.outcome, RelocateRootOutcomeKind::Relocated);
        let summary = reloc_res.root.expect("summary present");
        assert_eq!(summary.status, RootStatus::Active);
        assert!(summary.reachable);
        assert_eq!(summary.file_count, 2);

        // Verify files and folders are byte-for-byte identical
        assert_eq!(
            snapshot_files_and_folders(&pool, &root_id_str).await,
            before_snap
        );

        let mut conn = pool.acquire().await.unwrap();
        let after_root = crate::database::roots::get_root(&mut conn, &root_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after_root.id, before_root.id);
        assert_eq!(after_root.volume_id, before_root.volume_id);
        assert_eq!(after_root.label, before_root.label);
        assert_eq!(after_root.follow_symlinks, before_root.follow_symlinks);
        assert_eq!(after_root.include_hidden, before_root.include_hidden);
        assert_eq!(after_root.created_at_ms, before_root.created_at_ms);
        assert_eq!(after_root.last_scan_at_ms, before_root.last_scan_at_ms);
        assert_eq!(
            after_root.last_scan_generation,
            before_root.last_scan_generation
        );
        assert_ne!(after_root.path_display, before_root.path_display);
        assert_eq!(after_root.status, RootStatus::Active);

        let after_vol = crate::database::volumes::get_volume(&mut conn, &vol_id)
            .await
            .unwrap()
            .unwrap();
        assert!(after_vol.last_seen_at_ms >= before_vol.last_seen_at_ms);
    }

    #[tokio::test]
    async fn relocate_root_refusals_and_cancelled_dialog() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let dir1 = temp.path().join("Root1");
        let dir2 = temp.path().join("OtherRoot");
        std::fs::create_dir_all(&dir1).unwrap();
        std::fs::create_dir_all(&dir2).unwrap();
        for i in 1..=4 {
            std::fs::write(dir1.join(format!("p{i}.jpg")), b"img").unwrap();
        }

        let out1 = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(dir1.clone())
        })
        .await
        .unwrap();
        let root_id_str = out1.root_id.unwrap();
        let root_id = RootId::from(root_id_str.clone());

        let _out2 = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(dir2.clone())
        })
        .await
        .unwrap();

        crate::scanner::scan_root(
            &pool,
            &root_id,
            crate::scanner::ScanOptions::default(),
            crate::scanner::ScanCancel::new(),
            |_| {},
        )
        .await
        .unwrap();

        // Ensure volume has deterministic fs_uuid across OSes
        let mut conn = pool.acquire().await.unwrap();
        let r1 = crate::database::roots::get_root(&mut conn, &root_id)
            .await
            .unwrap()
            .unwrap();
        let vol_id = r1.volume_id.clone().unwrap();
        sqlx::query("UPDATE volumes SET fs_uuid = 'uuid-reloc-test' WHERE id = ?;")
            .bind(vol_id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        drop(conn);

        let same_vol = |_: &Path| -> Result<crate::domain::VolumeInfo, crate::domain::RootError> {
            Ok(crate::domain::VolumeInfo {
                kind: VolumeKind::Internal,
                label: None,
                fs_uuid: Some("uuid-reloc-test".to_string()),
                fs_type: Some("apfs".to_string()),
            })
        };

        // 1. not_found
        let err_nf = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            "missing_root_id",
            || async { Some(dir1.clone()) },
            same_vol,
        )
        .await
        .unwrap_err();
        assert_eq!(err_nf.code, "not_found");

        // 2. root_not_offline (root is Active)
        let err_active = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(dir1.clone()) },
            same_vol,
        )
        .await
        .unwrap_err();
        assert_eq!(err_active.code, "root_not_offline");

        // Transition root to Offline by moving dir1 away and calling list_roots_impl
        let wrong_dir = temp.path().join("WrongFolder");
        std::fs::create_dir_all(&wrong_dir).unwrap();
        let aside_dir = temp.path().join("Root1Moved");
        std::fs::rename(&dir1, &aside_dir).unwrap();
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let roots = list_roots_impl(&pool, &in_flight).await.unwrap();
        assert_eq!(roots[0].status, RootStatus::Offline);

        // 3. Cancelled dialog changes nothing
        let cancel_res = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { None },
            same_vol,
        )
        .await
        .unwrap();
        assert_eq!(cancel_res.outcome, RelocateRootOutcomeKind::Cancelled);
        assert!(cancel_res.root.is_none());

        // 4. root_busy when ScanGuard is held
        let guard = crate::scanner::ScanGuard::acquire(&root_id).unwrap();
        let err_busy = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(aside_dir.clone()) },
            same_vol,
        )
        .await
        .unwrap_err();
        assert_eq!(err_busy.code, "root_busy");
        drop(guard);

        // 5. root_protected
        let err_prot = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(app_data.clone()) },
            same_vol,
        )
        .await
        .unwrap_err();
        assert_eq!(err_prot.code, "root_protected");

        // 6. root_overlaps with another root
        let err_overlap = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(dir2.clone()) },
            same_vol,
        )
        .await
        .unwrap_err();
        assert_eq!(err_overlap.code, "root_overlaps");

        // 7. relocate_volume_mismatch: different uuid, NULL live uuid, NULL stored uuid
        let err_vol_diff = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(aside_dir.clone()) },
            |_| {
                Ok(crate::domain::VolumeInfo {
                    kind: VolumeKind::Internal,
                    label: None,
                    fs_uuid: Some("uuid-different".to_string()),
                    fs_type: None,
                })
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err_vol_diff.code, "relocate_volume_mismatch");

        let err_vol_null_live = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(aside_dir.clone()) },
            |_| {
                Ok(crate::domain::VolumeInfo {
                    kind: VolumeKind::Network,
                    label: None,
                    fs_uuid: None,
                    fs_type: None,
                })
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err_vol_null_live.code, "relocate_volume_mismatch");

        sqlx::query("UPDATE volumes SET fs_uuid = NULL WHERE id = ?;")
            .bind(vol_id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        let err_vol_null_stored = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(aside_dir.clone()) },
            same_vol,
        )
        .await
        .unwrap_err();
        assert_eq!(err_vol_null_stored.code, "relocate_volume_mismatch");

        // Restore stored fs_uuid
        sqlx::query("UPDATE volumes SET fs_uuid = 'uuid-reloc-test' WHERE id = ?;")
            .bind(vol_id.as_str())
            .execute(&pool)
            .await
            .unwrap();

        // 8. relocate_content_mismatch: below half (1 of 4) fails; exactly half (2 of 4) passes!
        std::fs::write(wrong_dir.join("p1.jpg"), b"img").unwrap();
        let err_content = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(wrong_dir.clone()) },
            same_vol,
        )
        .await
        .unwrap_err();
        assert_eq!(err_content.code, "relocate_content_mismatch");

        std::fs::write(wrong_dir.join("p2.jpg"), b"img").unwrap();
        let ok_half = relocate_root_with_picker_and_describer(
            &pool,
            &ctx,
            &dialog_guard,
            &root_id_str,
            || async { Some(wrong_dir.clone()) },
            same_vol,
        )
        .await
        .expect("exactly half matching files must pass");
        assert_eq!(ok_half.outcome, RelocateRootOutcomeKind::Relocated);
        assert_eq!(ok_half.root.unwrap().status, RootStatus::Active);
    }

    #[tokio::test]
    async fn remove_root_works_when_offline() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");
        let app_data = temp.path().join("app_data");
        std::fs::create_dir_all(&app_data).expect("create app_data");
        let ctx = test_context(&app_data);
        let dialog_guard = DialogGuard::default();

        let dir = temp.path().join("OfflineRemove");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jpg"), b"a").unwrap();
        std::fs::write(dir.join("b.jpg"), b"b").unwrap();

        let added = add_root_with_picker(&pool, &ctx, &dialog_guard, None, || async {
            Some(dir.clone())
        })
        .await
        .unwrap();
        let root_id_str = added.root_id.unwrap();
        let root_id = RootId::from(root_id_str.clone());

        crate::scanner::scan_root(
            &pool,
            &root_id,
            crate::scanner::ScanOptions::default(),
            crate::scanner::ScanCancel::new(),
            |_| {},
        )
        .await
        .unwrap();

        // Remove folder from disk and transition root to Offline
        std::fs::remove_dir_all(&dir).unwrap();
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let roots = list_roots_impl(&pool, &in_flight).await.unwrap();
        assert_eq!(roots[0].status, RootStatus::Offline);

        let preview = preview_remove_root_impl(&pool, &root_id_str).await.unwrap();
        assert_eq!(preview.file_count, 2);
        assert_eq!(preview.asset_count, 2);

        remove_root_impl(&pool, &root_id_str, preview.file_count)
            .await
            .expect("removing offline root must succeed without touching disk");

        let roots_after = list_roots_impl(&pool, &in_flight).await.unwrap();
        assert!(roots_after.is_empty());
    }
}
