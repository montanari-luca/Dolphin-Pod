//! The sync half of the local database: capturing local edits as field patches, applying remote ones, and
//! the bookkeeping tables in between. Everything here is synchronous SQL; the engine drives it.

use std::sync::atomic::Ordering;

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde_json::{Map, Value};

use super::hlc::{self, MAX_DRIFT_MS, Stamp};
use super::kinds::{self, API_KEY_FIELD, KindDef};
use crate::db::Db;
use crate::error::Result;
use crate::util::now_ms;

pub type Fields = Map<String, Value>;

/// A decoded remote operation: a patch of fields for one record, stamped by its author.
#[derive(Debug, Clone)]
pub struct PlainOp {
    pub kind: String,
    pub id: String,
    pub hlc: String,
    pub device_id: String,
    pub patch: Fields,
}

/// A local edit, captured and waiting to be pushed.
#[derive(Debug, Clone)]
pub struct OutboxRow {
    pub seq: i64,
    pub op: PlainOp,
}

/// What applying a remote operation did, so the app can tell the UI.
#[derive(Debug, Clone)]
pub struct Applied {
    pub kind: String,
    pub id: String,
    pub created: bool,
    pub fields: Vec<String>,
    /// Keychain writes the apply asks for (name, value; None deletes).
    pub secrets: Vec<(String, Option<String>)>,
}

fn sql_to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => Value::from(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(_) => Value::Null,
    }
}

fn json_to_sql(v: &Value) -> Sql {
    match v {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(*b as i64),
        Value::Number(n) => n.as_i64().map(Sql::Integer).unwrap_or_else(|| Sql::Real(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => Sql::Text(s.clone()),
        other => Sql::Text(other.to_string()),
    }
}

fn read_fields(conn: &Connection, def: &KindDef, id: &str) -> rusqlite::Result<Option<Fields>> {
    let Some(parts) = def.split_id(id) else { return Ok(None) };
    let sql = format!("SELECT {} FROM {} WHERE {}", def.fields.join(", "), def.table, def.where_clause());
    conn.query_row(&sql, params_from_iter(parts.iter()), |r| {
        let mut m = Fields::new();
        for (i, f) in def.fields.iter().enumerate() {
            m.insert((*f).into(), sql_to_json(r.get_ref(i)?));
        }
        Ok(m)
    })
    .optional()
}

fn shadow_of(conn: &Connection, kind: &str, id: &str) -> rusqlite::Result<std::collections::HashMap<String, (String, String)>> {
    let mut st = conn.prepare("SELECT field, hlc, value FROM sync_shadow WHERE kind = ?1 AND id = ?2")?;
    let rows = st
        .query_map([kind, id], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, String>(1)?, r.get::<_, String>(2)?))))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn set_shadow(conn: &Connection, kind: &str, id: &str, field: &str, hlc: &str, value: &Value) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sync_shadow (kind, id, field, hlc, value) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (kind, id, field) DO UPDATE SET hlc = excluded.hlc, value = excluded.value",
        params![kind, id, field, hlc, value.to_string()],
    )?;
    Ok(())
}

/// Messages that are still being written stay on the device until they finish; remote-triggered ones are
/// the exception, since the phone is waiting to see them.
fn message_in_progress(fields: &Fields) -> bool {
    let streaming = matches!(fields.get("status").and_then(Value::as_str), Some("pending" | "streaming"));
    let remote = fields
        .get("meta")
        .and_then(Value::as_str)
        .and_then(|m| serde_json::from_str::<Value>(m).ok())
        .is_some_and(|m| m.get("remote").and_then(Value::as_bool).unwrap_or(false));
    streaming && !remote
}

impl Db {
    // ------------------------------------------------------------------ clock

    fn clock_now(&self) -> i64 {
        now_ms() + self.clock_skew_ms.load(Ordering::Relaxed)
    }

    /// Test hook: pretend this device's wall clock is off by `ms`.
    pub fn set_clock_skew_ms(&self, ms: i64) {
        self.clock_skew_ms.store(ms, Ordering::Relaxed);
    }

    fn last_stamp(conn: &Connection) -> Stamp {
        let raw: Option<String> =
            conn.query_row("SELECT value FROM meta WHERE key = 'hlc_last'", [], |r| r.get(0)).optional().ok().flatten();
        raw.and_then(|s| hlc::parse(&s)).unwrap_or_default()
    }

