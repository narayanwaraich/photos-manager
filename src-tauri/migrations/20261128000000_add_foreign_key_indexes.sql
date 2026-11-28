-- Migration 20261128000000: Add indexes for cascading foreign keys (TASK-1G Round 3 R4)

CREATE INDEX IF NOT EXISTS idx_files_folder ON files(folder_id);
CREATE INDEX IF NOT EXISTS idx_folders_parent ON folders(parent_id);
CREATE INDEX IF NOT EXISTS idx_assets_primary_file ON assets(primary_file_id);
CREATE INDEX IF NOT EXISTS idx_asset_files_file ON asset_files(file_id);
