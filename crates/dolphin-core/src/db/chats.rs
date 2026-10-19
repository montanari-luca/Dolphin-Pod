use rusqlite::{OptionalExtension, Row, params};

use super::Db;
use crate::error::{CoreError, Result};
use crate::model::Chat;
use crate::util::now_ms;

const COLS: &str = "id, name, icon, sort, cleanup_enabled, cleanup_ttl_hours, workspace_path, created_at, updated_at";

fn row_to_chat(r: &Row) -> rusqlite::Result<Chat> {
    Ok(Chat {
        id: r.get(0)?,
        name: r.get(1)?,
        icon: r.get(2)?,
        sort: r.get(3)?,
        agent_ids: Vec::new(),
        cleanup_enabled: r.get::<_, i64>(4)? != 0,
        cleanup_ttl_hours: r.get(5)?,
        workspace_path: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

impl Db {
    pub fn list_chats(&self) -> Result<Vec<Chat>> {
        let mut chats = {
            let conn = self.conn();
            let mut st =
                conn.prepare(&format!("SELECT {COLS} FROM chats WHERE deleted = 0 ORDER BY sort, created_at"))?;
            st.query_map([], row_to_chat)?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for c in &mut chats {
            c.agent_ids = self.chat_agent_ids(&c.id)?;
        }
        Ok(chats)
    }

    pub fn get_chat(&self, id: &str) -> Result<Chat> {
        let mut chat = self
            .conn()
            .query_row(&format!("SELECT {COLS} FROM chats WHERE id = ?1 AND deleted = 0"), [id], row_to_chat)
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("chat {id}")))?;
        chat.agent_ids = self.chat_agent_ids(id)?;
        Ok(chat)
    }

    fn chat_agent_ids(&self, chat_id: &str) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT ca.agent_id FROM chat_agents ca JOIN agents a ON a.id = ca.agent_id
             WHERE ca.chat_id = ?1 AND ca.deleted = 0 AND a.deleted = 0 ORDER BY a.sort",
        )?;
        let ids = st.query_map([chat_id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    }

    pub fn upsert_chat(&self, c: &Chat) -> Result<Chat> {
        let mut c = c.clone();
        let now = now_ms();
        c.updated_at = now;
        if c.created_at == 0 {
            c.created_at = now;
        }
        let dev = self.device_id().to_string();
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO chats (id, name, icon, sort, cleanup_enabled, cleanup_ttl_hours, workspace_path, created_at, updated_at, device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, icon = excluded.icon, sort = excluded.sort,
               cleanup_enabled = excluded.cleanup_enabled, cleanup_ttl_hours = excluded.cleanup_ttl_hours,
               workspace_path = excluded.workspace_path, updated_at = excluded.updated_at, device_id = excluded.device_id",
            params![
                c.id,
                c.name,
                c.icon,
                c.sort,
                c.cleanup_enabled as i64,
                c.cleanup_ttl_hours,
                c.workspace_path,
                c.created_at,
                c.updated_at,
                dev
            ],
        )?;
        // Membership: tombstone removed agents, (re)activate current ones.
        tx.execute(
            "UPDATE chat_agents SET deleted = 1, updated_at = ?2, device_id = ?3 WHERE chat_id = ?1",
            params![c.id, now, dev],
        )?;
        for aid in &c.agent_ids {
            tx.execute(
                "INSERT INTO chat_agents (chat_id, agent_id, updated_at, device_id, deleted) VALUES (?1, ?2, ?3, ?4, 0)
                 ON CONFLICT(chat_id, agent_id) DO UPDATE SET deleted = 0, updated_at = excluded.updated_at",
                params![c.id, aid, now, dev],
            )?;
        }
        tx.commit()?;
        drop(conn);
        self.get_chat(&c.id)
    }

    pub fn delete_chat(&self, id: &str) -> Result<()> {
        let now = now_ms();
        let conn = self.conn();
        conn.execute(
            "UPDATE chats SET deleted = 1, updated_at = ?2, device_id = ?3 WHERE id = ?1",
            params![id, now, self.device_id()],
        )?;
        conn.execute(
            "UPDATE messages SET deleted = 1, content = '', reasoning = NULL, updated_at = ?2 WHERE chat_id = ?1",
            params![id, now],
        )?;
        Ok(())
    }
}
