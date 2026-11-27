use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::domain::{CatalogueError, RootId};

/// Parameters for creating a new job row in the database.
pub struct NewJobParams<'a> {
    pub id: &'a str,
    pub root_id: Option<&'a str>,
    pub job_type: &'a str,
    pub status: &'a str,
    pub priority: i32,
    pub payload_json: &'a str,
    pub now_ms: i64,
}

/// Parameters for finalizing a job row upon completion, cancellation, or failure.
pub struct FinalizeJobParams<'a> {
    pub id: &'a str,
    pub status: &'a str,
    pub progress_done: i64,
    pub warning_count: u64,
    pub error_count: u64,
    pub error_code: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub finished_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Summary of the last job for a root (spec §12.1, Decision j).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastJobSummary {
    pub job_id: String,
    pub status: String,
    pub error_code: Option<String>,
    pub finished_at_ms: Option<i64>,
    pub trigger: String,
}

/// Inserts a new job row into the `jobs` table (spec §5.2, Decision d).
pub async fn insert_job(
    conn: &mut SqliteConnection,
    params: &NewJobParams<'_>,
) -> Result<(), CatalogueError> {
    sqlx::query(
        "INSERT INTO jobs (
           id, root_id, job_type, status, priority, payload_json,
           progress_done, progress_total, current_item,
           warning_count, error_count,
           created_at_ms, started_at_ms, updated_at_ms, finished_at_ms,
           error_code, error_message
         ) VALUES (?, ?, ?, ?, ?, ?, 0, NULL, NULL, 0, 0, ?, ?, ?, NULL, NULL, NULL);",
    )
    .bind(params.id)
    .bind(params.root_id)
    .bind(params.job_type)
    .bind(params.status)
    .bind(params.priority)
    .bind(params.payload_json)
    .bind(params.now_ms)
    .bind(params.now_ms)
    .bind(params.now_ms)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Updates the progress of a running job at most once per second (Decision e).
pub async fn update_job_progress(
    conn: &mut SqliteConnection,
    job_id: &str,
    progress_done: i64,
    updated_at_ms: i64,
) -> Result<(), CatalogueError> {
    sqlx::query("UPDATE jobs SET progress_done = ?, updated_at_ms = ? WHERE id = ?;")
        .bind(progress_done)
        .bind(updated_at_ms)
        .bind(job_id)
        .execute(&mut *conn)
        .await?;

    Ok(())
}

/// Updates a running job's status to 'cancelling' (spec §6.2, Decision c, R2).
///
/// Only updates the row if its current status is 'running'.
/// Returns the number of rows changed (1 if marked cancelling, 0 if job was not running).
pub async fn mark_job_cancelling(
    conn: &mut SqliteConnection,
    job_id: &str,
    updated_at_ms: i64,
) -> Result<u64, CatalogueError> {
    let result = sqlx::query(
        "UPDATE jobs SET status = 'cancelling', updated_at_ms = ? WHERE id = ? AND status = 'running';",
    )
    .bind(updated_at_ms)
    .bind(job_id)
    .execute(&mut *conn)
    .await?;

    Ok(result.rows_affected())
}

