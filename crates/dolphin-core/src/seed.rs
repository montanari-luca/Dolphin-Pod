//! First-run defaults: the fourteen default agents and five default chats.
//! Agents are ordinary records, so custom agents can be added later without a migration.

use crate::db::Db;
use crate::error::Result;
use crate::model::{Agent, AgentColor, AgentRole, Chat, UserRole};
use crate::util::{now_ms, stable_id};

pub struct AgentSeed {
    pub slug: &'static str,
    pub name: &'static str,
    pub role: AgentRole,
    pub specialty: &'static str,
    pub job: &'static str,
    pub color: fn() -> AgentColor,
}

pub const AGENTS: &[AgentSeed] = &[
    AgentSeed {
        slug: "roger",
        name: "Roger",
        role: AgentRole::Moderator,
        specialty: "moderator",
        job: "Moderator. Keeps order in the pod, answers general questions first, routes your requests to the right specialist and explains what is going on.",
        color: || AgentColor::gradient(135.0, &[("#3d8bfd", 0.0), ("#a77bdb", 100.0)]),
    },
    AgentSeed {
        slug: "lincoln",
        name: "Lincoln",
        role: AgentRole::Orchestrator,
        specialty: "orchestrator",
        job: "Orchestrator. Works behind the scenes: turns Roger's decisions into precise briefs, directs the specialists and enforces permissions.",
        color: || AgentColor::gradient(135.0, &[("#4f46e5", 0.0), ("#0ea5e9", 100.0)]),
    },
    AgentSeed {
        slug: "rick",
        name: "Rick",
        role: AgentRole::Specialist,
        specialty: "coder",
        job: "Coding specialist. Writes, refactors and reviews code in any language.",
        color: || AgentColor::gradient(135.0, &[("#2563eb", 0.0), ("#22d3ee", 100.0)]),
    },
    AgentSeed {
        slug: "micheal",
        name: "Micheal",
        role: AgentRole::Specialist,
        specialty: "long_horizon",
        job: "Long-horizon agent. Plans and carries out big, multi-step projects end to end.",
        color: || AgentColor::gradient(135.0, &[("#7c3aed", 0.0), ("#f59e0b", 100.0)]),
    },
    AgentSeed {
        slug: "viktor",
        name: "Viktor",
        role: AgentRole::Specialist,
        specialty: "security",
        job: "Security and cyber specialist. Threat models, reviews code for vulnerabilities and hardens systems.",
        color: || AgentColor::gradient(135.0, &[("#dc2626", 0.0), ("#7f1d1d", 100.0)]),
    },
    AgentSeed {
        slug: "jordan",
        name: "Jordan",
        role: AgentRole::Specialist,
        specialty: "debug",
        job: "Debug specialist. Reads stack traces and logs, reproduces bugs and finds root causes.",
        color: || AgentColor::gradient(135.0, &[("#f97316", 0.0), ("#facc15", 100.0)]),
    },
    AgentSeed {
        slug: "emma",
        name: "Emma",
        role: AgentRole::Specialist,
        specialty: "creative",
        job: "Creative writing specialist. Stories, poems, scripts, copy and brainstorming.",
        color: || AgentColor::gradient(135.0, &[("#ec4899", 0.0), ("#f472b6", 100.0)]),
    },
    AgentSeed {
        slug: "ducky",
        name: "Ducky",
        role: AgentRole::Specialist,
        specialty: "docs",
        job: "Docs specialist. Writes READMEs, API references, guides and clear explanations of code.",
        color: || AgentColor::gradient(135.0, &[("#eab308", 0.0), ("#fde047", 100.0)]),
    },
    AgentSeed {
        slug: "sarah",
        name: "Sarah",
        role: AgentRole::Specialist,
        specialty: "image",
        job: "Image specialist. Turns your ideas into detailed image prompts and generates the images.",
        color: || AgentColor::gradient(135.0, &[("#f7826f", 0.0), ("#fbb36a", 100.0)]),
    },
    AgentSeed {
        slug: "prof",
        name: "Prof",
        role: AgentRole::Specialist,
        specialty: "teacher",
        job: "Professor. Helps you learn anything step by step, with examples and checks for understanding.",
        color: || AgentColor::gradient(135.0, &[("#10b981", 0.0), ("#14b8a6", 100.0)]),
    },
    AgentSeed {
        slug: "ruby",
        name: "Ruby",
        role: AgentRole::Specialist,
        specialty: "fun_facts",
        job: "Fun facts agent. Tells you surprising, true fun facts whenever you ask for one.",
        color: || AgentColor::gradient(135.0, &[("#e11d48", 0.0), ("#fb7185", 100.0)]),
    },
    AgentSeed {
        slug: "amelia",
        name: "Amelia",
        role: AgentRole::Specialist,
        specialty: "verifier",
        job: "Verification agent, called only when you explicitly ask her to double-check work, with evidence for every verdict. She checks what the other agents did and reports what holds up and what does not.",
        color: || AgentColor::gradient(135.0, &[("#0d9488", 0.0), ("#2dd4bf", 100.0)]),
    },
    AgentSeed {
        slug: "luna",
        name: "Luna",
        role: AgentRole::Specialist,
        specialty: "researcher",
        job: "Research agent. Runs many web searches in parallel, validates and cross-checks the sources, checks dates and reports the final findings with citations.",
        color: || AgentColor::gradient(135.0, &[("#4338ca", 0.0), ("#818cf8", 100.0)]),
    },
    AgentSeed {
        slug: "robert",
        name: "Robert",
        role: AgentRole::Specialist,
        specialty: "git",
        job: "Git manager. Tracks history and handles status, commit, push, pull and fetch, usually on behalf of the coding agents. Asks you before anything that touches a remote or rewrites history.",
        color: || AgentColor::gradient(135.0, &[("#ea580c", 0.0), ("#f59e0b", 100.0)]),
    },
];

