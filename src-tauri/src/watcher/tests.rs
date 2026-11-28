use super::*;
use crate::commands::roots::{
    add_root_with_picker_and_watcher, list_roots_with_runtime_state, relocate_root_with_deps,
    remove_root_impl_with_watcher, AddRootOutcomeKind, DialogGuard,
};
use crate::commands::settings::{get_auto_scan_impl, set_auto_scan_impl};
use crate::database::models::{FileRow, FolderRow, RootRow, VolumeRow};
use crate::database::{self, init_pool, NewJobParams};
use crate::domain::{
    FileId, FileStatus, FolderId, FolderStatus, RootId, RootStatus, VolumeId, VolumeKind,
    WatchingState,
};
use crate::jobs::{JobManager, ScanTrigger};
use crate::library::canonical::extract_components;
use crate::library::protected::ProtectedContext;
use crate::watcher::backend::{WatchBackend, WatchCallback, WatchEvent};
use crate::watcher::debounce::{
    classify_notify_event, fold_event_into_inbox, RootDebounceState, RootInbox, DEBOUNCE_MAX_WAIT,
    DEBOUNCE_QUIET_PERIOD,
};
use crate::watcher::scheduler::{
    decide_next_auto_scan, AutoScanReason, SchedulerRootInput, AUTO_SCAN_MIN_GAP_MS,
    RECONCILE_MAX_AGE_MS,
};
use notify::event::{CreateKind, Flag, ModifyKind, RemoveKind, RenameMode};
use notify::{Event, EventKind};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn test_context(app_data_path: &Path) -> ProtectedContext {
    let canon =
        std::fs::canonicalize(app_data_path).unwrap_or_else(|_| app_data_path.to_path_buf());
    let app_data_comps = crate::library::overlap::normalize_components(&extract_components(&canon));
    let app_data_refs: Vec<&str> = app_data_comps.iter().map(|s| s.as_str()).collect();
    ProtectedContext::synthetic_unix(
        &app_data_refs,
        &["users", "alice"],
        &["users"],
        &["volumes"],
        &[&["system"]],
    )
}

#[derive(Default)]
struct FakeWatchBackend {
    active: Mutex<HashMap<RootId, (PathBuf, WatchCallback)>>,
    starts: Mutex<Vec<RootId>>,
    stops: Mutex<Vec<RootId>>,
}

impl FakeWatchBackend {
    fn is_watching(&self, root_id: &RootId) -> bool {
        self.active.lock().unwrap().contains_key(root_id)
    }

    fn emit(&self, root_id: &RootId, event: WatchEvent) {
        let cb = self
            .active
            .lock()
            .unwrap()
            .get(root_id)
            .map(|(_, cb)| Arc::clone(cb));
        if let Some(callback) = cb {
            callback(event);
        }
    }

    fn start_count(&self, root_id: &RootId) -> usize {
        self.starts
            .lock()
            .unwrap()
            .iter()
            .filter(|id| *id == root_id)
            .count()
    }

    fn stop_count(&self, root_id: &RootId) -> usize {
        self.stops
            .lock()
            .unwrap()
            .iter()
            .filter(|id| *id == root_id)
            .count()
    }
}

impl WatchBackend for FakeWatchBackend {
    fn start_watch(
        &self,
        root_id: &RootId,
        path: &Path,
        callback: WatchCallback,
    ) -> Result<(), String> {
        self.starts.lock().unwrap().push(root_id.clone());
        self.active
            .lock()
            .unwrap()
            .insert(root_id.clone(), (path.to_path_buf(), callback));
        Ok(())
    }

    fn stop_watch(&self, root_id: &RootId) {
        self.stops.lock().unwrap().push(root_id.clone());
        self.active.lock().unwrap().remove(root_id);
    }
}

#[test]
fn debouncer_folds_a_burst_into_one_mark_after_the_quiet_period() {
    let mut state = RootDebounceState::default();
    let t0 = Instant::now();

    assert!(!state.poll_ready(t0));

    // Burst of events within 1.5 s
    state.record_event(t0);
    state.record_event(t0 + Duration::from_millis(400));
    state.record_event(t0 + Duration::from_millis(900));
    state.record_event(t0 + Duration::from_millis(1500));

    // Before 2 s quiet after the last event (1500 + 2000 = 3500 ms), nothing fires
    assert!(!state.poll_ready(t0 + Duration::from_millis(3499)));

    // At 2 s quiet after the last event, exactly one mark fires
    assert!(state.poll_ready(t0 + Duration::from_millis(3500)));

    // Subsequent polls without new events do not fire again
    assert!(!state.poll_ready(t0 + Duration::from_secs(10)));
}

