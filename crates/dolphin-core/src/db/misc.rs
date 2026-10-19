//! Settings, usage counters, permission grants, action log and automations.

use rusqlite::{OptionalExtension, Row, params};

use super::{Db, enum_str, from_json, parse_enum, to_json};
use crate::error::{CoreError, Result};
use crate::model::{ActionLogEntry, Automation, PermissionGrant, PermissionLevel, UsageStats};
use crate::util::{new_id, now_ms};

impl Db {
    // ---------------------------------------------------------------- settings (raw JSON values)

    pub fn setting_raw(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let v: Option<String> = self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        Ok(from_json(v))
    }

    pub fn set_setting_raw(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings (key, value, updated_at, device_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, device_id = excluded.device_id",
            params![key, value.to_string(), now_ms(), self.device_id()],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- usage

    pub fn record_usage(
        &self,
        agent_id: &str,
        tokens_in: i64,
        tokens_out: i64,
        thinking_ms: i64,
        working_ms: i64,
    ) -> Result<()> {
        let now = now_ms();
        self.conn().execute(
            "INSERT INTO usage (agent_id, device_id, tokens_in, tokens_out, thinking_ms, working_ms, requests, last_query_ms, last_query_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?6, ?7, ?7)
             ON CONFLICT(agent_id, device_id) DO UPDATE SET
               tokens_in = tokens_in + excluded.tokens_in, tokens_out = tokens_out + excluded.tokens_out,
               thinking_ms = thinking_ms + excluded.thinking_ms, working_ms = working_ms + excluded.working_ms,
               requests = requests + 1, last_query_ms = excluded.last_query_ms, last_query_at = excluded.last_query_at,
               updated_at = excluded.updated_at",
            params![agent_id, self.device_id(), tokens_in, tokens_out, thinking_ms, working_ms, now],
        )?;
        Ok(())
    }

    /// All-time usage per agent, summed over every device.
    pub fn usage_all(&self) -> Result<Vec<UsageStats>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT u.agent_id, SUM(u.tokens_in), SUM(u.tokens_out), SUM(u.thinking_ms), SUM(u.working_ms), SUM(u.requests),
                (SELECT last_query_ms FROM usage x WHERE x.agent_id = u.agent_id ORDER BY last_query_at DESC LIMIT 1),
                MAX(u.last_query_at)
             FROM usage u GROUP BY u.agent_id",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(UsageStats {
                    agent_id: r.get(0)?,
                    tokens_in: r.get(1)?,
                    tokens_out: r.get(2)?,
                    thinking_ms: r.get(3)?,
                    working_ms: r.get(4)?,
                    requests: r.get(5)?,
                    last_query_ms: r.get(6)?,
                    last_query_at: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn usage_for(&self, agent_id: &str) -> Result<UsageStats> {
        Ok(self
            .usage_all()?
            .into_iter()
            .find(|u| u.agent_id == agent_id)
            .unwrap_or(UsageStats { agent_id: agent_id.into(), ..Default::default() }))
    }

    // ---------------------------------------------------------------- permission grants

    pub fn grant(&self, chat_id: &str, agent_id: &str) -> Result<PermissionGrant> {
        let g = self
            .conn()
            .query_row(
                "SELECT chat_id, agent_id, level, reason, updated_at FROM permission_grants WHERE chat_id = ?1 AND agent_id = ?2",
                params![chat_id, agent_id],
                |r| {
                    Ok(PermissionGrant {
                        chat_id: r.get(0)?,
                        agent_id: r.get(1)?,
                        level: parse_enum(&r.get::<_, String>(2)?).unwrap_or(PermissionLevel::ReadOnly),
                        reason: r.get(3)?,
                        updated_at: r.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(g.unwrap_or(PermissionGrant {
            chat_id: chat_id.into(),
            agent_id: agent_id.into(),
            level: PermissionLevel::ReadOnly,
            reason: "default".into(),
            updated_at: 0,
        }))
    }

    pub fn grants_for_chat(&self, chat_id: &str) -> Result<Vec<PermissionGrant>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT chat_id, agent_id, level, reason, updated_at FROM permission_grants WHERE chat_id = ?1",
        )?;
        let rows = st
            .query_map([chat_id], |r| {
                Ok(PermissionGrant {
                    chat_id: r.get(0)?,
                    agent_id: r.get(1)?,
                    level: parse_enum(&r.get::<_, String>(2)?).unwrap_or(PermissionLevel::ReadOnly),
                    reason: r.get(3)?,
                    updated_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_grant(
        &self,
        chat_id: &str,
        agent_id: &str,
        level: PermissionLevel,
        reason: &str,
    ) -> Result<PermissionGrant> {
        self.conn().execute(
            "INSERT INTO permission_grants (chat_id, agent_id, level, reason, updated_at, device_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(chat_id, agent_id) DO UPDATE SET level = excluded.level, reason = excluded.reason,
               updated_at = excluded.updated_at, device_id = excluded.device_id",
            params![chat_id, agent_id, enum_str(&level), reason, now_ms(), self.device_id()],
        )?;
        self.grant(chat_id, agent_id)
    }

    // ---------------------------------------------------------------- action log

    pub fn log_action(
        &self,
        chat_id: &str,
        agent_id: &str,
        tool: &str,
        summary: &str,
        allowed: bool,
        detail: &str,
    ) -> Result<ActionLogEntry> {
        let e = ActionLogEntry {
            id: new_id(),
            chat_id: chat_id.into(),
            agent_id: agent_id.into(),
            tool: tool.into(),
            summary: summary.into(),
            allowed,
            detail: detail.into(),
            created_at: now_ms(),
        };
        self.conn().execute(
            "INSERT INTO action_log (id, chat_id, agent_id, tool, summary, allowed, detail, created_at, device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![e.id, e.chat_id, e.agent_id, e.tool, e.summary, e.allowed as i64, e.detail, e.created_at, self.device_id()],
        )?;
        Ok(e)
    }

    pub fn action_log(&self, chat_id: &str, limit: usize) -> Result<Vec<ActionLogEntry>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, chat_id, agent_id, tool, summary, allowed, detail, created_at FROM action_log
             WHERE chat_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = st
            .query_map(params![chat_id, limit as i64], |r| {
                Ok(ActionLogEntry {
                    id: r.get(0)?,
                    chat_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    tool: r.get(3)?,
                    summary: r.get(4)?,
                    allowed: r.get::<_, i64>(5)? != 0,
                    detail: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---------------------------------------------------------------- automations

    pub fn list_automations(&self) -> Result<Vec<Automation>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, chat_id, text, time_of_day, weekdays, enabled, last_run_date, updated_at FROM automations
             WHERE deleted = 0 ORDER BY time_of_day",
        )?;
        let rows = st.query_map([], row_to_automation)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn upsert_automation(&self, a: &Automation) -> Result<Automation> {
        let mut a = a.clone();
        a.updated_at = now_ms();
        self.conn().execute(
            "INSERT INTO automations (id, chat_id, text, time_of_day, weekdays, enabled, last_run_date, runner_device_id, updated_at, device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?8)
             ON CONFLICT(id) DO UPDATE SET chat_id = excluded.chat_id, text = excluded.text, time_of_day = excluded.time_of_day,
               weekdays = excluded.weekdays, enabled = excluded.enabled, last_run_date = excluded.last_run_date,
               updated_at = excluded.updated_at, device_id = excluded.device_id",
            params![
                a.id,
                a.chat_id,
                a.text,
                a.time_of_day,
                to_json(&a.weekdays),
                a.enabled as i64,
                a.last_run_date,
                self.device_id(),
                a.updated_at
            ],
        )?;
        Ok(a)
    }

    pub fn delete_automation(&self, id: &str) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE automations SET deleted = 1, updated_at = ?2, device_id = ?3 WHERE id = ?1",
            params![id, now_ms(), self.device_id()],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("automation {id}")));
        }
        Ok(())
    }
}

fn row_to_automation(r: &Row) -> rusqlite::Result<Automation> {
    Ok(Automation {
        id: r.get(0)?,
        chat_id: r.get(1)?,
        text: r.get(2)?,
        time_of_day: r.get(3)?,
        weekdays: from_json(r.get(4)?).unwrap_or_default(),
        enabled: r.get::<_, i64>(5)? != 0,
        last_run_date: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_accumulates_per_device() {
        let db = Db::open_in_memory().unwrap();
        db.record_usage("a", 10, 20, 100, 400).unwrap();
        db.record_usage("a", 1, 2, 10, 40).unwrap();
        let u = db.usage_for("a").unwrap();
        assert_eq!((u.tokens_in, u.tokens_out, u.thinking_ms, u.working_ms, u.requests), (11, 22, 110, 440, 2));
        assert_eq!(u.last_query_ms, Some(40));
    }

    #[test]
    fn grants_default_to_read_only() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.grant("c", "a").unwrap().level, PermissionLevel::ReadOnly);
        db.set_grant("c", "a", PermissionLevel::Full, "plan approved").unwrap();
        assert_eq!(db.grant("c", "a").unwrap().level, PermissionLevel::Full);
    }
}
