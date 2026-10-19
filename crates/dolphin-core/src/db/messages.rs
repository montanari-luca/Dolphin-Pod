use rusqlite::{OptionalExtension, Row, params};

use super::{Db, enum_str, from_json, parse_enum, to_json};
use crate::error::{CoreError, Result};
use crate::model::{Attachment, AttachmentKind, Message, MessageKind, MessageStatus};
use crate::util::now_ms;

const COLS: &str = "id, chat_id, agent_id, kind, status, content, reasoning, error, mentions, meta, \
                    thinking_ms, working_ms, tokens_in, tokens_out, created_at, updated_at";

fn row_to_message(r: &Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get(0)?,
        chat_id: r.get(1)?,
        agent_id: r.get(2)?,
        kind: parse_enum(&r.get::<_, String>(3)?).unwrap_or(MessageKind::Text),
        status: parse_enum(&r.get::<_, String>(4)?).unwrap_or(MessageStatus::Done),
        content: r.get(5)?,
        reasoning: r.get(6)?,
        error: r.get(7)?,
        mentions: from_json(r.get(8)?).unwrap_or_default(),
        attachments: Vec::new(),
        meta: from_json(r.get(9)?).unwrap_or(serde_json::Value::Object(Default::default())),
        thinking_ms: r.get(10)?,
        working_ms: r.get(11)?,
        tokens_in: r.get(12)?,
        tokens_out: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
    })
}

fn row_to_attachment(r: &Row) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: r.get(0)?,
        name: r.get(1)?,
        mime: r.get(2)?,
        kind: parse_enum(&r.get::<_, String>(3)?).unwrap_or(AttachmentKind::Other),
        size: r.get(4)?,
        path: r.get(5)?,
    })
}

impl Db {
    pub fn insert_message(&self, m: &Message) -> Result<()> {
        self.conn().execute(
            &format!("INSERT INTO messages ({COLS}, device_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)"),
            params![
                m.id,
                m.chat_id,
                m.agent_id,
                enum_str(&m.kind),
                enum_str(&m.status),
                m.content,
                m.reasoning,
                m.error,
                to_json(&m.mentions),
                to_json(&m.meta),
                m.thinking_ms,
                m.working_ms,
                m.tokens_in,
                m.tokens_out,
                m.created_at,
                m.updated_at,
                self.device_id()
            ],
        )?;
        for a in &m.attachments {
            self.link_attachment(&a.id, &m.id, &m.chat_id)?;
        }
        Ok(())
    }

    pub fn update_message(&self, m: &Message) -> Result<()> {
        self.conn().execute(
            "UPDATE messages SET kind = ?2, status = ?3, content = ?4, reasoning = ?5, error = ?6, meta = ?7,
               thinking_ms = ?8, working_ms = ?9, tokens_in = ?10, tokens_out = ?11, updated_at = ?12, device_id = ?13
             WHERE id = ?1",
            params![
                m.id,
                enum_str(&m.kind),
                enum_str(&m.status),
                m.content,
                m.reasoning,
                m.error,
                to_json(&m.meta),
                m.thinking_ms,
                m.working_ms,
                m.tokens_in,
                m.tokens_out,
                now_ms(),
                self.device_id()
            ],
        )?;
        Ok(())
    }

