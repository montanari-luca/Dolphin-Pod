-- v10: a note can be shared with other agents. `share` is JSON ({"type":"none"|"all"|"agents","ids":[...]}) and syncs.
ALTER TABLE agent_memory ADD COLUMN share TEXT NOT NULL DEFAULT '{"type":"none"}';

DROP TRIGGER IF EXISTS sync_agent_memory_update;
CREATE TRIGGER IF NOT EXISTS sync_agent_memory_update AFTER UPDATE OF agent_id, kind, text, project, source, pinned, share, created_at, deleted ON agent_memory
WHEN (SELECT applying FROM sync_ctl) = 0
BEGIN
    INSERT INTO sync_dirty (kind, id) VALUES ('agent_memory', NEW.id) ON CONFLICT (kind, id) DO NOTHING;
END;
