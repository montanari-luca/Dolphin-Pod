-- v5: per-chat command policy (network access, pre-approved commands). Local to this device, never synced.
CREATE TABLE IF NOT EXISTS chat_shell_policy (
    chat_id        TEXT PRIMARY KEY,
    network        INTEGER NOT NULL DEFAULT 0,
    allow_commands TEXT NOT NULL DEFAULT '[]',
    updated_at     INTEGER NOT NULL
);