    pub fn get_message(&self, id: &str) -> Result<Message> {
        let mut m = self
            .conn()
            .query_row(&format!("SELECT {COLS} FROM messages WHERE id = ?1 AND deleted = 0"), [id], row_to_message)
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("message {id}")))?;
        m.attachments = self.message_attachments(&m.id)?;
        Ok(m)
    }

    /// Most recent `limit` messages of a chat, oldest first.
    pub fn list_messages(&self, chat_id: &str, limit: usize) -> Result<Vec<Message>> {
        let mut msgs = {
            let conn = self.conn();
            let mut st = conn.prepare(&format!(
                "SELECT * FROM (SELECT {COLS} FROM messages WHERE chat_id = ?1 AND deleted = 0
                 ORDER BY created_at DESC, id DESC LIMIT ?2) ORDER BY created_at ASC, id ASC"
            ))?;
            st.query_map(params![chat_id, limit as i64], row_to_message)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for m in &mut msgs {
            m.attachments = self.message_attachments(&m.id)?;
        }
        Ok(msgs)
    }

    /// Rolling cleanup: tombstone every finished message older than `cutoff_ms`. Returns the removed ids.
    pub fn tombstone_messages_before(&self, chat_id: &str, cutoff_ms: i64) -> Result<Vec<String>> {
        let conn = self.conn();
        let ids = {
            let mut st = conn.prepare(
                "SELECT id FROM messages WHERE chat_id = ?1 AND deleted = 0 AND created_at < ?2
                 AND status NOT IN ('pending', 'streaming')",
            )?;
            st.query_map(params![chat_id, cutoff_ms], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if ids.is_empty() {
            return Ok(ids);
        }
        conn.execute(
            "UPDATE messages SET deleted = 1, content = '', reasoning = NULL, meta = '{}', updated_at = ?3, device_id = ?4
             WHERE chat_id = ?1 AND deleted = 0 AND created_at < ?2 AND status NOT IN ('pending', 'streaming')",
            params![chat_id, cutoff_ms, now_ms(), self.device_id()],
        )?;
        Ok(ids)
    }

    pub fn tombstone_message(&self, id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE messages SET deleted = 1, content = '', reasoning = NULL, meta = '{}', updated_at = ?2, device_id = ?3 WHERE id = ?1",
            params![id, now_ms(), self.device_id()],
        )?;
        Ok(())
    }

    /// Clear every message in a chat (manual "clean this chat").
    pub fn tombstone_all_messages(&self, chat_id: &str) -> Result<Vec<String>> {
        self.tombstone_messages_before(chat_id, i64::MAX)
    }

    /// Hard-delete tombstones older than `cutoff_ms` (keeps the database small once sync has propagated them).
    pub fn purge_tombstones(&self, cutoff_ms: i64) -> Result<usize> {
        Ok(self.conn().execute("DELETE FROM messages WHERE deleted = 1 AND updated_at < ?1", [cutoff_ms])?)
    }

    /// Messages left mid-stream by a crash or quit are marked cancelled on startup.
    pub fn recover_interrupted_messages(&self) -> Result<usize> {
        Ok(self.conn().execute(
            "UPDATE messages SET status = 'cancelled' WHERE status IN ('pending', 'streaming') AND deleted = 0",
            [],
        )?)
    }

    pub fn find_notice(&self, chat_id: &str, notice_key: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id FROM messages WHERE chat_id = ?1 AND deleted = 0 AND kind = 'notice'
                 AND json_extract(meta, '$.notice') = ?2 ORDER BY created_at DESC LIMIT 1",
                params![chat_id, notice_key],
                |r| r.get(0),
            )
            .optional()?)
    }

    // ---------------------------------------------------------------- attachments

    pub fn insert_attachment(&self, a: &Attachment, chat_id: Option<&str>) -> Result<()> {
        let now = now_ms();
        self.conn().execute(
            "INSERT INTO attachments (id, chat_id, message_id, name, mime, kind, size, path, created_at, updated_at, device_id)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
            params![a.id, chat_id, a.name, a.mime, enum_str(&a.kind), a.size, a.path, now, self.device_id()],
        )?;
        Ok(())
    }

    pub fn get_attachment(&self, id: &str) -> Result<Attachment> {
        self.conn()
            .query_row(
                "SELECT id, name, mime, kind, size, path FROM attachments WHERE id = ?1 AND deleted = 0",
                [id],
                row_to_attachment,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("attachment {id}")))
    }

    pub(crate) fn link_attachment(&self, id: &str, message_id: &str, chat_id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE attachments SET message_id = ?2, chat_id = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, message_id, chat_id, now_ms()],
        )?;
        Ok(())
    }

    pub fn add_attachment_to_message(&self, a: &Attachment, message_id: &str, chat_id: &str) -> Result<()> {
        self.insert_attachment(a, Some(chat_id))?;
        self.link_attachment(&a.id, message_id, chat_id)
    }

    fn message_attachments(&self, message_id: &str) -> Result<Vec<Attachment>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, name, mime, kind, size, path FROM attachments WHERE message_id = ?1 AND deleted = 0 ORDER BY created_at",
        )?;
        let rows = st.query_map([message_id], row_to_attachment)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Attachments whose message was cleaned up (or that were staged and never sent). Returns their file paths.
    pub fn orphaned_attachment_paths(&self, staged_cutoff_ms: i64) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT a.id, a.path FROM attachments a LEFT JOIN messages m ON m.id = a.message_id
             WHERE a.deleted = 0 AND ((a.message_id IS NOT NULL AND (m.id IS NULL OR m.deleted = 1))
                OR (a.message_id IS NULL AND a.created_at < ?1))",
        )?;
        let rows = st
            .query_map([staged_cutoff_ms], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn tombstone_attachment(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("UPDATE attachments SET deleted = 1, updated_at = ?2 WHERE id = ?1", params![id, now_ms()])?;
        Ok(())
    }
}
