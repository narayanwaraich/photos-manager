-- Jobs table (spec §5.2, TASK-1E decision b)
CREATE TABLE jobs (
  id TEXT PRIMARY KEY,
  root_id TEXT REFERENCES library_roots(id) ON DELETE CASCADE,
  job_type TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued','running','cancelling','completed','cancelled','failed','paused')),
  priority INTEGER NOT NULL DEFAULT 50,
  payload_json TEXT NOT NULL,
  progress_done INTEGER NOT NULL DEFAULT 0,
  progress_total INTEGER,
  current_item TEXT,
  warning_count INTEGER NOT NULL DEFAULT 0,
  error_count INTEGER NOT NULL DEFAULT 0,
  created_at_ms INTEGER NOT NULL,
  started_at_ms INTEGER,
  updated_at_ms INTEGER NOT NULL,
  finished_at_ms INTEGER,
  error_code TEXT,
  error_message TEXT
);

CREATE INDEX idx_jobs_root_created ON jobs(root_id, created_at_ms);