/// Finalizes a job row with its terminal status, counts, and error details (Decision e).
pub async fn finalize_job(
    conn: &mut SqliteConnection,
    params: &FinalizeJobParams<'_>,
) -> Result<(), CatalogueError> {
    sqlx::query(
        "UPDATE jobs SET
           status = ?,
           progress_done = ?,
           warning_count = ?,
           error_count = ?,
           error_code = ?,
           error_message = ?,
           finished_at_ms = ?,
           updated_at_ms = ?
         WHERE id = ?;",
    )
    .bind(params.status)
    .bind(params.progress_done)
    .bind(params.warning_count as i64)
    .bind(params.error_count as i64)
    .bind(params.error_code)
    .bind(params.error_message)
    .bind(params.finished_at_ms)
    .bind(params.updated_at_ms)
    .bind(params.id)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Prunes older finished jobs for a root beyond `keep_count` (Decision e: newest 20 per root).
///
/// Running, cancelling, or queued jobs are NEVER deleted.
pub async fn prune_finished_jobs(
    conn: &mut SqliteConnection,
    root_id: &str,
    keep_count: usize,
) -> Result<u64, CatalogueError> {
    let result = sqlx::query(
        "DELETE FROM jobs
         WHERE root_id = ?
           AND status IN ('completed', 'cancelled', 'failed')
           AND id NOT IN (
             SELECT id FROM jobs
             WHERE root_id = ?
               AND status IN ('completed', 'cancelled', 'failed')
             ORDER BY finished_at_ms DESC, created_at_ms DESC
             LIMIT ?
           );",
    )
    .bind(root_id)
    .bind(root_id)
    .bind(keep_count as i64)
    .execute(&mut *conn)
    .await?;

    Ok(result.rows_affected())
}

/// Startup reconciliation: marks all running, cancelling, or queued jobs as failed/interrupted (Decision f).
pub async fn reconcile_interrupted_jobs(
    conn: &mut SqliteConnection,
    error_message: &str,
    now_ms: i64,
) -> Result<u64, CatalogueError> {
    let result = sqlx::query(
        "UPDATE jobs
         SET status = 'failed',
             error_code = 'interrupted',
             error_message = ?,
             updated_at_ms = ?,
             finished_at_ms = ?
         WHERE status IN ('running', 'cancelling', 'queued');",
    )
    .bind(error_message)
    .bind(now_ms)
    .bind(now_ms)
    .execute(&mut *conn)
    .await?;

    Ok(result.rows_affected())
}

/// Fetches the newest job for a root (Decision j).
/// Note: `error_code` is returned only when status is 'failed'.
pub async fn get_last_job_for_root(
    conn: &mut SqliteConnection,
    root_id: &RootId,
) -> Result<Option<LastJobSummary>, CatalogueError> {
    let row: Option<SqliteRow> = sqlx::query(
        "SELECT id, status, error_code, finished_at_ms, payload_json
         FROM jobs
         WHERE root_id = ?
         ORDER BY created_at_ms DESC, id DESC
         LIMIT 1;",
    )
    .bind(root_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;

    match row {
        Some(r) => {
            let job_id: String = r.try_get("id")?;
            let status: String = r.try_get("status")?;
            let raw_error_code: Option<String> = r.try_get("error_code")?;
            let finished_at_ms: Option<i64> = r.try_get("finished_at_ms")?;
            let payload_json: String = r.try_get("payload_json")?;

            // errorCode is null unless failed (Decision j)
            let error_code = if status == "failed" {
                raw_error_code
            } else {
                None
            };

            let trigger = serde_json::from_str::<serde_json::Value>(&payload_json)
                .ok()
                .and_then(|v| {
                    v.get("trigger")
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                })
                .filter(|t| t == "auto" || t == "manual")
                .unwrap_or_else(|| "manual".to_string());

            Ok(Some(LastJobSummary {
                job_id,
                status,
                error_code,
                finished_at_ms,
                trigger,
            }))
        }
        None => Ok(None),
    }
}

/// Raw job fields tuple (status, payload_json, error_code, error_message, progress_done, finished_at_ms).
pub type JobDetailsTuple = (
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    Option<i64>,
);

/// Fetches raw job details by ID.
pub async fn get_job_by_id(
    conn: &mut SqliteConnection,
    id: &str,
) -> Result<Option<JobDetailsTuple>, CatalogueError> {
    let row: Option<JobDetailsTuple> = sqlx::query_as(
        "SELECT status, payload_json, error_code, error_message, progress_done, finished_at_ms
         FROM jobs WHERE id = ?;",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;

    Ok(row)
}

/// Counts total job rows in the database (useful for test assertions).
pub async fn count_total_jobs(conn: &mut SqliteConnection) -> Result<i64, CatalogueError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM jobs;")
        .fetch_one(&mut *conn)
        .await?;

    Ok(row.0)
}
