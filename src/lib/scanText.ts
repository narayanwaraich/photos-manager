import type { RootStatus, ScanCounts } from "./tauri/schemas";

/**
 * Fixed English messages for all scanner warning codes (spec D7, TASK-1E decision k).
 * Keys must match ALL_WARNING_CODES in Rust (verified bidirectionally by a unit test).
 */
export const SCAN_WARNING_MESSAGES: Record<string, string> = {
  path_key_collision:
    "A file or folder with an equivalent comparison key already exists in this folder.",
  path_not_unicode: "The file path contains invalid Unicode and was skipped.",
  entry_unreadable:
    "The file or folder could not be accessed due to disk or permission errors.",
  file_time_invalid:
    "The file modification timestamp is invalid and was skipped.",
  size_out_of_range: "The file size exceeds the supported 63-bit range.",
  path_not_inside_root: "The path is located outside the library root.",
  path_parent_component: "The path contains a forbidden parent component.",
  path_current_dir_component:
    "The path contains a forbidden current directory component.",
  path_absolute_component: "The path contains a forbidden absolute component.",
  path_empty_component: "The path contains an empty component.",
  path_nul_byte: "The path contains a forbidden null byte.",
  path_invalid_separator: "The path contains an invalid separator.",
  path_verbatim_mismatch: "The path contains a verbatim prefix mismatch.",
  path_invalid_native_length:
    "The native path encoding has an invalid byte length.",
};

/**
 * Translates a scanner warning code to a user-facing sentence.
 * Unknown codes fallback to a generic message without printing the raw code (Decision k).
 */
export function formatScanWarning(code: string): string {
  return (
    SCAN_WARNING_MESSAGES[code] ??
    "An item could not be processed due to an unexpected warning condition."
  );
}

/**
 * Translates a root addition warning code to a user-facing sentence.
 * Unknown codes fallback to a generic message without printing the raw code (Decision k, carried nit from 1C2).
 */
export function formatRootWarning(code: string): string {
  switch (code) {
    case "case_sensitivity_unknown":
      return "Could not tell whether this drive treats upper and lower case as different, so it is treated as case-insensitive.";
    case "network_volume":
      return "This folder is on a network drive. Scanning may be slow and the folder can go offline.";
    default:
      return "An unknown storage volume condition was detected.";
  }
}

/**
 * Formats live scan progress line: "Scanning… N files in M folders".
 */
export function formatScanningProgress(filesSeen: number, foldersSeen: number): string {
  return `Scanning… ${filesSeen} files in ${foldersSeen} folders`;
}

/**
 * Formats completed scan summary (Decision k).
 * Appends missing explanation when markedMissing > 0.
 */
export function formatCompletedSummary(
  counts: ScanCounts,
  warningCount: number,
): string {
  const parts = [
    `${counts.new} new`,
    `${counts.changed} changed`,
    `${counts.unchanged} unchanged`,
    `${counts.moved} moved`,
    `${counts.markedMissing} marked missing`,
    `${counts.skippedByRule} skipped by rule`,
    `${warningCount} warnings`,
  ];
  let summary = `Scan completed: ${parts.join(", ")}.`;
  if (counts.markedMissing > 0) {
    summary +=
      " Missing means: not found on disk during this scan; the files are not deleted by the app.";
  }
  return summary;
}

/**
 * Summary shown when a scan is cancelled by the user.
 */
export const SCAN_CANCELLED_TEXT =
  "Scan cancelled. Nothing was marked missing.";

/**
 * Text shown for last job when the job was interrupted by app closure.
 */
export const SCAN_INTERRUPTED_TEXT =
  "The last scan did not finish (the app closed). Press Scan to run it again.";

/**
 * Fallback message when an unexpected progress event payload arrives.
 */
export const SCAN_PROGRESS_ERROR_TEXT =
  "Something went wrong while reporting progress.";

/**
 * Fixed English messages for all scan failure codes (TASK-1F decision j, finding N1).
 * Keys must match ALL_SCAN_FAILURE_CODES in Rust and values must match fixed_message_for_code(code)
 * (verified bidirectionally by a Rust unit test).
 */
