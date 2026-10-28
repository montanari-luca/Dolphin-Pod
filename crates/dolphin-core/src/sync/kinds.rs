//! Which tables sync, under which record kind, and which of their columns travel.
//!
//! Left out on purpose: `permission_grants` and `action_log` (trust decisions and audit trails stay on the
//! device that made them), and device-local columns such as a chat's `workspace_path` or an attachment's
//! file `path`.

pub struct KindDef {
    pub kind: &'static str,
    pub table: &'static str,
    /// Primary-key columns; a composite key becomes the record id joined with `|`.
    pub keys: &'static [&'static str],
    /// Columns that sync, each as its own last-writer-wins field.
    pub fields: &'static [&'static str],
    pub has_deleted: bool,
}

pub const PROVIDER: &str = "provider";
pub const MESSAGE: &str = "message";
pub const ATTACHMENT: &str = "attachment";
pub const SETTING: &str = "setting";

/// The provider's API key travels as a virtual field: it lives in the keychain, not in the table.
pub const API_KEY_FIELD: &str = "api_key";

pub const KINDS: &[KindDef] = &[
    KindDef {
        kind: PROVIDER,
        table: "provider_configs",
        keys: &["id"],
        fields: &["kind", "name", "base_url", "extra", "enabled", "deleted"],
        has_deleted: true,
    },
    KindDef {
        kind: "agent",
        table: "agents",
        keys: &["id"],
        fields: &["slug", "name", "role", "specialty", "job", "color", "model", "image_model", "ios_model", "persona", "learning", "sort", "deleted"],
        has_deleted: true,
    },
    KindDef {
        kind: "chat",
        table: "chats",
        keys: &["id"],
        fields: &["name", "icon", "sort", "cleanup_enabled", "cleanup_ttl_hours", "created_at", "deleted"],
        has_deleted: true,
    },
    KindDef { kind: "chat_agent", table: "chat_agents", keys: &["chat_id", "agent_id"], fields: &["deleted"], has_deleted: true },
    KindDef {
        kind: MESSAGE,
        table: "messages",
        keys: &["id"],
        fields: &[
            "chat_id", "agent_id", "kind", "status", "content", "reasoning", "error", "mentions", "meta", "thinking_ms",
            "working_ms", "tokens_in", "tokens_out", "created_at", "deleted",
        ],
        has_deleted: true,
    },
    KindDef {
        kind: ATTACHMENT,
        table: "attachments",
        keys: &["id"],
        fields: &["chat_id", "message_id", "name", "mime", "kind", "size", "created_at", "deleted"],
        has_deleted: true,
    },
    KindDef { kind: SETTING, table: "settings", keys: &["key"], fields: &["value"], has_deleted: false },
    KindDef {
        kind: "usage",
        table: "usage",
        keys: &["agent_id", "device_id"],
        fields: &["tokens_in", "tokens_out", "thinking_ms", "working_ms", "requests", "last_query_ms", "last_query_at"],
        has_deleted: false,
    },
    KindDef {
        kind: "role",
        table: "user_roles",
        keys: &["id"],
        fields: &["name", "icon", "color", "sort", "deleted"],
        has_deleted: true,
    },
    KindDef {
        kind: "agent_memory",
        table: "agent_memory",
        keys: &["id"],
        fields: &["agent_id", "kind", "text", "project", "source", "pinned", "created_at", "deleted"],
        has_deleted: true,
    },
    KindDef {
        kind: "automation",
        table: "automations",
        keys: &["id"],
        fields: &["chat_id", "text", "time_of_day", "weekdays", "enabled", "last_run_date", "runner_device_id", "deleted"],
        has_deleted: true,
    },
];

pub fn def(kind: &str) -> Option<&'static KindDef> {
    KINDS.iter().find(|k| k.kind == kind)
}

impl KindDef {
    pub fn split_id<'a>(&self, id: &'a str) -> Option<Vec<&'a str>> {
        let parts: Vec<&str> = id.split('|').collect();
        (parts.len() == self.keys.len()).then_some(parts)
    }

    pub fn where_clause(&self) -> String {
        self.keys.iter().enumerate().map(|(i, k)| format!("{k} = ?{}", i + 1)).collect::<Vec<_>>().join(" AND ")
    }

    /// SQL expression producing this kind's record id from a row.
    pub fn id_expr(&self) -> String {
        self.keys.join(" || '|' || ")
    }
}

/// Settings that never leave the device (reserved prefixes for per-device configuration).
pub fn setting_is_local(key: &str) -> bool {
    key.starts_with("sync.") || key.starts_with("remote.") || key.starts_with("device.")
}