#[test]
fn debouncer_max_wait_forces_a_mark_during_a_continuous_stream() {
    let mut state = RootDebounceState::default();
    let t0 = Instant::now();

    // Continuous stream every 1 s (< 2 s quiet period) for 29 s: nothing fires yet
    for sec in 0..30 {
        let t = t0 + Duration::from_secs(sec);
        state.record_event(t);
        assert!(
            !state.poll_ready(t),
            "continuous stream must not fire before 30 s max wait (at {sec} s)"
        );
    }

    // At t0 + 30 s (even with another event at 30 s so quiet period is 0 s), max wait forces a mark
    let t30 = t0 + DEBOUNCE_MAX_WAIT;
    state.record_event(t30);
    assert!(
        state.poll_ready(t30),
        "max wait (30 s) must force one mark during a continuous stream"
    );
    assert!(!state.is_pending());
}

#[test]
fn event_flood_is_folded_into_one_flag() {
    let mut inbox = RootInbox::default();
    let base_size = std::mem::size_of_val(&inbox);
    let t0 = Instant::now();

    for i in 0..1_000_000u64 {
        let offset = Duration::from_millis(i % 1500);
        fold_event_into_inbox(&mut inbox, WatchEvent::Changed, t0 + offset);
    }

    assert_eq!(
        std::mem::size_of_val(&inbox),
        base_size,
        "folding 1,000,000 events must keep O(1) memory footprint"
    );
    assert!(inbox.debounce.is_pending());
    assert!(!inbox.debounce.poll_ready(t0 + Duration::from_millis(2000)));
    assert!(inbox
        .debounce
        .poll_ready(t0 + Duration::from_millis(1499) + DEBOUNCE_QUIET_PERIOD));
    assert!(!inbox.debounce.is_pending());
}

