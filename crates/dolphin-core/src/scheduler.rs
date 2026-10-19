//! Background jobs, all programmatic (no model involved): rolling chat cleanup, automations,
//! tombstone purging and orphaned-attachment removal. Ticks every 30 seconds.

use std::time::Duration;

use chrono::{Datelike, Local, NaiveTime, Timelike};
use serde_json::json;

use crate::core::CoreRef;
use crate::events::CoreEvent;
use crate::model::{Automation, MessageKind};
use crate::router::ack;
use crate::settings::{self, keys};
use crate::util::now_ms;

const HOUR_MS: i64 = 3_600_000;
/// An automation fires only if the app is running within this window after its time.
const FIRE_WINDOW_MIN: i64 = 5;

pub fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

/// Whether `a` should fire now. Pure, for testing.
pub fn automation_due(a: &Automation, now: chrono::NaiveDateTime) -> bool {
    if !a.enabled {
        return false;
    }
    let Some(t) = parse_time(&a.time_of_day) else { return false };
    let today = now.date().format("%Y-%m-%d").to_string();
    if a.last_run_date.as_deref() == Some(today.as_str()) {
        return false;
    }
    let weekday = now.weekday().num_days_from_monday() as u8;
    if !a.weekdays.is_empty() && !a.weekdays.contains(&weekday) {
        return false;
    }
    let mins_now = (now.hour() * 60 + now.minute()) as i64;
    let mins_at = (t.hour() * 60 + t.minute()) as i64;
    mins_now >= mins_at && mins_now < mins_at + FIRE_WINDOW_MIN
}

pub fn spawn(core: CoreRef) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        let mut last_maintenance = 0i64;
        loop {
            tick.tick().await;
            if let Err(e) = cleanup(&core) {
                tracing::warn!("cleanup failed: {e}");
            }
            run_automations(&core).await;
            if now_ms() - last_maintenance > 6 * HOUR_MS {
                last_maintenance = now_ms();
                maintenance(&core);
            }
        }
    });
}

/// Rolling cleanup: remove messages older than each chat's TTL, then replace Roger's cleanup notice.
pub fn cleanup(core: &CoreRef) -> crate::Result<()> {
    if !settings::get_bool(&core.db, keys::CLEANUP_ENABLED) {
        return Ok(());
    }
    let roger = core.db.agent_by_slug("roger")?;
    for chat in core.db.list_chats()? {
        if !chat.cleanup_enabled {
            continue;
        }
        let cutoff = now_ms() - chat.cleanup_ttl_hours as i64 * HOUR_MS;
        let removed = core.db.tombstone_messages_before(&chat.id, cutoff)?;
        if removed.is_empty() {
            continue;
        }
        let mut ids = removed;
        // Only one cleanup notice per chat: the previous one is replaced.
        if let Some(prev) = core.db.find_notice(&chat.id, "cleanup")? {
            core.db.tombstone_message(&prev)?;
            ids.push(prev);
        }
        core.emit(CoreEvent::MessagesRemoved { chat_id: chat.id.clone(), message_ids: ids, all: false });
        let text = ack::CLEANED.replace("{hours}", &chat.cleanup_ttl_hours.to_string());
        core.post(&chat.id, &roger.id, MessageKind::Notice, &text, json!({ "notice": "cleanup" }))?;
    }
    Ok(())
}

async fn run_automations(core: &CoreRef) {
    let Ok(list) = core.db.list_automations() else { return };
    let now = Local::now().naive_local();
    for mut a in list {
        if !automation_due(&a, now) {
            continue;
        }
        a.last_run_date = Some(now.date().format("%Y-%m-%d").to_string());
        if core.db.upsert_automation(&a).is_err() {
            continue;
        }
        // The scheduled message enters the normal pipeline, exactly as if the user had sent it.
        if let Err(e) = core.send_message_with_meta(&a.chat_id, &a.text, vec![], json!({ "automation": a.id })).await {
            tracing::warn!("automation {} failed: {e}", a.id);
        }
    }
}

fn maintenance(core: &CoreRef) {
    let _ = core.db.purge_tombstones(now_ms() - 30 * 24 * HOUR_MS);
    if let Ok(orphans) = core.db.orphaned_attachment_paths(now_ms() - 24 * HOUR_MS) {
        for (id, path) in orphans {
            let _ = std::fs::remove_file(&path);
            let _ = core.db.tombstone_attachment(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn auto(time: &str, weekdays: Vec<u8>) -> Automation {
        Automation { id: "a".into(), chat_id: "c".into(), text: "hi".into(), time_of_day: time.into(), weekdays, enabled: true, last_run_date: None, updated_at: 0 }
    }

    #[test]
    fn automation_window_and_weekdays() {
        // 2026-10-19 is a Monday.
        let at = |h, m| NaiveDate::from_ymd_opt(2026, 10, 19).unwrap().and_hms_opt(h, m, 0).unwrap();
        let a = auto("09:00", vec![]);
        assert!(!automation_due(&a, at(8, 59)));
        assert!(automation_due(&a, at(9, 0)));
        assert!(automation_due(&a, at(9, 4)));
        assert!(!automation_due(&a, at(9, 5)), "missed while the device was off: no catch-up");
        let mut ran = a.clone();
        ran.last_run_date = Some("2026-10-19".into());
        assert!(!automation_due(&ran, at(9, 1)));
        assert!(!automation_due(&auto("09:00", vec![2, 4]), at(9, 1)), "Wednesday/Friday only");
        assert!(automation_due(&auto("09:00", vec![0]), at(9, 1)));
    }

    #[test]
    fn rolling_cleanup_removes_only_old_messages() {
        use crate::db::Db;
        use crate::secrets::MemoryStore;
        let dir = tempfile::tempdir().unwrap();
        let core = crate::core::Core::with_db(Db::open_in_memory().unwrap(), dir.path(), std::sync::Arc::new(MemoryStore::default())).unwrap();
        let chat = core.list_chats().unwrap().remove(0);
        let mut old = core.new_message(&chat.id, None, MessageKind::Text, crate::model::MessageStatus::Done, "old");
        old.created_at = now_ms() - 25 * HOUR_MS;
        core.db.insert_message(&old).unwrap();
        let fresh = core.new_message(&chat.id, None, MessageKind::Text, crate::model::MessageStatus::Done, "fresh");
        core.db.insert_message(&fresh).unwrap();
        cleanup(&core).unwrap();
        let left: Vec<String> = core.list_messages(&chat.id, 50).unwrap().into_iter().map(|m| m.content).collect();
        assert_eq!(left.len(), 2, "fresh message + one cleanup notice");
        assert!(left.contains(&"fresh".to_string()));
        assert!(left.iter().any(|c| c.starts_with("🧹")));
        // A second sweep with nothing to remove does not post another notice.
        cleanup(&core).unwrap();
        assert_eq!(core.list_messages(&chat.id, 50).unwrap().len(), 2);
        // Per-chat opt-out is respected.
        let mut c = core.db.get_chat(&chat.id).unwrap();
        c.cleanup_enabled = false;
        core.db.upsert_chat(&c).unwrap();
        let mut old2 = core.new_message(&chat.id, None, MessageKind::Text, crate::model::MessageStatus::Done, "old2");
        old2.created_at = now_ms() - 48 * HOUR_MS;
        core.db.insert_message(&old2).unwrap();
        cleanup(&core).unwrap();
        assert!(core.list_messages(&chat.id, 50).unwrap().iter().any(|m| m.content == "old2"));
    }
}
