/**
 * Stable IPC error codes shared between Rust domain/catalogue errors and frontend.
 * Sync is enforced bidirectionally by a Rust unit test.
 */
export const KNOWN_IPC_ERROR_CODES = [
  "path_key_collision",
  "catalogue_platform_mismatch",
  "not_found",
  "size_out_of_range",
  "database",
  "database_migration",
  "path_not_unicode",
  "path_not_inside_root",
  "path_parent_component",
  "path_current_dir_component",
  "path_absolute_component",
  "path_empty_component",
  "path_nul_byte",
  "path_invalid_separator",
  "path_verbatim_mismatch",
  "path_invalid_native_length",
  "invalid_enum",
  "app_data_dir_not_usable",
  "catalogue_open_failed",
  "libvips_init_failed",
  "root_path_invalid",
  "root_not_unicode",
  "root_overlaps",
  "root_protected",
  "root_label_invalid",
  "root_not_found",
  "root_changed_since_preview",
  "volume_unavailable",
  "dialog_busy",
  "scan_already_running",
  "scan_root_unreachable",
  "scan_root_option_unsupported",
  "internal",
] as const;

export type KnownIpcErrorCode = (typeof KNOWN_IPC_ERROR_CODES)[number];
