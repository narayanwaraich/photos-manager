-- Migration 20261102000000: Create app_settings table (TASK-0A)
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
