-- Migration 20261116000000: Create Phase 1 catalogue tables (TASK-1A, spec §5.2, D27)

CREATE TABLE volumes (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL DEFAULT 'unknown' CHECK (kind IN ('internal','removable','network','unknown')),
  label TEXT,
  fs_uuid TEXT,
  fs_type TEXT,
  case_sensitive INTEGER NOT NULL DEFAULT 0 CHECK (case_sensitive IN (0,1)),
  last_seen_at_ms INTEGER
);

CREATE TABLE library_roots (
  id TEXT PRIMARY KEY,
  volume_id TEXT REFERENCES volumes(id),
  label TEXT NOT NULL,
  path_display TEXT NOT NULL,
  path_native BLOB NOT NULL,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','paused','offline','removed')),
  follow_symlinks INTEGER NOT NULL DEFAULT 0 CHECK (follow_symlinks IN (0,1)),
  include_hidden INTEGER NOT NULL DEFAULT 0 CHECK (include_hidden IN (0,1)),
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  last_scan_at_ms INTEGER,
  last_scan_generation TEXT
);

CREATE TABLE folders (
  id TEXT PRIMARY KEY,
  root_id TEXT NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
  parent_id TEXT REFERENCES folders(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  rel_path_display TEXT NOT NULL,
  rel_path_native BLOB NOT NULL,
  path_key BLOB NOT NULL,
  status TEXT NOT NULL DEFAULT 'present' CHECK (status IN ('present','missing','inaccessible')),
  modified_at_ms INTEGER,
  last_seen_generation TEXT,
  UNIQUE (root_id, path_key)
);

CREATE TABLE files (
  id TEXT PRIMARY KEY,
  root_id TEXT NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
  folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  extension TEXT,
  rel_path_display TEXT NOT NULL,
  rel_path_native BLOB NOT NULL,
  path_key BLOB NOT NULL,
  size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
  modified_at_ms INTEGER NOT NULL,
  created_at_ms INTEGER,
  file_identity TEXT,
  status TEXT NOT NULL DEFAULT 'present'
    CHECK (status IN ('present','missing','inaccessible','unsupported','error')),
  scan_generation TEXT,
  last_verified_at_ms INTEGER,
  created_in_catalog_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  UNIQUE (root_id, path_key)
);

CREATE INDEX idx_files_size ON files(size_bytes);
CREATE INDEX idx_files_identity ON files(file_identity);

CREATE TABLE assets (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL DEFAULT 'image' CHECK (kind IN ('image','video')),
  primary_file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  format TEXT,
  width INTEGER,
  height INTEGER,
  orientation INTEGER,
  date_taken_ms INTEGER,
  date_taken_source TEXT,
  date_taken_raw TEXT,
  tz_offset_min INTEGER,
  camera_make TEXT,
  camera_model TEXT,
  lens_model TEXT,
  iso INTEGER,
  aperture REAL,
  shutter_speed TEXT,
  focal_length REAL,
  latitude REAL,
  longitude REAL,
  is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0,1)),
  status TEXT NOT NULL DEFAULT 'present'
    CHECK (status IN ('present','missing','unsupported','error')),
  created_in_catalog_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_assets_date ON assets(date_taken_ms);

CREATE TABLE asset_files (
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'primary',
  PRIMARY KEY (asset_id, file_id)
);