/// Slugs added after the first release. Existing pods get them once, through `seed_new_agents_if_needed`.
const AGENTS_V2: &[&str] = &["ruby", "amelia", "luna", "robert"];

pub struct ChatSeed {
    pub name: &'static str,
    pub icon: &'static str,
    /// Specialist slugs; Roger and Lincoln are always added.
    pub agents: &'static [&'static str],
}

pub const CHATS: &[ChatSeed] = &[
    // Prof is in Coding too, so "how do I write X in Java" can go to him (the learning flow).
    ChatSeed { name: "Coding", icon: "code", agents: &["rick", "jordan", "ducky", "viktor", "prof", "robert", "amelia"] },
    ChatSeed {
        name: "General",
        icon: "message-circle",
        agents: &["rick", "micheal", "viktor", "jordan", "emma", "ducky", "sarah", "prof", "ruby", "amelia", "luna", "robert"],
    },
    ChatSeed { name: "Long Horizon Agentic", icon: "rocket", agents: &["micheal", "rick", "jordan", "robert", "amelia", "luna"] },
    ChatSeed { name: "Learning", icon: "graduation-cap", agents: &["prof", "ducky", "ruby", "luna"] },
    ChatSeed { name: "Creative", icon: "palette", agents: &["emma", "sarah", "ruby"] },
];

pub fn agent_seed_id(slug: &str) -> String {
    stable_id(&format!("agent:{slug}"))
}

pub fn chat_seed_id(name: &str) -> String {
    stable_id(&format!("chat:{name}"))
}

pub fn master_role_id() -> String {
    stable_id("role:master")
}

pub fn seed_if_needed(db: &Db) -> Result<bool> {
    if db.meta_get("seeded")?.is_some() {
        return Ok(false);
    }
    let now = now_ms();
    let mut ids = std::collections::HashMap::new();
    for (i, s) in AGENTS.iter().enumerate() {
        let a = Agent {
            id: agent_seed_id(s.slug),
            slug: s.slug.into(),
            name: s.name.into(),
            role: s.role,
            specialty: s.specialty.into(),
            job: s.job.into(),
            color: (s.color)(),
            model: None,
            image_model: None,
            ios_model: None,
            persona: None,
            sort: i as i32,
            updated_at: now,
        };
        db.upsert_agent(&a)?;
        ids.insert(s.slug, a.id);
    }
    for (i, c) in CHATS.iter().enumerate() {
        let mut agent_ids = vec![ids["roger"].clone(), ids["lincoln"].clone()];
        agent_ids.extend(c.agents.iter().map(|s| ids[s].clone()));
        db.upsert_chat(&Chat {
            id: chat_seed_id(c.name),
            name: c.name.into(),
            icon: c.icon.into(),
            sort: i as i32,
            agent_ids,
            cleanup_enabled: true,
            cleanup_ttl_hours: 24,
            workspace_path: None,
            created_at: now,
            updated_at: now,
        })?;
    }
    db.meta_set("seeded", "1")?;
    db.meta_set(V2_FLAG, "1")?;
    Ok(true)
}

