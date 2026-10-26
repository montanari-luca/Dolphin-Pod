//! Local-first SQLite store. Each device keeps its own database as the source of truth.

mod agents;
mod chats;
mod messages;
mod misc;
mod providers;
mod roles;

use std::path::Path;
use std::sync::atomic::AtomicI64;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::Result;
use crate::util::new_id;

/// Ordered migrations; index + 1 is the schema version it produces. Only ever append.
const MIGRATIONS: &[&str] = &[
    include_str!("schema.sql"),
    include_str!("migration_0002_user_roles.sql"),
    include_str!("migration_0003_sync.sql"),
    include_str!("migration_0004_remote.sql"),
    include_str!("migration_0005_shell_policy.sql"),
];

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    device_id: Arc<str>,
    /// Test hook for the sync clock; always 0 in production.
    pub(crate) clock_skew_ms: Arc<AtomicI64>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
            conn.execute_batch(&format!("BEGIN;\n{sql}\nCOMMIT;"))?;
            conn.pragma_update(None, "user_version", i as i32 + 1)?;
        }
        let device_id: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = 'device_id'", [], |r| r.get(0))
            .optional()?;
        let device_id = match device_id {
            Some(id) => id,
            None => {
                let id = new_id();
                conn.execute("INSERT INTO meta (key, value) VALUES ('device_id', ?1)", [&id])?;
                id
            }
        };
        Ok(Self { conn: Arc::new(Mutex::new(conn)), device_id: device_id.into(), clock_skew_ms: Arc::new(AtomicI64::new(0)) })
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        // A poisoned lock only means another thread panicked mid-query; SQLite itself is still consistent.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Replace an id everywhere it appears: key columns, foreign keys and the JSON that embeds ids.
    /// Returns false (and changes nothing) when `to` is already taken by another record.
    pub(crate) fn rename_id(&self, from: &str, to: &str) -> Result<bool> {
        let mut conn = self.conn();
        let taken: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE id = ?1) OR EXISTS(SELECT 1 FROM chats WHERE id = ?1)
                 OR EXISTS(SELECT 1 FROM user_roles WHERE id = ?1)",
            [to],
            |r| r.get(0),
        )?;
        if taken {
            return Ok(false);
        }
        let tx = conn.transaction()?;
        tx.execute("UPDATE sync_ctl SET applying = 1", [])?;
        // Whole-string columns.
        for (table, col) in [
            ("agents", "id"),
            ("chats", "id"),
            ("user_roles", "id"),
            ("chat_agents", "chat_id"),
            ("chat_agents", "agent_id"),
            ("messages", "chat_id"),
            ("messages", "agent_id"),
            ("attachments", "chat_id"),
            ("usage", "agent_id"),
            ("permission_grants", "chat_id"),
            ("permission_grants", "agent_id"),
            ("chat_shell_policy", "chat_id"),
            ("action_log", "chat_id"),
            ("action_log", "agent_id"),
            ("automations", "chat_id"),
        ] {
            tx.execute(&format!("UPDATE {table} SET {col} = ?2 WHERE {col} = ?1"), [from, to])?;
        }
        // Ids embedded in JSON (mentions, routing metadata) and in per-id setting values.
        tx.execute("UPDATE messages SET mentions = REPLACE(mentions, ?1, ?2) WHERE mentions LIKE '%' || ?1 || '%'", [from, to])?;
        tx.execute("UPDATE messages SET meta = REPLACE(meta, ?1, ?2) WHERE meta LIKE '%' || ?1 || '%'", [from, to])?;
        tx.execute("UPDATE settings SET value = REPLACE(value, ?1, ?2) WHERE value LIKE '%' || ?1 || '%'", [from, to])?;
        tx.execute("UPDATE sync_ctl SET applying = 0", [])?;
        tx.commit()?;
        Ok(true)
    }

    /// Attachments with a local file that haven't been uploaded for sync yet (id, path).
    pub fn sync_blobs_pending(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT a.id, a.path FROM attachments a
             WHERE a.deleted = 0 AND a.message_id IS NOT NULL AND a.path != ''
               AND NOT EXISTS (SELECT 1 FROM sync_blobs b WHERE b.attachment_id = a.id)",
        )?;
        Ok(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn sync_blob_done(&self, attachment_id: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO sync_blobs (attachment_id, uploaded_at) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            rusqlite::params![attachment_id, crate::util::now_ms()],
        )?;
        Ok(())
    }

    /// Remember a remote request id. Returns false if it was already seen (a replay).
    pub fn remote_mark_seen(&self, id: &str) -> Result<bool> {
        let now = crate::util::now_ms();
        let conn = self.conn();
        conn.execute("DELETE FROM remote_seen WHERE seen_at < ?1", [now - 24 * 3_600_000])?;
        Ok(conn.execute("INSERT INTO remote_seen (id, seen_at) VALUES (?1, ?2) ON CONFLICT (id) DO NOTHING", rusqlite::params![id, now])? == 1)
    }

    /// Write a message that arrived over the nearby-network channel. It is not queued for sync (the desktop
    /// that wrote it syncs it); when the same message arrives through sync it simply agrees.
    pub fn ingest_message(&self, m: &crate::model::Message) -> Result<()> {
        use crate::db::{enum_str, to_json};
        let conn = self.conn();
        conn.execute("UPDATE sync_ctl SET applying = 1", [])?;
        let r = conn.execute(
            "INSERT INTO messages (id, chat_id, agent_id, kind, status, content, reasoning, error, mentions, meta,
               thinking_ms, working_ms, tokens_in, tokens_out, created_at, updated_at, device_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT (id) DO UPDATE SET status = excluded.status, content = excluded.content, reasoning = excluded.reasoning,
               error = excluded.error, meta = excluded.meta, thinking_ms = excluded.thinking_ms, working_ms = excluded.working_ms,
               tokens_in = excluded.tokens_in, tokens_out = excluded.tokens_out, updated_at = excluded.updated_at",
            rusqlite::params![
                m.id, m.chat_id, m.agent_id, enum_str(&m.kind), enum_str(&m.status), m.content, m.reasoning, m.error,
                to_json(&m.mentions), to_json(&m.meta), m.thinking_ms, m.working_ms, m.tokens_in, m.tokens_out,
                m.created_at, crate::util::now_ms(), "lan"
            ],
        );
        conn.execute("UPDATE sync_ctl SET applying = 0", [])?;
        r?;
        Ok(())
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }
}

pub(crate) fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

pub(crate) fn from_json<T: DeserializeOwned>(s: Option<String>) -> Option<T> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

pub(crate) fn enum_str<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

pub(crate) fn parse_enum<T: DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_a_v1_database_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("old.db");
        {
            let conn = Connection::open(&p).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let db = Db::open(&p).unwrap();
        let v: i32 = db.conn().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
        assert!(db.list_roles().unwrap().is_empty(), "table exists, seeding happens in seed.rs");
    }

    #[test]
    fn device_id_is_stable_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pod.db");
        let a = Db::open(&p).unwrap().device_id().to_string();
        let b = Db::open(&p).unwrap().device_id().to_string();
        assert_eq!(a, b);
    }
}
