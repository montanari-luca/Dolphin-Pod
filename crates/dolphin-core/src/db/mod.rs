//! Local-first SQLite store. Each device keeps its own database as the source of truth.

mod agents;
mod chats;
mod messages;
mod misc;
mod providers;
mod roles;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::Result;
use crate::util::new_id;

/// Ordered migrations; index + 1 is the schema version it produces. Only ever append.
const MIGRATIONS: &[&str] = &[include_str!("schema.sql"), include_str!("migration_0002_user_roles.sql")];

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    device_id: Arc<str>,
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
        Ok(Self { conn: Arc::new(Mutex::new(conn)), device_id: device_id.into() })
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        // A poisoned lock only means another thread panicked mid-query; SQLite itself is still consistent.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
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
