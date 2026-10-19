//! First-run defaults: the ten default agents and five default chats.
//! Agents are ordinary records, so custom agents can be added later without a migration.

use crate::db::Db;
use crate::error::Result;
use crate::model::{Agent, AgentColor, AgentRole, Chat};
use crate::util::{new_id, now_ms};

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
];

pub struct ChatSeed {
    pub name: &'static str,
    pub icon: &'static str,
    /// Specialist slugs; Roger and Lincoln are always added.
    pub agents: &'static [&'static str],
}

pub const CHATS: &[ChatSeed] = &[
    // Prof is in Coding too, so "how do I write X in Java" can go to him (the learning flow).
    ChatSeed { name: "Coding", icon: "code", agents: &["rick", "jordan", "ducky", "viktor", "prof"] },
    ChatSeed {
        name: "General",
        icon: "message-circle",
        agents: &["rick", "micheal", "viktor", "jordan", "emma", "ducky", "sarah", "prof"],
    },
    ChatSeed { name: "Long Horizon Agentic", icon: "rocket", agents: &["micheal", "rick", "jordan"] },
    ChatSeed { name: "Learning", icon: "graduation-cap", agents: &["prof", "ducky"] },
    ChatSeed { name: "Creative", icon: "palette", agents: &["emma", "sarah"] },
];

pub fn seed_if_needed(db: &Db) -> Result<bool> {
    if db.meta_get("seeded")?.is_some() {
        return Ok(false);
    }
    let now = now_ms();
    let mut ids = std::collections::HashMap::new();
    for (i, s) in AGENTS.iter().enumerate() {
        let a = Agent {
            id: new_id(),
            slug: s.slug.into(),
            name: s.name.into(),
            role: s.role,
            specialty: s.specialty.into(),
            job: s.job.into(),
            color: (s.color)(),
            model: None,
            image_model: None,
            ios_model: None,
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
            id: new_id(),
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
    Ok(true)
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
        assert_eq!(agents.len(), 10);
        let roger = db.agent_by_slug("roger").unwrap();
        let lincoln = db.agent_by_slug("lincoln").unwrap();
        let chats = db.list_chats().unwrap();
        assert_eq!(chats.len(), 5);
        for c in &chats {
            assert!(c.agent_ids.contains(&roger.id) && c.agent_ids.contains(&lincoln.id), "{}", c.name);
        }
        let general = chats.iter().find(|c| c.name == "General").unwrap();
        assert_eq!(general.agent_ids.len(), 10);
    }
}