const V2_FLAG: &str = "agents_v2_seeded";

/// Pods seeded before Ruby, Amelia, Luna and Robert existed get them once: the agents are added with their
/// stable ids (so devices that upgrade separately converge through sync) and appended to the default chats.
/// Chats are only extended, never re-seeded, so the user's edits survive; an agent deleted afterwards stays deleted.
pub fn seed_new_agents_if_needed(db: &Db) -> Result<bool> {
    if db.meta_get(V2_FLAG)?.is_some() {
        return Ok(false);
    }
    let first = AGENTS.len() - AGENTS_V2.len();
    let mut added = Vec::new();
    for (i, s) in AGENTS.iter().enumerate().skip(first) {
        let id = agent_seed_id(s.slug);
        if db.agent_row_exists(&id, s.slug)? {
            continue;
        }
        db.upsert_agent(&Agent {
            id: id.clone(),
            slug: s.slug.into(),
            name: s.name.into(),
            role: s.role,
            specialty: s.specialty.into(),
            job: s.job.into(),
            color: (s.color)(),
            model: None,
            image_model: None,
            ios_model: None,
            persona: None,
            sort: i as i32,
            updated_at: now_ms(),
        })?;
        added.push(s.slug);
    }
    for c in CHATS {
        let Ok(mut chat) = db.get_chat(&chat_seed_id(c.name)) else { continue };
        let before = chat.agent_ids.len();
        for slug in c.agents.iter().filter(|s| added.contains(s)) {
            let id = agent_seed_id(slug);
            if !chat.agent_ids.contains(&id) {
                chat.agent_ids.push(id);
            }
        }
        if chat.agent_ids.len() != before {
            db.upsert_chat(&chat)?;
        }
    }
    db.meta_set(V2_FLAG, "1")?;
    Ok(!added.is_empty())
}

/// Databases seeded before ids were stable have random ids for the default agents, chats and role.
/// Rewrites them (and every reference) to the stable ids, so two devices that each seeded their own
/// copy line up once they sync. Runs once per database and never touches custom agents or chats.
pub fn canonicalize_seed_ids(db: &Db) -> Result<usize> {
    if db.meta_get("seed_ids_stable")?.is_some() {
        return Ok(0);
    }
    let mut renamed = 0;
    for a in db.list_agents()? {
        if AGENTS.iter().any(|s| s.slug == a.slug) {
            let to = agent_seed_id(&a.slug);
            if a.id != to && db.rename_id(&a.id, &to)? {
                renamed += 1;
            }
        }
    }
    // A default chat is the oldest chat of that name; a chat the user made later with the same name stays as is.
    let mut chats = db.list_chats()?;
    chats.sort_by_key(|c| c.created_at);
    for s in CHATS {
        if let Some(c) = chats.iter().find(|c| c.name == s.name) {
            let to = chat_seed_id(s.name);
            if c.id != to && db.rename_id(&c.id, &to)? {
                renamed += 1;
            }
        }
    }
    if db.meta_get("roles_seeded")?.is_some()
        && let Some(r) = db.list_roles()?.into_iter().find(|r| r.name == "Master" && r.icon == "crown")
        && r.id != master_role_id()
        && db.rename_id(&r.id, &master_role_id())?
    {
        renamed += 1;
    }
    db.meta_set("seed_ids_stable", "1")?;
    Ok(renamed)
}

/// The user's default role: "Master", in a rainbow gradient. Runs once per database (fresh or upgraded).
pub fn seed_roles_if_needed(db: &Db) -> Result<bool> {
    if db.meta_get("roles_seeded")?.is_some() {
        return Ok(false);
    }
    if db.list_roles()?.is_empty() {
        db.upsert_role(&UserRole {
            id: master_role_id(),
            name: "Master".into(),
            icon: "crown".into(),
            color: rainbow(),
            sort: 0,
            updated_at: now_ms(),
        })?;
    }
    db.meta_set("roles_seeded", "1")?;
    Ok(true)
}