    fn store_stamp(conn: &Connection, s: Stamp) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('hlc_last', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [hlc::encode(s, "")],
        )?;
        Ok(())
    }

    /// A new clock value for a local event; `at_ms` is when the edit happened if it is only being captured now.
    fn tick(&self, conn: &Connection, at_ms: Option<i64>) -> rusqlite::Result<String> {
        let wall = self.clock_now();
        let at = at_ms.map_or(wall, |t| (t + self.clock_skew_ms.load(Ordering::Relaxed)).min(wall));
        let next = hlc::next(Self::last_stamp(conn), at);
        Self::store_stamp(conn, next)?;
        Ok(hlc::encode(next, &hlc::node_of(self.device_id())))
    }

    /// Fold a remote stamp into the local clock, so everything done next orders after it.
    fn observe(conn: &Connection, remote: Stamp) -> rusqlite::Result<()> {
        let last = Self::last_stamp(conn);
        if remote > last {
            Self::store_stamp(conn, remote)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ dirty tracking

    /// First sync on this device: every existing record has to be offered to the remote once.
    pub fn sync_mark_all_dirty(&self) -> Result<()> {
        let conn = self.conn();
        for def in kinds::KINDS {
            conn.execute(
                &format!("INSERT INTO sync_dirty (kind, id) SELECT '{}', {} FROM {} WHERE true ON CONFLICT (kind, id) DO NOTHING", def.kind, def.id_expr(), def.table),
                [],
            )?;
        }
        Ok(())
    }

    /// Forget which records changed locally. A device joining a pod does this before it reads the pod: its
    /// untouched defaults must not be stamped as fresh edits that then beat the pod's real values.
    pub fn sync_clear_dirty(&self) -> Result<()> {
        self.conn().execute("DELETE FROM sync_dirty", [])?;
        Ok(())
    }

    pub fn sync_dirty_provider_ids(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT id FROM sync_dirty WHERE kind = ?1")?;
        Ok(st.query_map([kinds::PROVIDER], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn sync_pending_count(&self) -> Result<i64> {
        let conn = self.conn();
        Ok(conn.query_row(
            "SELECT (SELECT COUNT(*) FROM sync_outbox) + (SELECT COUNT(*) FROM sync_dirty)",
            [],
            |r| r.get(0),
        )?)
    }

    /// Forget everything about a previous sync account (when sync is turned off or pointed elsewhere).
    pub fn sync_reset(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute_batch(
            "DELETE FROM sync_dirty; DELETE FROM sync_shadow; DELETE FROM sync_outbox; DELETE FROM sync_state; DELETE FROM sync_blobs;",
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------ capture

    /// Turn locally edited records into field patches. `virtuals` supplies fields that don't live in the
    /// table (the provider API key), keyed by record id. Returns how many patches were queued.
    pub fn sync_capture(&self, virtuals: &std::collections::HashMap<String, Vec<(String, Value)>>) -> Result<usize> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let dirty: Vec<(String, String)> = {
            let mut st = tx.prepare("SELECT kind, id FROM sync_dirty")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        let mut queued = 0;
        for (kind, id) in dirty {
            let Some(def) = kinds::def(&kind) else {
                tx.execute("DELETE FROM sync_dirty WHERE kind = ?1 AND id = ?2", [&kind, &id])?;
                continue;
            };
            if self.capture_one(&tx, def, &id, virtuals.get(&id).map(Vec::as_slice).unwrap_or(&[]))? {
                queued += 1;
            }
        }
        tx.commit()?;
        Ok(queued)
    }

    /// Capture one record. Returns true if a patch was queued. Leaves the record dirty if it isn't ready.
    fn capture_one(&self, conn: &Connection, def: &KindDef, id: &str, virtuals: &[(String, Value)]) -> Result<bool> {
        let finished = |conn: &Connection| conn.execute("DELETE FROM sync_dirty WHERE kind = ?1 AND id = ?2", [def.kind, id]);
        if def.kind == kinds::SETTING && kinds::setting_is_local(id) {
            finished(conn)?;
            return Ok(false);
        }
        let Some(mut current) = read_fields(conn, def, id)? else {
            finished(conn)?; // hard-deleted locally (tombstone purge); nothing to say about it
            return Ok(false);
        };
        if def.kind == kinds::MESSAGE && message_in_progress(&current) {
            return Ok(false);
        }
        if def.kind == kinds::ATTACHMENT && current.get("message_id").is_none_or(Value::is_null) {
            return Ok(false); // staged, not sent yet
        }
        for (k, v) in virtuals {
            current.insert(k.clone(), v.clone());
        }
        let shadow = shadow_of(conn, def.kind, id)?;
        let mut patch = Fields::new();
        for (field, value) in &current {
            let unchanged = shadow.get(field).is_some_and(|(_, old)| *old == value.to_string());
            if !unchanged {
                patch.insert(field.clone(), value.clone());
            }
        }
        finished(conn)?;
        if patch.is_empty() {
            return Ok(false);
        }
        let edited_at = conn
            .query_row(&format!("SELECT updated_at FROM {} WHERE {}", def.table, def.where_clause()), params_from_iter(def.split_id(id).unwrap_or_default().iter()), |r| r.get::<_, i64>(0))
            .ok();
        let stamp = self.tick(conn, edited_at)?;
        for (field, value) in &patch {
            set_shadow(conn, def.kind, id, field, &stamp, value)?;
        }
        conn.execute(
            "INSERT INTO sync_outbox (kind, id, hlc, patch, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![def.kind, id, stamp, Value::Object(patch).to_string(), now_ms()],
        )?;
        Ok(true)
    }

    // ------------------------------------------------------------------ outbox

    pub fn sync_outbox(&self, limit: usize) -> Result<Vec<OutboxRow>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT seq, kind, id, hlc, patch FROM sync_outbox ORDER BY seq LIMIT ?1")?;
        let device = self.device_id().to_string();
        let rows = st
            .query_map([limit as i64], |r| {
                let patch: String = r.get(4)?;
                Ok(OutboxRow {
                    seq: r.get(0)?,
                    op: PlainOp {
                        kind: r.get(1)?,
                        id: r.get(2)?,
                        hlc: r.get(3)?,
                        device_id: device.clone(),
                        patch: serde_json::from_str::<Value>(&patch).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default(),
                    },
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn sync_outbox_ack(&self, up_to_seq: i64) -> Result<()> {
        self.conn().execute("DELETE FROM sync_outbox WHERE seq <= ?1", [up_to_seq])?;
        Ok(())
    }

    // ------------------------------------------------------------------ cursor

    pub fn sync_cursor(&self, remote: &str) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT cursor FROM sync_state WHERE remote = ?1", [remote], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }

    pub fn sync_set_cursor(&self, remote: &str, cursor: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO sync_state (remote, cursor) VALUES (?1, ?2) ON CONFLICT (remote) DO UPDATE SET cursor = excluded.cursor",
            params![remote, cursor],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------ apply

    /// Merge a remote operation into the local record, field by field, newest clock wins. Idempotent and
    /// order-independent. `local_virtuals` are this device's current values of virtual fields for the record,
    /// so a pending local edit is captured before the remote one can overwrite it.
    pub fn sync_apply(&self, op: &PlainOp, local_virtuals: &[(String, Value)]) -> Result<Option<Applied>> {
        let Some(def) = kinds::def(&op.kind) else { return Ok(None) };
        let Some(stamp) = hlc::parse(&op.hlc) else { return Ok(None) };
        if def.split_id(&op.id).is_none() || (def.kind == kinds::SETTING && kinds::setting_is_local(&op.id)) {
            return Ok(None);
        }
        if stamp.phys > self.clock_now() + MAX_DRIFT_MS {
            tracing::warn!("refusing a sync operation stamped far in the future ({})", op.hlc);
            return Ok(None);
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        // A local edit that hasn't been captured yet must get its own clock value before the remote one lands.
        let is_dirty: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_dirty WHERE kind = ?1 AND id = ?2)",
            [&op.kind, &op.id],
            |r| r.get(0),
        )?;
        if is_dirty {
            self.capture_one(&tx, def, &op.id, local_virtuals)?;
        }
        Self::observe(&tx, stamp)?;

        let shadow = shadow_of(&tx, &op.kind, &op.id)?;
        let existing = read_fields(&tx, def, &op.id)?;
        let tombstoning = op.patch.get("deleted").and_then(Value::as_i64) == Some(1);
        let mut accepted: Vec<(&String, &Value)> = Vec::new();
        for (field, value) in &op.patch {
            let known = field == API_KEY_FIELD && def.kind == kinds::PROVIDER || def.fields.contains(&field.as_str());
            if !known {
                continue;
            }
            if shadow.get(field).is_some_and(|(have, _)| have.as_str() >= op.hlc.as_str()) {
                continue; // we already know something at least as new
            }
            // A finished message's text never changes (apart from being cleaned away).
            if def.kind == kinds::MESSAGE
                && !tombstoning
                && matches!(field.as_str(), "content" | "reasoning")
                && existing.as_ref().is_some_and(|e| e.get("status").and_then(Value::as_str) == Some("done"))
            {
                continue;
            }
            accepted.push((field, value));
        }
        if accepted.is_empty() {
            tx.commit()?;
            return Ok(None);
        }

        tx.execute("UPDATE sync_ctl SET applying = 1", [])?;
        let table_fields: Vec<(&String, &Value)> = accepted.iter().copied().filter(|(f, _)| def.fields.contains(&f.as_str())).collect();
        let created = existing.is_none();
        let parts = def.split_id(&op.id).unwrap_or_default();
        let now = now_ms();
        let cols = table_columns(&tx, def.table)?;
        let has = |c: &str| cols.iter().any(|(n, ..)| n == c);
        let is_key = |c: &str| def.keys.contains(&c);

        if created {
            let mut names: Vec<String> = def.keys.iter().map(|k| (*k).to_string()).collect();
            let mut values: Vec<Sql> = parts.iter().map(|p| Sql::Text((*p).into())).collect();
            for (f, v) in &table_fields {
                names.push((*f).clone());
                values.push(json_to_sql(v));
            }
            if has("updated_at") && !is_key("updated_at") {
                names.push("updated_at".into());
                values.push(Sql::Integer(now));
            }
            if has("device_id") && !is_key("device_id") {
                names.push("device_id".into());
                values.push(Sql::Text(op.device_id.clone()));
            }
            // Columns the operation doesn't cover yet get neutral defaults; later operations fill them in.
            for (name, ty, notnull, dflt) in &cols {
                if names.contains(name) || !*notnull || dflt.is_some() {
                    continue;
                }
                names.push(name.clone());
                values.push(if ty.to_uppercase().contains("INT") { Sql::Integer(0) } else { Sql::Text(String::new()) });
            }
            let placeholders = (1..=names.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
            let sql = format!("INSERT INTO {} ({}) VALUES ({})", def.table, names.join(", "), placeholders);
            let mut result = tx.execute(&sql, params_from_iter(values.iter()));
            // Two devices that each made an agent with the same slug: keep both, rename the newcomer.
            if def.kind == "agent"
                && result.is_err()
                && let Some(slug_at) = names.iter().position(|n| n == "slug")
                && let Sql::Text(slug) = &values[slug_at]
            {
                let tail: String = op.id.chars().filter(|c| c.is_ascii_alphanumeric()).take(6).collect();
                values[slug_at] = Sql::Text(format!("{slug}-{tail}"));
                result = tx.execute(&sql, params_from_iter(values.iter()));
            }
            result?;
        } else if !table_fields.is_empty() {
            let mut sets: Vec<String> = Vec::new();
            let mut values: Vec<Sql> = Vec::new();
            for (f, v) in &table_fields {
                values.push(json_to_sql(v));
                sets.push(format!("{f} = ?{}", values.len()));
            }
            if has("updated_at") && !is_key("updated_at") {
                values.push(Sql::Integer(now));
                sets.push(format!("updated_at = ?{}", values.len()));
            }
            if has("device_id") && !is_key("device_id") {
                values.push(Sql::Text(op.device_id.clone()));
                sets.push(format!("device_id = ?{}", values.len()));
            }
            let mut where_parts = Vec::new();
            for (i, k) in def.keys.iter().enumerate() {
                values.push(Sql::Text(parts[i].into()));
                where_parts.push(format!("{k} = ?{}", values.len()));
            }
            tx.execute(
                &format!("UPDATE {} SET {} WHERE {}", def.table, sets.join(", "), where_parts.join(" AND ")),
                params_from_iter(values.iter()),
            )?;
        }
        for (field, value) in &accepted {
            set_shadow(&tx, &op.kind, &op.id, field, &op.hlc, value)?;
        }
        tx.execute("UPDATE sync_ctl SET applying = 0", [])?;

        let mut secrets = Vec::new();
        if def.kind == kinds::PROVIDER {
            let name = crate::secrets::provider_key_name(&op.id);
            if let Some((_, v)) = accepted.iter().find(|(f, _)| f.as_str() == API_KEY_FIELD) {
                secrets.push((name.clone(), v.as_str().filter(|s| !s.is_empty()).map(String::from)));
            }
            if tombstoning {
                secrets.push((name, None));
            }
        }
        let fields = accepted.iter().map(|(f, _)| (*f).clone()).collect();
        tx.commit()?;
        Ok(Some(Applied { kind: op.kind.clone(), id: op.id.clone(), created, fields, secrets }))
    }
}

/// (name, declared type, not null, default) for each column.
fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<(String, String, bool, Option<String>)>> {
    let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)? != 0, r.get::<_, Option<String>>(4)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

impl Db {
    /// Every synced record and its synced fields, for comparing two devices.
    pub fn sync_snapshot(&self) -> Result<std::collections::BTreeMap<(String, String), Fields>> {
        let conn = self.conn();
        let mut out = std::collections::BTreeMap::new();
        for def in kinds::KINDS {
            let ids: Vec<String> = {
                let mut st = conn.prepare(&format!("SELECT {} FROM {}", def.id_expr(), def.table))?;
                st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
            };
            for id in ids {
                if def.kind == kinds::SETTING && kinds::setting_is_local(&id) {
                    continue;
                }
                if let Some(fields) = read_fields(&conn, def, &id)? {
                    out.insert((def.kind.to_string(), id), fields);
                }
            }
        }
        Ok(out)
    }
}
