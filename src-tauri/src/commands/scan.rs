use sqlx::SqlitePool;
use std::sync::Arc;

use crate::commands::error::IpcError;
use crate::domain::RootId;
use crate::jobs::{JobManager, ScanEvent, StartScanResult};

/// Plain async implementation of `start_scan` (Decision g).
///
/// Decoupled from Tauri types so tests can pass a recording closure.
pub async fn start_scan_impl<S>(
    manager: &Arc<JobManager>,
    root_id_str: &str,
    sink: S,
) -> Result<StartScanResult, IpcError>
where
    S: Fn(ScanEvent) + Send + Sync + 'static,
{
    let root_id = RootId::from(root_id_str.to_string());
    manager.start_scan_impl(root_id, sink).await
}

/// Plain async implementation of `cancel_scan` (Decision c).
pub async fn cancel_scan_impl(manager: &JobManager, job_id: &str) -> Result<(), IpcError> {
    manager.cancel_scan_impl(job_id).await
}

//
// Tauri command wrappers
//

#[tauri::command]
pub async fn start_scan(
    _pool: tauri::State<'_, SqlitePool>,
    manager: tauri::State<'_, Arc<JobManager>>,
    root_id: String,
    on_event: tauri::ipc::Channel<ScanEvent>,
) -> Result<StartScanResult, IpcError> {
    let root_id_typed = RootId::from(root_id);
    manager
        .start_scan_impl(root_id_typed, move |event| {
            // Failed send (e.g. window reloaded or closed) is ignored and never stops the job (Decision g)
            let _ = on_event.send(event);
        })
        .await
}

#[tauri::command]
pub async fn cancel_scan(
    manager: tauri::State<'_, Arc<JobManager>>,
    job_id: String,
) -> Result<(), IpcError> {
    manager.cancel_scan_impl(&job_id).await
}