pub fn rainbow() -> AgentColor {
    AgentColor::gradient(
        90.0,
        &[("#ff3b5c", 0.0), ("#ff9f1c", 20.0), ("#ffd60a", 38.0), ("#2ee59d", 56.0), ("#3d8bfd", 76.0), ("#b15cff", 100.0)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_once_with_moderators_everywhere() {
        let db = Db::open_in_memory().unwrap();
        assert!(seed_if_needed(&db).unwrap());
        assert!(!seed_if_needed(&db).unwrap());
        let agents = db.list_agents().unwrap();
        assert_eq!(agents.len(), 14);
        let roger = db.agent_by_slug("roger").unwrap();
        let lincoln = db.agent_by_slug("lincoln").unwrap();
        let chats = db.list_chats().unwrap();
        assert_eq!(chats.len(), 5);
        for c in &chats {
            assert!(c.agent_ids.contains(&roger.id) && c.agent_ids.contains(&lincoln.id), "{}", c.name);
        }
        let general = chats.iter().find(|c| c.name == "General").unwrap();
        assert_eq!(general.agent_ids.len(), 14);
    }

    /// Simulates a pod seeded by the first release: no v2 agents, default chats without them.
    fn old_pod() -> Db {
        let db = Db::open_in_memory().unwrap();
        seed_if_needed(&db).unwrap();
        for slug in AGENTS_V2 {
            let id = agent_seed_id(slug);
            db.conn().execute("DELETE FROM chat_agents WHERE agent_id = ?1", [&id]).unwrap();
            db.conn().execute("DELETE FROM agents WHERE id = ?1", [&id]).unwrap();
        }
        db.conn().execute("DELETE FROM meta WHERE key = ?1", [V2_FLAG]).unwrap();
        db
    }

    #[test]
    fn existing_pods_get_the_new_agents_once() {
        let db = old_pod();
        assert_eq!(db.list_agents().unwrap().len(), 10);
        let general_id = chat_seed_id("General");
        let mut general = db.get_chat(&general_id).unwrap();
        general.workspace_path = Some("/work".into());
        db.upsert_chat(&general).unwrap();

        assert!(seed_new_agents_if_needed(&db).unwrap());
        assert_eq!(db.list_agents().unwrap().len(), 14);
        let general = db.get_chat(&general_id).unwrap();
        assert_eq!(general.agent_ids.len(), 14);
        assert_eq!(general.workspace_path.as_deref(), Some("/work"), "chat edits survive");
        let coding = db.get_chat(&chat_seed_id("Coding")).unwrap();
        assert!(coding.agent_ids.contains(&agent_seed_id("robert")) && coding.agent_ids.contains(&agent_seed_id("amelia")));
        assert!(!coding.agent_ids.contains(&agent_seed_id("ruby")));

        // Second run is a no-op, and a deleted agent stays deleted.
        assert!(!seed_new_agents_if_needed(&db).unwrap());
        db.conn().execute("UPDATE agents SET deleted = 1 WHERE slug = 'ruby'", []).unwrap();
        assert!(!seed_new_agents_if_needed(&db).unwrap());
        assert_eq!(db.list_agents().unwrap().len(), 13);
    }

    #[test]
    fn fresh_pods_skip_the_upgrade_step() {
        let db = Db::open_in_memory().unwrap();
        seed_if_needed(&db).unwrap();
        assert!(!seed_new_agents_if_needed(&db).unwrap());
        assert_eq!(db.list_agents().unwrap().len(), 14);
    }

    #[test]
    fn master_role_is_seeded_once_and_stays_deleted() {
        let db = Db::open_in_memory().unwrap();
        assert!(seed_roles_if_needed(&db).unwrap());
        let roles = db.list_roles().unwrap();
        assert_eq!(roles.len(), 1);
        assert_eq!((roles[0].name.as_str(), roles[0].icon.as_str()), ("Master", "crown"));
        assert!(matches!(&roles[0].color, AgentColor::Gradient { stops, .. } if stops.len() == 6));
        db.delete_role(&roles[0].id).unwrap();
        assert!(!seed_roles_if_needed(&db).unwrap(), "deleting every role is respected");
        assert!(db.list_roles().unwrap().is_empty());
    }
}
