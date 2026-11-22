-- Migration 20261121000000: Enforce one root-folder per library root (TASK-1C, spec §5.2, D27)

CREATE UNIQUE INDEX idx_folders_one_root_folder ON folders(root_id) WHERE parent_id IS NULL;
