-- v3: sync bookkeeping. Local-first: these tables only describe what still has to leave this device and
-- what the device knows about each synced field. The synced data itself stays in the tables above.

-- Records written locally since the last capture. Filled by triggers, so no write path has to know about sync.
CREATE TABLE IF NOT EXISTS sync_dirty (
    kind TEXT NOT NULL,
    id   TEXT NOT NULL,
    PRIMARY KEY (kind, id)
);

-- The last known value and hybrid-logical-clock of every synced field. A change from this device only
-- becomes an operation when the live value differs from the shadow; a remote operation only applies to
-- the fields where it is newer than the shadow (last-writer-wins per field).
CREATE TABLE IF NOT EXISTS sync_shadow (
    kind  TEXT NOT NULL,
    id    TEXT NOT NULL,
    field TEXT NOT NULL,
    hlc   TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (kind, id, field)
);

-- Field patches waiting to be pushed, in the order they were captured.
CREATE TABLE IF NOT EXISTS sync_outbox (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT NOT NULL,
    id         TEXT NOT NULL,
    hlc        TEXT NOT NULL,
    patch      TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- Pull position per remote (the remote's operation sequence number).
CREATE TABLE IF NOT EXISTS sync_state (
    remote TEXT PRIMARY KEY,
    cursor INTEGER NOT NULL DEFAULT 0
);

-- Attachment files already uploaded as encrypted blobs.
CREATE TABLE IF NOT EXISTS sync_blobs (
    attachment_id TEXT PRIMARY KEY,
    uploaded_at   INTEGER NOT NULL
);

-- Single-row switch: while a remote operation is being applied the triggers stay quiet.
CREATE TABLE IF NOT EXISTS sync_ctl (applying INTEGER NOT NULL);
INSERT INTO sync_ctl (applying) SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM sync_ctl);

CREATE TRIGGER IF NOT EXISTS sync_provider_configs_insert AFTER INSERT ON provider_configs
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('provider', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_provider_configs_update AFTER UPDATE ON provider_configs
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('provider', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_agents_insert AFTER INSERT ON agents
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('agent', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_agents_update AFTER UPDATE ON agents
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('agent', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_chats_insert AFTER INSERT ON chats
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('chat', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_chats_update AFTER UPDATE ON chats
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('chat', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_chat_agents_insert AFTER INSERT ON chat_agents
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('chat_agent', NEW.chat_id || '|' || NEW.agent_id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_chat_agents_update AFTER UPDATE ON chat_agents
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('chat_agent', NEW.chat_id || '|' || NEW.agent_id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_messages_insert AFTER INSERT ON messages
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('message', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_messages_update AFTER UPDATE ON messages
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('message', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_attachments_insert AFTER INSERT ON attachments
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('attachment', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_attachments_update AFTER UPDATE ON attachments
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('attachment', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_settings_insert AFTER INSERT ON settings
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('setting', NEW.key) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_settings_update AFTER UPDATE ON settings
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('setting', NEW.key) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_usage_insert AFTER INSERT ON usage
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('usage', NEW.agent_id || '|' || NEW.device_id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_usage_update AFTER UPDATE ON usage
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('usage', NEW.agent_id || '|' || NEW.device_id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_user_roles_insert AFTER INSERT ON user_roles
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('role', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_user_roles_update AFTER UPDATE ON user_roles
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('role', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_automations_insert AFTER INSERT ON automations
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('automation', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;

CREATE TRIGGER IF NOT EXISTS sync_automations_update AFTER UPDATE ON automations
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('automation', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;
