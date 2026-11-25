import type { ScanCounts } from "./tauri/schemas";

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
