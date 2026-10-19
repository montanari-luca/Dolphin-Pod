-- Dolphin Pod local database, schema v1.
-- Syncable tables carry updated_at / device_id / deleted (tombstone) for Phase 2 sync.

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_configs (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    name       TEXT NOT NULL,
    base_url   TEXT NOT NULL,
    extra      TEXT NOT NULL DEFAULT '{}',
    enabled    INTEGER NOT NULL DEFAULT 1,
    updated_at INTEGER NOT NULL,
    device_id  TEXT NOT NULL,
    deleted    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS agents (
    id          TEXT PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    role        TEXT NOT NULL,
    specialty   TEXT NOT NULL,
    job         TEXT NOT NULL,
    color       TEXT NOT NULL,
    model       TEXT,
    image_model TEXT,
    ios_model   TEXT,
    sort        INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL,
    device_id   TEXT NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS chats (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    icon              TEXT NOT NULL DEFAULT '',
    sort              INTEGER NOT NULL DEFAULT 0,
    cleanup_enabled   INTEGER NOT NULL DEFAULT 1,
    cleanup_ttl_hours INTEGER NOT NULL DEFAULT 24,
    workspace_path    TEXT,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    device_id         TEXT NOT NULL,
    deleted           INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS chat_agents (
    chat_id    TEXT NOT NULL,
    agent_id   TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    device_id  TEXT NOT NULL,
    deleted    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (chat_id, agent_id)
);

CREATE TABLE IF NOT EXISTS messages (
    id          TEXT PRIMARY KEY,
    chat_id     TEXT NOT NULL,
    agent_id    TEXT,
    kind        TEXT NOT NULL,
    status      TEXT NOT NULL,
    content     TEXT NOT NULL DEFAULT '',
    reasoning   TEXT,
    error       TEXT,
    mentions    TEXT NOT NULL DEFAULT '[]',
    meta        TEXT NOT NULL DEFAULT '{}',
    thinking_ms INTEGER,
    working_ms  INTEGER,
    tokens_in   INTEGER,
    tokens_out  INTEGER,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    device_id   TEXT NOT NULL,
    deleted     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_messages_chat_time ON messages (chat_id, deleted, created_at);

CREATE TABLE IF NOT EXISTS attachments (
    id         TEXT PRIMARY KEY,
    chat_id    TEXT,
    message_id TEXT,
    name       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    path       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    device_id  TEXT NOT NULL,
    deleted    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments (message_id);

CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    device_id  TEXT NOT NULL
);

-- Usage is counted per device and summed, so concurrent devices never overwrite each other.
CREATE TABLE IF NOT EXISTS usage (
    agent_id      TEXT NOT NULL,
    device_id     TEXT NOT NULL,
    tokens_in     INTEGER NOT NULL DEFAULT 0,
    tokens_out    INTEGER NOT NULL DEFAULT 0,
    thinking_ms   INTEGER NOT NULL DEFAULT 0,
    working_ms    INTEGER NOT NULL DEFAULT 0,
    requests      INTEGER NOT NULL DEFAULT 0,
    last_query_ms INTEGER,
    last_query_at INTEGER,
    updated_at    INTEGER NOT NULL,
    PRIMARY KEY (agent_id, device_id)
);

CREATE TABLE IF NOT EXISTS permission_grants (
    chat_id    TEXT NOT NULL,
    agent_id   TEXT NOT NULL,
    level      TEXT NOT NULL,
    reason     TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL,
    device_id  TEXT NOT NULL,
    PRIMARY KEY (chat_id, agent_id)
);

CREATE TABLE IF NOT EXISTS action_log (
    id         TEXT PRIMARY KEY,
    chat_id    TEXT NOT NULL,
    agent_id   TEXT NOT NULL,
    tool       TEXT NOT NULL,
    summary    TEXT NOT NULL,
    allowed    INTEGER NOT NULL,
    detail     TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    device_id  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_action_log_chat ON action_log (chat_id, created_at);

CREATE TABLE IF NOT EXISTS automations (
    id               TEXT PRIMARY KEY,
    chat_id          TEXT NOT NULL,
    text             TEXT NOT NULL,
    time_of_day      TEXT NOT NULL,
    weekdays         TEXT NOT NULL DEFAULT '[]',
    enabled          INTEGER NOT NULL DEFAULT 1,
    last_run_date    TEXT,
    runner_device_id TEXT,
    updated_at       INTEGER NOT NULL,
    device_id        TEXT NOT NULL,
    deleted          INTEGER NOT NULL DEFAULT 0
);
