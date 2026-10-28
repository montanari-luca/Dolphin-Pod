use rusqlite::{OptionalExtension, Row, params, params_from_iter};

use super::{Db, enum_str, parse_enum};
use crate::error::{CoreError, Result};
use crate::model::{AgentMemory, MemoryKind, MemorySource};
use crate::util::now_ms;

const COLS: &str = "id, agent_id, kind, text, project, source, pinned, hits, created_at, updated_at";

fn row_to_memory(r: &Row) -> rusqlite::Result<AgentMemory> {
    Ok(AgentMemory {
        id: r.get(0)?,
        agent_id: r.get(1)?,
        kind: parse_enum(&r.get::<_, String>(2)?).unwrap_or(MemoryKind::Fact),
        text: r.get(3)?,
        project: r.get(4)?,
        source: parse_enum(&r.get::<_, String>(5)?).unwrap_or(MemorySource::Auto),
        pinned: r.get::<_, i64>(6)? != 0,
        hits: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

impl Db {
    /// An agent's memories, pinned first, then oldest first.
    pub fn list_memories(&self, agent_id: &str) -> Result<Vec<AgentMemory>> {
        let conn = self.conn();
        let mut st =
            conn.prepare(&format!("SELECT {COLS} FROM agent_memory WHERE agent_id = ?1 AND deleted = 0 ORDER BY pinned DESC, created_at, id"))?;
        Ok(st.query_map([agent_id], row_to_memory)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_memory(&self, id: &str) -> Result<AgentMemory> {
        self.conn()
            .query_row(&format!("SELECT {COLS} FROM agent_memory WHERE id = ?1 AND deleted = 0"), [id], row_to_memory)
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("memory {id}")))
    }

    /// Whose memory this is, even after it was forgotten.
    pub fn memory_agent_id(&self, id: &str) -> Result<Option<String>> {
        Ok(self.conn().query_row("SELECT agent_id FROM agent_memory WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    pub fn count_memories(&self, agent_id: &str) -> Result<usize> {
        let n: i64 = self.conn().query_row("SELECT COUNT(*) FROM agent_memory WHERE agent_id = ?1 AND deleted = 0", [agent_id], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Insert or update one entry. `hits` and `created_at` are only written when the entry is new.
    pub fn upsert_memory(&self, m: &AgentMemory) -> Result<AgentMemory> {
        let mut m = m.clone();
        m.updated_at = now_ms();
        if m.created_at == 0 {
            m.created_at = m.updated_at;
        }
        self.conn().execute(
            "INSERT INTO agent_memory (id, agent_id, kind, text, project, source, pinned, hits, created_at, updated_at, device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, text = excluded.text, project = excluded.project,
               source = excluded.source, pinned = excluded.pinned, updated_at = excluded.updated_at,
               device_id = excluded.device_id, deleted = 0",
            params![
                m.id,
                m.agent_id,
                enum_str(&m.kind),
                m.text,
                m.project,
                enum_str(&m.source),
                m.pinned as i64,
                m.hits,
                m.created_at,
                m.updated_at,
                self.device_id()
            ],
        )?;
        Ok(m)
    }

    /// Forget one entry (a tombstone, so the deletion syncs). Whether it existed.
    pub fn delete_memory(&self, id: &str) -> Result<bool> {
        let n = self.conn().execute(
            "UPDATE agent_memory SET deleted = 1, updated_at = ?2, device_id = ?3 WHERE id = ?1 AND deleted = 0",
            params![id, now_ms(), self.device_id()],
        )?;
        Ok(n > 0)
    }

    /// Forget everything one agent remembers. How many entries went.
    pub fn forget_memories(&self, agent_id: &str) -> Result<usize> {
        Ok(self.conn().execute(
            "UPDATE agent_memory SET deleted = 1, updated_at = ?2, device_id = ?3 WHERE agent_id = ?1 AND deleted = 0",
            params![agent_id, now_ms(), self.device_id()],
        )?)
    }

    /// Count these entries as sent with a request. Stays on this device: it does not mark them as edited.
    pub fn bump_memory_hits(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let marks = vec!["?"; ids.len()].join(", ");
        self.conn().execute(&format!("UPDATE agent_memory SET hits = hits + 1 WHERE id IN ({marks})"), params_from_iter(ids.iter()))?;
        Ok(())
    }
}