#[test]
fn ignored_names_do_not_mark_changes() {
    let root = PathBuf::from("/tmp/photos_root");

    let skipped_paths = [
        root.join(".DS_Store"),
        root.join("sub/.ds_store"),
        root.join("Thumbs.db"),
        root.join("nested/@eaDir/thumb.jpg"),
        root.join(".Trashes/501/old.jpg"),
        root.join("System Volume Information/IndexerVolumeGuid"),
        root.join("$RECYCLE.BIN/S-1-5-21/pic.jpg"),
        root.join("Photos Library.photoslibrary/database/Photos.sqlite"),
        root.join("archive/Family.PHOTOSLIBRARY/originals/0/001.heic"),
    ];

    let kinds = [
        EventKind::Create(CreateKind::File),
        EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content)),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
        EventKind::Remove(RemoveKind::File),
        EventKind::Other,
        EventKind::Any,
    ];

    for p in &skipped_paths {
        for kind in &kinds {
            let ev = Event {
                kind: *kind,
                paths: vec![p.clone()],
                attrs: Default::default(),
            };
            assert!(
                !classify_notify_event(Some(&root), &ev),
                "skipped path {:?} with kind {:?} must not mark changes",
                p,
                kind
            );
        }
    }

    // Real photo paths mark changes across create, modify, rename, remove, other
    let real_photo = root.join("2026/IMG_0001.JPG");
    for kind in &kinds {
        let ev = Event {
            kind: *kind,
            paths: vec![real_photo.clone()],
            attrs: Default::default(),
        };
        assert!(
            classify_notify_event(Some(&root), &ev),
            "real photo path with kind {:?} must mark changes",
            kind
        );
    }

    // Read-only Access events without rescan do not mark changes
    let access_ev = Event {
        kind: EventKind::Access(notify::event::AccessKind::Read),
        paths: vec![real_photo.clone()],
        attrs: Default::default(),
    };
    assert!(!classify_notify_event(Some(&root), &access_ev));

    // Overflow / Rescan flag always marks changes even on access events or skipped paths
    let overflow_ev = Event {
        kind: EventKind::Access(notify::event::AccessKind::Read),
        paths: vec![root.join(".DS_Store")],
        attrs: Default::default(),
    }
    .set_flag(Flag::Rescan);
    assert!(classify_notify_event(Some(&root), &overflow_ev));
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct FileSnap {
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
struct FolderSnap {
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

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct RootSnap {
    id: String,
    volume_id: Option<String>,
    label: String,
    path_display: String,
    path_native: Vec<u8>,
    status: String,
    follow_symlinks: i32,
    include_hidden: i32,
    created_at_ms: i64,
    updated_at_ms: i64,
    last_scan_at_ms: Option<i64>,
    last_scan_generation: Option<String>,
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct JobSnap {
    id: String,
    root_id: Option<String>,
    job_type: String,
    status: String,
    priority: i32,
    payload_json: String,
    progress_done: i64,
    progress_total: Option<i64>,
    current_item: Option<String>,
    warning_count: i64,
    error_count: i64,
    created_at_ms: i64,
    started_at_ms: Option<i64>,
    updated_at_ms: i64,
    finished_at_ms: Option<i64>,
    error_code: Option<String>,
    error_message: Option<String>,
}

#[tokio::test]
async fn watcher_event_writes_no_catalogue_rows() {
    let temp = tempdir().expect("tempdir");
    let db_path = temp.path().join("cat.db");
    let root_dir = temp.path().join("photos");
    std::fs::create_dir_all(&root_dir).unwrap();

    let pool = init_pool(&db_path).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();

    let vol_id = VolumeId::new();
    database::volumes::insert_volume(
        &mut conn,
        &VolumeRow {
            id: vol_id.clone(),
            kind: VolumeKind::Internal,
            label: Some("Disk".into()),
            fs_uuid: Some("vol-1".into()),
            fs_type: Some("apfs".into()),
            case_sensitive: false,
            last_seen_at_ms: Some(1000),
        },
    )
    .await
    .unwrap();

    let root_id = RootId::new();
    database::roots::insert_root(
        &mut conn,
        &RootRow {
            id: root_id.clone(),
            volume_id: Some(vol_id),
            label: "Photos".into(),
            path_display: root_dir.to_string_lossy().to_string(),
            path_native: crate::filesystem::native_to_blob(&root_dir),
            status: RootStatus::Active,
            follow_symlinks: false,
            include_hidden: false,
            created_at_ms: 1000,
            updated_at_ms: 1000,
            last_scan_at_ms: Some(2000),
            last_scan_generation: Some("gen-1".into()),
        },
    )
    .await
    .unwrap();

    let root_folder_id = FolderId::new();
    database::folders::insert_folder(
        &mut conn,
        &FolderRow {
            id: root_folder_id.clone(),
            root_id: root_id.clone(),
            parent_id: None,
            name: String::new(),
            rel_path_display: String::new(),
            rel_path_native: Vec::new(),
            path_key: Vec::new(),
            status: FolderStatus::Present,
            modified_at_ms: Some(1000),
            last_seen_generation: Some("gen-1".into()),
        },
    )
    .await
    .unwrap();

    database::files::insert_file(
        &mut conn,
        &FileRow {
            id: FileId::new(),
            root_id: root_id.clone(),
            folder_id: root_folder_id,
            name: "img.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "img.jpg".into(),
            rel_path_native: b"img.jpg".to_vec(),
            path_key: b"img.jpg".to_vec(),
            size_bytes: 123,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: Some("gen-1".into()),
            last_verified_at_ms: Some(2000),
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        },
    )
    .await
    .unwrap();

    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: "job_1",
            root_id: Some(root_id.as_str()),
            job_type: "scan",
            status: "completed",
            priority: 50,
            payload_json: "{}",
            now_ms: 2000,
        },
    )
    .await
    .unwrap();

    let files_before: Vec<FileSnap> = sqlx::query_as("SELECT * FROM files ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    let folders_before: Vec<FolderSnap> = sqlx::query_as("SELECT * FROM folders ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    let roots_before: Vec<RootSnap> = sqlx::query_as("SELECT * FROM library_roots ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    let jobs_before: Vec<JobSnap> = sqlx::query_as("SELECT * FROM jobs ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();

    let backend = Arc::new(FakeWatchBackend::default());
    let watcher_manager =
        WatcherManager::with_backend(Arc::clone(&backend) as Arc<dyn WatchBackend>);
    watcher_manager.ensure_watching(&root_id, &root_dir);
    backend.emit(&root_id, WatchEvent::Changed);
    backend.emit(&root_id, WatchEvent::Error);
    watcher_manager.flush_debouncers(Instant::now() + Duration::from_secs(3));

    let (changes_detected, watching) = watcher_manager.status_for_root(&root_id);
    assert!(changes_detected);
    assert_eq!(watching, WatchingState::Degraded);

    let files_after: Vec<FileSnap> = sqlx::query_as("SELECT * FROM files ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    let folders_after: Vec<FolderSnap> = sqlx::query_as("SELECT * FROM folders ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    let roots_after: Vec<RootSnap> = sqlx::query_as("SELECT * FROM library_roots ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    let jobs_after: Vec<JobSnap> = sqlx::query_as("SELECT * FROM jobs ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .unwrap();

    assert_eq!(files_before, files_after);
    assert_eq!(folders_before, folders_after);
    assert_eq!(roots_before, roots_after);
    assert_eq!(jobs_before, jobs_after);
}

fn make_sched_root(
    id: &str,
    status: RootStatus,
    changes_detected: bool,
    changes_detected_at: Option<Instant>,
    has_active_job: bool,
    last_scan_ended_at_ms: Option<i64>,
    last_completed_scan_at_ms: Option<i64>,
) -> SchedulerRootInput {
    SchedulerRootInput {
        id: RootId::from(id.to_string()),
        status,
        changes_detected,
        changes_detected_at,
        has_active_job,
        last_scan_ended_at_ms,
        last_completed_scan_at_ms,
    }
}

#[test]
fn scheduler_does_nothing_when_auto_scan_is_off() {
    let now_inst = Instant::now();
    let now_ms = 10_000_000_000i64;
    let roots = vec![make_sched_root(
        "r1",
        RootStatus::Active,
        true,
        Some(now_inst - Duration::from_secs(10)),
        false,
        Some(now_ms - 120_000),
        Some(now_ms - RECONCILE_MAX_AGE_MS - 5000),
    )];

    assert_eq!(
        decide_next_auto_scan(false, false, &roots, now_inst, now_ms),
        None
    );
}

#[test]
fn scheduler_skips_busy_offline_and_paused_roots() {
    let now_inst = Instant::now();
    let now_ms = 10_000_000_000i64;
    let roots = vec![
        make_sched_root(
            "busy",
            RootStatus::Active,
            true,
            Some(now_inst - Duration::from_secs(30)),
            true,
            None,
            None,
        ),
        make_sched_root(
            "offline",
            RootStatus::Offline,
            true,
            Some(now_inst - Duration::from_secs(20)),
            false,
            None,
            Some(now_ms - RECONCILE_MAX_AGE_MS - 10_000),
        ),
        make_sched_root(
            "paused",
            RootStatus::Paused,
            true,
            Some(now_inst - Duration::from_secs(10)),
            false,
            None,
            Some(now_ms - RECONCILE_MAX_AGE_MS - 10_000),
        ),
        make_sched_root(
            "removed",
            RootStatus::Removed,
            true,
            Some(now_inst - Duration::from_secs(5)),
            false,
            None,
            None,
        ),
    ];

    assert_eq!(
        decide_next_auto_scan(true, false, &roots, now_inst, now_ms),
        None
    );
}

#[test]
fn scheduler_respects_the_60s_gap() {
    let now_inst = Instant::now();
    let now_ms = 10_000_000_000i64;

    // Last scan ended 59.999 s ago -> must wait
    let too_soon = vec![make_sched_root(
        "r1",
        RootStatus::Active,
        true,
        Some(now_inst),
        false,
        Some(now_ms - (AUTO_SCAN_MIN_GAP_MS - 1)),
        Some(now_ms - (AUTO_SCAN_MIN_GAP_MS - 1)),
    )];
    assert_eq!(
        decide_next_auto_scan(true, false, &too_soon, now_inst, now_ms),
        None
    );

    // Last scan ended exactly 60 s ago -> eligible
    let ready = vec![make_sched_root(
        "r1",
        RootStatus::Active,
        true,
        Some(now_inst),
        false,
        Some(now_ms - AUTO_SCAN_MIN_GAP_MS),
        Some(now_ms - AUTO_SCAN_MIN_GAP_MS),
    )];
    let decision = decide_next_auto_scan(true, false, &ready, now_inst, now_ms).unwrap();
    assert_eq!(decision.root_id.as_str(), "r1");
    assert_eq!(decision.reason, AutoScanReason::ChangesDetected);
}

#[test]
fn scheduler_starts_one_scan_at_a_time_oldest_change_first() {
    let now_inst = Instant::now();
    let now_ms = 10_000_000_000i64;
    let roots = vec![
        make_sched_root(
            "newer",
            RootStatus::Active,
            true,
            Some(now_inst - Duration::from_secs(5)),
            false,
            Some(now_ms - 120_000),
            Some(now_ms - 120_000),
        ),
        make_sched_root(
            "oldest",
            RootStatus::Active,
            true,
            Some(now_inst - Duration::from_secs(20)),
            false,
            Some(now_ms - 120_000),
            Some(now_ms - 120_000),
        ),
        make_sched_root(
            "middle",
            RootStatus::Active,
            true,
            Some(now_inst - Duration::from_secs(10)),
            false,
            Some(now_ms - 120_000),
            Some(now_ms - 120_000),
        ),
    ];

    // When an automatic scan is already running app-wide, no second auto scan starts
    assert_eq!(
        decide_next_auto_scan(true, true, &roots, now_inst, now_ms),
        None
    );

    // Otherwise returns the single oldest-changed root
    let decision = decide_next_auto_scan(true, false, &roots, now_inst, now_ms).unwrap();
    assert_eq!(decision.root_id.as_str(), "oldest");
    assert_eq!(decision.reason, AutoScanReason::ChangesDetected);
}

#[test]
fn stale_root_is_due_after_24h() {
    let now_inst = Instant::now();
    let now_ms = 10_000_000_000i64;

    // Exactly 24h ago (not > 24h) -> not due
    let boundary = vec![make_sched_root(
        "r1",
        RootStatus::Active,
        false,
        None,
        false,
        Some(now_ms - RECONCILE_MAX_AGE_MS),
        Some(now_ms - RECONCILE_MAX_AGE_MS),
    )];
    assert_eq!(
        decide_next_auto_scan(true, false, &boundary, now_inst, now_ms),
        None
    );

    // 24h + 1 ms ago -> due for reconciliation with no watcher event
    let stale = vec![make_sched_root(
        "r1",
        RootStatus::Active,
        false,
        None,
        false,
        Some(now_ms - RECONCILE_MAX_AGE_MS - 1),
        Some(now_ms - RECONCILE_MAX_AGE_MS - 1),
    )];
    let decision = decide_next_auto_scan(true, false, &stale, now_inst, now_ms).unwrap();
    assert_eq!(decision.root_id.as_str(), "r1");
    assert_eq!(decision.reason, AutoScanReason::Reconciliation24h);
}

#[tokio::test]
async fn registry_stops_the_watcher_when_the_root_goes_offline_and_starts_it_when_active() {
    let temp = tempdir().expect("tempdir");
    let app_data = temp.path().join("appdata");
    let root_dir = temp.path().join("photos");
    std::fs::create_dir_all(&app_data).unwrap();
    std::fs::create_dir_all(&root_dir).unwrap();

    let pool = init_pool(&app_data.join("cat.db")).await.unwrap();
    let ctx = test_context(&app_data);
    let dialog_guard = DialogGuard::default();
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    let backend = Arc::new(FakeWatchBackend::default());
    let watcher = Arc::new(WatcherManager::with_backend(
        Arc::clone(&backend) as Arc<dyn WatchBackend>
    ));

    let root_clone = root_dir.clone();
    let add_res = add_root_with_picker_and_watcher(
        &pool,
        &ctx,
        &dialog_guard,
        None,
        || async move { Some(root_clone) },
        Some(&watcher),
    )
    .await
    .unwrap();
    let root_id = RootId::from(add_res.root_id.unwrap());
    assert!(backend.is_watching(&root_id));

    // Delete root folder -> list_roots transitions root Active -> Offline and stops watcher
    std::fs::remove_dir_all(&root_dir).unwrap();
    let list_offline = list_roots_with_runtime_state(&pool, &in_flight, |_| None, Some(&watcher))
        .await
        .unwrap();
    assert_eq!(list_offline[0].status, RootStatus::Offline);
    assert_eq!(list_offline[0].watching, WatchingState::Off);
    assert!(!backend.is_watching(&root_id));

    // Recreate folder -> list_roots transitions Offline -> Active and restarts watcher
    std::fs::create_dir_all(&root_dir).unwrap();
    let list_active = list_roots_with_runtime_state(&pool, &in_flight, |_| None, Some(&watcher))
        .await
        .unwrap();
    assert_eq!(list_active[0].status, RootStatus::Active);
    assert_eq!(list_active[0].watching, WatchingState::On);
    assert!(backend.is_watching(&root_id));

    // Paused root -> watcher stopped
    {
        let mut conn = pool.acquire().await.unwrap();
        database::roots::set_root_status(&mut conn, &root_id, RootStatus::Paused, 5000)
            .await
            .unwrap();
    }
    let list_paused = list_roots_with_runtime_state(&pool, &in_flight, |_| None, Some(&watcher))
        .await
        .unwrap();
    assert_eq!(list_paused[0].status, RootStatus::Paused);
    assert_eq!(list_paused[0].watching, WatchingState::Off);
    assert!(!backend.is_watching(&root_id));

    // Back to Offline, then relocate -> starts watcher again on new path
    let new_dir = temp.path().join("photos_relocated");
    std::fs::create_dir_all(&new_dir).unwrap();
    {
        let mut conn = pool.acquire().await.unwrap();
        database::roots::set_root_status(&mut conn, &root_id, RootStatus::Offline, 6000)
            .await
            .unwrap();
    }
    let new_clone = new_dir.clone();
    let reloc = relocate_root_with_deps(
        &pool,
        &ctx,
        &dialog_guard,
        root_id.as_str(),
        || async move { Some(new_clone) },
        crate::library::volume::describe_volume,
        Some(&watcher),
    )
    .await
    .unwrap();
    assert_eq!(
        reloc.outcome,
        crate::commands::roots::RelocateRootOutcomeKind::Relocated
    );
    assert!(backend.is_watching(&root_id));

    // Remove root -> watcher stopped and entry removed
    remove_root_impl_with_watcher(&pool, root_id.as_str(), 0, Some(&watcher))
        .await
        .unwrap();
    assert!(!backend.is_watching(&root_id));
    assert!(backend.stop_count(&root_id) >= 2);
    assert!(backend.start_count(&root_id) >= 2);
}

#[tokio::test]
async fn watcher_start_failure_shows_off_and_does_not_fail_add_root() {
    let temp = tempdir().expect("tempdir");
    let app_data = temp.path().join("appdata");
    let root_dir = temp.path().join("photos");
    std::fs::create_dir_all(&app_data).unwrap();
    std::fs::create_dir_all(&root_dir).unwrap();

    let pool = init_pool(&app_data.join("cat.db")).await.unwrap();
    let ctx = test_context(&app_data);
    let dialog_guard = DialogGuard::default();
    let in_flight = Arc::new(Mutex::new(HashSet::new()));

    // Fake backend that fails every start_watch call
    struct AlwaysFailBackend;
    impl WatchBackend for AlwaysFailBackend {
        fn start_watch(&self, _: &RootId, _: &Path, _: WatchCallback) -> Result<(), String> {
            Err("permission denied / watch limit".to_string())
        }
        fn stop_watch(&self, _: &RootId) {}
    }

    let watcher = Arc::new(WatcherManager::with_backend(Arc::new(AlwaysFailBackend)));
    let root_clone = root_dir.clone();
    let res = add_root_with_picker_and_watcher(
        &pool,
        &ctx,
        &dialog_guard,
        None,
        || async move { Some(root_clone) },
        Some(&watcher),
    )
    .await
    .expect("watcher start failure must never fail add_root");

    assert_eq!(res.outcome, AddRootOutcomeKind::Added);
    let root_id = RootId::from(res.root_id.unwrap());
    assert_eq!(
        watcher.status_for_root(&root_id),
        (false, WatchingState::Off)
    );

    let summaries = list_roots_with_runtime_state(&pool, &in_flight, |_| None, Some(&watcher))
        .await
        .unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].status, RootStatus::Active);
    assert_eq!(summaries[0].watching, WatchingState::Off);
}

#[tokio::test]
async fn changes_detected_is_cleared_when_a_scan_starts() {
    let temp = tempdir().expect("tempdir");
    let app_data = temp.path().join("appdata");
    let root_dir = temp.path().join("photos");
    std::fs::create_dir_all(&app_data).unwrap();
    std::fs::create_dir_all(&root_dir).unwrap();
    std::fs::write(root_dir.join("a.jpg"), b"img").unwrap();

    let pool = init_pool(&app_data.join("cat.db")).await.unwrap();
    let ctx = test_context(&app_data);
    let dialog_guard = DialogGuard::default();
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    let backend = Arc::new(FakeWatchBackend::default());
    let watcher = Arc::new(WatcherManager::with_backend(
        Arc::clone(&backend) as Arc<dyn WatchBackend>
    ));

    let root_clone = root_dir.clone();
    let add_res = add_root_with_picker_and_watcher(
        &pool,
        &ctx,
        &dialog_guard,
        None,
        || async move { Some(root_clone) },
        Some(&watcher),
    )
    .await
    .unwrap();
    let root_id = RootId::from(add_res.root_id.unwrap());

    backend.emit(&root_id, WatchEvent::Changed);
    watcher.flush_debouncers(Instant::now() + Duration::from_secs(3));
    assert_eq!(watcher.status_for_root(&root_id), (true, WatchingState::On));

    let job_manager = Arc::new(JobManager::new_with_deps(
        pool.clone(),
        Arc::clone(&in_flight),
        Some(Arc::clone(&watcher)),
    ));

    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let done_tx = Arc::new(Mutex::new(Some(done_tx)));
    let start_res = job_manager
        .start_scan_with_trigger(root_id.clone(), ScanTrigger::Auto, move |ev| {
            if matches!(
                ev,
                crate::jobs::ScanEvent::Finished { .. } | crate::jobs::ScanEvent::Failed { .. }
            ) {
                if let Some(tx) = done_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
            }
        })
        .await
        .unwrap();

    // Cleared immediately upon scan start
    assert_eq!(
        watcher.status_for_root(&root_id),
        (false, WatchingState::On)
    );

    let _ = done_rx.await;

    // Verify payload_json persisted "trigger": "auto" and LastJobSummary surfaces it
    let mut conn = pool.acquire().await.unwrap();
    let job_row = database::get_job_by_id(&mut conn, &start_res.job_id)
        .await
        .unwrap()
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&job_row.1).unwrap();
    assert_eq!(payload["trigger"], "auto");

    let last_job = database::get_last_job_for_root(&mut conn, &root_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(last_job.trigger, "auto");
}

#[tokio::test]
async fn auto_scan_setting_roundtrip_and_default_off() {
    let temp = tempdir().expect("tempdir");
    let pool = init_pool(&temp.path().join("settings.db")).await.unwrap();

    // Default when unset is false
    assert!(!get_auto_scan_impl(&pool).await.unwrap());

    // Toggle on -> persists true
    assert!(set_auto_scan_impl(&pool, true).await.unwrap());
    assert!(get_auto_scan_impl(&pool).await.unwrap());

    // Toggle off -> persists false
    assert!(!set_auto_scan_impl(&pool, false).await.unwrap());
    assert!(!get_auto_scan_impl(&pool).await.unwrap());
}

#[tokio::test]
async fn real_watcher_detects_new_file_smoke() {
    let temp = tempdir().expect("tempdir");
    let root_dir = temp.path().join("watched_photos");
    std::fs::create_dir_all(&root_dir).unwrap();
    let canon_root = std::fs::canonicalize(&root_dir).unwrap();

    let watcher = WatcherManager::new_real();
    let root_id = RootId::new();
    watcher.ensure_watching(&root_id, &canon_root);
    assert_eq!(
        watcher.status_for_root(&root_id),
        (false, WatchingState::On)
    );

    // Give the OS watcher stream a moment to attach before writing the file
    tokio::time::sleep(Duration::from_millis(250)).await;
    std::fs::write(canon_root.join("smoke_photo.jpg"), b"jpeg-bytes").unwrap();

    // Poll with a 30 s deadline for the 2 s debouncer to mark changes_detected
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        watcher.flush_debouncers(Instant::now());
        let (detected, _) = watcher.status_for_root(&root_id);
        if detected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "real filesystem watcher did not mark changes within 30 s"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    watcher.remove_root(&root_id);
}

#[test]
fn scheduler_skips_never_scanned_root_even_with_changes_detected() {
    let now_inst = Instant::now();
    let now_ms = 10_000_000_000i64;

    // Never-scanned root (last_completed_scan_at_ms == None) with changes_detected=true must not auto-scan.
    let never_scanned = vec![make_sched_root(
        "r1",
        RootStatus::Active,
        true,
        Some(now_inst - Duration::from_secs(10)),
        false,
        None,
        None,
    )];
    assert_eq!(
        decide_next_auto_scan(true, false, &never_scanned, now_inst, now_ms),
        None
    );

    // Same root once a scan has completed becomes eligible for ChangesDetected.
    let scanned = vec![make_sched_root(
        "r1",
        RootStatus::Active,
        true,
        Some(now_inst - Duration::from_secs(10)),
        false,
        Some(now_ms - 120_000),
        Some(now_ms - 120_000),
    )];
    let decision = decide_next_auto_scan(true, false, &scanned, now_inst, now_ms)
        .expect("scanned root with changes_detected must be selected");
    assert_eq!(decision.root_id.as_str(), "r1");
    assert_eq!(decision.reason, AutoScanReason::ChangesDetected);
}

#[tokio::test]
async fn never_scanned_root_summary_hides_changes_detected_until_first_scan() {
    let temp = tempdir().expect("tempdir");
    let app_data = temp.path().join("appdata");
    let root_dir = temp.path().join("photos");
    std::fs::create_dir_all(&app_data).unwrap();
    std::fs::create_dir_all(&root_dir).unwrap();
    std::fs::write(root_dir.join("a.jpg"), b"img").unwrap();

    let pool = init_pool(&app_data.join("cat.db")).await.unwrap();
    let ctx = test_context(&app_data);
    let dialog_guard = DialogGuard::default();
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    let backend = Arc::new(FakeWatchBackend::default());
    let watcher = Arc::new(WatcherManager::with_backend(
        Arc::clone(&backend) as Arc<dyn WatchBackend>
    ));

    let root_clone = root_dir.clone();
    let add_res = add_root_with_picker_and_watcher(
        &pool,
        &ctx,
        &dialog_guard,
        None,
        || async move { Some(root_clone) },
        Some(&watcher),
    )
    .await
    .unwrap();
    let root_id = RootId::from(add_res.root_id.unwrap());
    assert!(backend.is_watching(&root_id));

    // Watcher receives a change before any scan has completed: mark exists internally,
    // but RootSummary must report changesDetected = false while watching stays On.
    backend.emit(&root_id, WatchEvent::Changed);
    watcher.flush_debouncers(Instant::now() + Duration::from_secs(3));
    let before_scan = list_roots_with_runtime_state(&pool, &in_flight, |_| None, Some(&watcher))
        .await
        .unwrap();
    assert_eq!(before_scan.len(), 1);
    assert!(before_scan[0].last_scan_at_ms.is_none());
    assert!(!before_scan[0].changes_detected);
    assert_eq!(before_scan[0].watching, WatchingState::On);

    // Run first scan to completion (stamps last_scan_at_ms).
    let job_manager = Arc::new(JobManager::new_with_deps(
        pool.clone(),
        Arc::clone(&in_flight),
        Some(Arc::clone(&watcher)),
    ));
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let done_tx = Arc::new(Mutex::new(Some(done_tx)));
    job_manager
        .start_scan_with_trigger(root_id.clone(), ScanTrigger::Manual, move |ev| {
            if matches!(
                ev,
                crate::jobs::ScanEvent::Finished { .. } | crate::jobs::ScanEvent::Failed { .. }
            ) {
                if let Some(tx) = done_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
            }
        })
        .await
        .unwrap();
    let _ = done_rx.await;

    // Subsequent watcher change after first completed scan surfaces changesDetected = true.
    backend.emit(&root_id, WatchEvent::Changed);
    watcher.flush_debouncers(Instant::now() + Duration::from_secs(3));
    let after_scan = list_roots_with_runtime_state(&pool, &in_flight, |_| None, Some(&watcher))
        .await
        .unwrap();
    assert!(after_scan[0].last_scan_at_ms.is_some());
    assert!(after_scan[0].changes_detected);
}
