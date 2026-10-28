-- v8: per-agent learning switch (synced with the agent) and the log of feedback signals the learner works through
-- (local to this device, never synced).
ALTER TABLE agents ADD COLUMN learning INTEGER NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS agent_feedback (
    id         TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL,
    chat_id    TEXT NOT NULL,
    message_id TEXT,
    signal     TEXT NOT NULL,
    detail     TEXT NOT NULL DEFAULT '{}',
    processed  INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_feedback_pending ON agent_feedback (processed, agent_id, created_at);