export const SCAN_FAILURE_MESSAGES: Record<string, string> = {
  scan_root_unreachable: "The library folder is missing or cannot be accessed.",
  scan_walk_failed: "The filesystem walk encountered an unrecoverable error.",
  scan_root_option_unsupported: "The requested scan option is not supported.",
  scan_already_running: "A scan is already in progress for this library folder.",
  root_offline:
    "This folder is not available. The drive may be unplugged, or the folder may have been moved or renamed.",
  root_volume_changed:
    "This path is now on a different drive than the one this folder was added from. Nothing was changed.",
  root_busy: "This folder is being scanned. Cancel the scan or wait until it ends.",
  root_not_found: "The specified library folder was not found.",
  job_not_found: "The requested job was not found or is no longer running.",
  interrupted: "The app closed before this job finished.",
  internal: "An internal application error occurred.",
  database: "A database error occurred.",
  not_found: "The requested entity was not found in the catalogue.",
  invalid_enum: "An invalid enum value was encountered.",
};

/**
 * Translates a scan failure code to a plain-language sentence without printing raw codes.
 */
export function formatScanFailure(code: string | null | undefined): string {
  if (code && Object.prototype.hasOwnProperty.call(SCAN_FAILURE_MESSAGES, code)) {
    return SCAN_FAILURE_MESSAGES[code];
  }
  return "The scan could not be completed.";
}

/**
 * Notice appended to a terminal scan result when the final job write could not be saved (TASK-1F decision k).
 */
export const SCAN_NOT_RECORDED_TEXT =
  "The result of this scan could not be saved to the history. The catalogue itself is fine.";

/**
 * Fixed UI texts for roots with stored status `Offline` (TASK-1F Part D).
 */
export const ROOT_OFFLINE_BADGE = "Not available";
export const ROOT_OFFLINE_DETAIL =
  "The drive may be unplugged, or the folder was moved or renamed.";
export const ROOT_OFFLINE_SCAN_DISABLED = "This folder is not available.";

/**
 * Fixed UI texts for filesystem watching hints and automatic scanning (TASK-1G decisions m, o, p).
 */
export const CHANGES_DETECTED_TEXT = "Changes detected since the last scan";
export const WATCHING_OFF_TEXT =
  "Watching: off — Automatic change detection could not start. Manual Scan still works.";
export const WATCHING_DEGRADED_TEXT =
  "Watching: degraded — Some folder changes may be missed. Manual Scan still works.";
export const AUTO_SCAN_CHECKBOX_LABEL = "Automatically scan changed folders";
export const AUTO_SCAN_RUNNING_TEXT = "Automatic scan is running…";
export const AUTO_SCAN_COMPLETED_BODY = "scan completed.";
export const AUTO_SCAN_CANCELLED_BODY =
  "scan cancelled. Nothing was marked missing.";

export interface LocalTimeParts {
  hours: number;
  minutes: number;
  seconds: number;
}

/**
 * Formats 24-hour local time with seconds (`HH:MM:SS`) from explicit parts.
 */
export function format24HourTimeFromParts(parts: LocalTimeParts): string {
  const hh = String(parts.hours).padStart(2, "0");
  const mm = String(parts.minutes).padStart(2, "0");
  const ss = String(parts.seconds).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

/**
 * Formats a Unix epoch millisecond timestamp as local 24-hour `HH:MM:SS`.
 */
export function formatLocalTime24(
  timestampMs: number,
  getParts: (date: Date) => LocalTimeParts = (d) => ({
    hours: d.getHours(),
    minutes: d.getMinutes(),
    seconds: d.getSeconds(),
  }),
): string | null {
  if (!Number.isFinite(timestampMs)) {
    return null;
  }
  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  return format24HourTimeFromParts(getParts(date));
}

/**
 * Formats the last-job result line for an automatic scan, including the local finish time when known.
 */
export function formatAutoScanResultLine(
  body: string,
  finishedAtMs: number | null | undefined,
  getParts?: (date: Date) => LocalTimeParts,
): string {
  const timeText =
    finishedAtMs !== null && finishedAtMs !== undefined
      ? formatLocalTime24(finishedAtMs, getParts)
      : null;
  return timeText
    ? `Started automatically at ${timeText}: ${body}`
    : `Started automatically: ${body}`;
}

/**
 * Maps stored root status and live reachability to the reachability indicator label.
 * When an Active root has reachable=false (e.g. an Unknown/timed-out check), show "Checking..."
 * rather than "not reachable"; Offline and other non-Active states keep "not reachable".
 */
export function getReachabilityLabel(
  status: RootStatus,
  reachable: boolean,
): "Checking..." | "not reachable" | null {
  if (reachable) {
    return null;
  }
  return status === "Active" ? "Checking..." : "not reachable";
}
