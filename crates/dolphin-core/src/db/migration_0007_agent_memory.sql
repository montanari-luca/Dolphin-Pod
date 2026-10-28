-- v7: per-agent memory (what an agent has learned about the user). Synced, except `hits` and `updated_at`, which stay on this device.
CREATE TABLE IF NOT EXISTS agent_memory (
    id         TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL,
    kind       TEXT NOT NULL,
    text       TEXT NOT NULL,
    project    TEXT,
    source     TEXT NOT NULL,
    pinned     INTEGER NOT NULL DEFAULT 0,
    hits       INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0,
    device_id  TEXT NOT NULL DEFAULT '',
    deleted    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_agent_memory_agent ON agent_memory (agent_id, deleted);

CREATE TRIGGER IF NOT EXISTS sync_agent_memory_insert AFTER INSERT ON agent_memory
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('agent_memory', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

-- Only synced columns: counting a memory as used must not make it look edited.
CREATE TRIGGER IF NOT EXISTS sync_agent_memory_update AFTER UPDATE OF agent_id, kind, text, project, source, pinned, created_at, deleted ON agent_memory
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('agent_memory', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;
