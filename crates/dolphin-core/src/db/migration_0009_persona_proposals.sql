-- v9: agents may suggest changes to their own persona (a switch synced with the agent); the suggestions wait here for
-- the user to accept or dismiss them. Proposals are local to this device.
ALTER TABLE agents ADD COLUMN suggest_persona INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS persona_proposal (
    id         TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL,
    text       TEXT NOT NULL,
    reason     TEXT NOT NULL DEFAULT '',
    status     TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_persona_proposal_agent ON persona_proposal (agent_id, status);
