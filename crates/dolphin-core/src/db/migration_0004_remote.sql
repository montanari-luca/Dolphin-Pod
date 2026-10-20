-- v4: remote control. Ids of requests this device already acted on, so a replayed request does nothing.
CREATE TABLE IF NOT EXISTS remote_seen (
    id      TEXT PRIMARY KEY,
    seen_at INTEGER NOT NULL
);
