//! First-run defaults: the twenty-four default agents and eight default chats.
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
    AgentSeed {
        slug: "council",
        name: "DP Council",
        role: AgentRole::Specialist,
        specialty: "council",
        job: "Council. Runs on the strongest model in the pod and criticizes the other agents' approach once, before they work on big tasks (or on their draft, for plans): what is solid, what is risky or missing, and what to do instead. Then it steps out.",
        color: || AgentColor::gradient(135.0, &[("#facc15", 0.0), ("#a16207", 100.0)]),
    },
    AgentSeed {
        slug: "vera",
        name: "Vera",
        role: AgentRole::Specialist,
        specialty: "director",
        job: "Software director. Runs the development chats: decides who works on a request and in what order (engineers, designers, the coder, the checks), criticizes the engineering plan before code is written and owns the final result.",
        color: || AgentColor::gradient(135.0, &[("#0f766e", 0.0), ("#a3e635", 100.0)]),
    },
    AgentSeed {
        slug: "nolan",
        name: "Nolan",
        role: AgentRole::Specialist,
        specialty: "engineer",
        job: "Software engineer for architecture and systems. Before any code is written, lays out the components, boundaries, data flow and risks.",
        color: || AgentColor::gradient(135.0, &[("#1d4ed8", 0.0), ("#38bdf8", 100.0)]),
    },
    AgentSeed {
        slug: "priya",
        name: "Priya",
        role: AgentRole::Specialist,
        specialty: "engineer",
        job: "Software engineer for interfaces and quality. Before any code is written, defines the data model, the interfaces, the edge cases and the test strategy.",
        color: || AgentColor::gradient(135.0, &[("#9333ea", 0.0), ("#38bdf8", 100.0)]),
    },
    AgentSeed {
        slug: "mira",
        name: "Mira",
        role: AgentRole::Specialist,
        specialty: "designer",
        job: "Designer, idea A: the bold, expressive direction. Works in parallel with Kai so you get two design ideas to pick between or merge.",
        color: || AgentColor::gradient(135.0, &[("#db2777", 0.0), ("#fb923c", 100.0)]),
    },
    AgentSeed {
        slug: "kai",
        name: "Kai",
        role: AgentRole::Specialist,
        specialty: "designer",
        job: "Designer, idea B: the minimal, practical direction. Works in parallel with Mira so you get two design ideas to pick between or merge.",
        color: || AgentColor::gradient(135.0, &[("#0891b2", 0.0), ("#86efac", 100.0)]),
    },
    AgentSeed {
        slug: "tilda",
        name: "Tilda",
        role: AgentRole::Specialist,
        specialty: "level_designer",
        job: "Level designer for games. Layouts, pacing, encounters, secrets and the difficulty curve.",
        color: || AgentColor::gradient(135.0, &[("#65a30d", 0.0), ("#fde047", 100.0)]),
    },
    AgentSeed {
        slug: "jules",
        name: "Jules",
        role: AgentRole::Specialist,
        specialty: "artist_3d",
        job: "3D artist for games. Models, materials, rigging and animation, plus asset pipelines and polygon and texture budgets.",
        color: || AgentColor::gradient(135.0, &[("#c026d3", 0.0), ("#6366f1", 100.0)]),
    },
    AgentSeed {
        slug: "sonia",
        name: "Sonia",
        role: AgentRole::Specialist,
        specialty: "audio_designer",
        job: "Audio designer for games. Music, sound effects, adaptive audio and mixing.",
        color: || AgentColor::gradient(135.0, &[("#e11d48", 0.0), ("#7c3aed", 100.0)]),
    },
    AgentSeed {
        slug: "wren",
        name: "Wren",
        role: AgentRole::Specialist,
        specialty: "story_designer",
        job: "Story designer for games. Narrative structure, characters, dialogue, quests and lore.",
        color: || AgentColor::gradient(135.0, &[("#b45309", 0.0), ("#fcd34d", 100.0)]),
    },
];

/// Slugs added after the first release. Existing pods get them once, through `seed_new_agents_if_needed`.
const AGENTS_V2: &[&str] = &["ruby", "amelia", "luna", "robert"];
const AGENTS_V3: &[&str] = &["council"];
const AGENTS_V4: &[&str] = &["vera", "nolan", "priya", "mira", "kai", "tilda", "jules", "sonia", "wren"];

pub struct ChatSeed {
    pub name: &'static str,
    pub icon: &'static str,
    /// Specialist slugs; Roger and Lincoln are always added.
    pub agents: &'static [&'static str],
}

pub const CHATS: &[ChatSeed] = &[
    // Prof is in Coding too, so "how do I write X in Java" can go to him (the learning flow).
    ChatSeed { name: "Coding", icon: "code", agents: &["rick", "jordan", "ducky", "viktor", "prof", "robert", "amelia", "council"] },
    ChatSeed {
        name: "General",
        icon: "message-circle",
        agents: &["rick", "micheal", "viktor", "jordan", "emma", "ducky", "sarah", "prof", "ruby", "amelia", "luna", "robert", "council"],
    },
    ChatSeed { name: "Long Horizon Agentic", icon: "rocket", agents: &["micheal", "rick", "jordan", "robert", "amelia", "luna", "council"] },
    ChatSeed { name: "Learning", icon: "graduation-cap", agents: &["prof", "ducky", "ruby", "luna"] },
    ChatSeed { name: "Creative", icon: "palette", agents: &["emma", "sarah", "ruby"] },
    ChatSeed { name: "Software Development", icon: "terminal", agents: &DEV_TEAM },
    ChatSeed { name: "Web Development", icon: "globe", agents: &DEV_TEAM },
    ChatSeed {
        name: "Game Development",
        icon: "gamepad-2",
        agents: &[
            "vera", "nolan", "priya", "mira", "kai", "rick", "micheal", "jordan", "ducky", "viktor", "prof", "robert", "amelia", "luna", "council",
            "tilda", "jules", "sonia", "wren",
        ],
    },
];

/// The specialists of the development chats; the game chat adds the game specialists on top.
const DEV_TEAM: [&str; 15] =
    ["vera", "nolan", "priya", "mira", "kai", "rick", "micheal", "jordan", "ducky", "viktor", "prof", "robert", "amelia", "luna", "council"];

/// Default chats added after the first release. Existing pods get them once, through `seed_new_chats_if_needed`.
const CHATS_V2: &[&str] = &["Software Development", "Web Development", "Game Development"];

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
            learning: s.specialty != "council" && s.specialty != "director",
            suggest_persona: false,
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
    for (flag, _) in UPGRADES {
        db.meta_set(flag, "1")?;
    }
    db.meta_set(CHATS_V2_FLAG, "1")?;
    Ok(true)
}

const V2_FLAG: &str = "agents_v2_seeded";
const V3_FLAG: &str = "agents_v3_seeded";
const V4_FLAG: &str = "agents_v4_seeded";
const CHATS_V2_FLAG: &str = "chats_v2_seeded";

/// Each batch of agents added after the first release, with the meta flag that records it was handed out.
const UPGRADES: &[(&str, &[&str])] = &[(V2_FLAG, AGENTS_V2), (V3_FLAG, AGENTS_V3), (V4_FLAG, AGENTS_V4)];

/// Pods seeded before an agent existed get it once: the agents are added with their stable ids (so devices
/// that upgrade separately converge through sync) and appended to the default chats that list them.
/// Chats are only extended, never re-seeded, so the user's edits survive; an agent deleted afterwards stays deleted.
pub fn seed_new_agents_if_needed(db: &Db) -> Result<bool> {
    let mut any = false;
    for (flag, slugs) in UPGRADES {
        if db.meta_get(flag)?.is_some() {
            continue;
        }
        let mut added = Vec::new();
        for (i, s) in AGENTS.iter().enumerate().filter(|(_, s)| slugs.contains(&s.slug)) {
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
                // The council and the director never take notes about the user: they criticize and route work, they do not learn habits.
                learning: s.specialty != "council" && s.specialty != "director",
                suggest_persona: false,
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
        db.meta_set(flag, "1")?;
        any |= !added.is_empty();
    }
    Ok(any)
}

/// Pods seeded before a default chat existed get it once, with the stable chat id and the roster of that
/// chat (Roger and Lincoln first). Run it after `seed_new_agents_if_needed`, so the agents are there.
/// A chat the user deleted afterwards stays deleted, and a chat that already exists is left alone.
pub fn seed_new_chats_if_needed(db: &Db) -> Result<bool> {
    if db.meta_get(CHATS_V2_FLAG)?.is_some() {
        return Ok(false);
    }
    let now = now_ms();
    let mut any = false;
    let existing = db.list_chats()?;
    for (i, c) in CHATS.iter().enumerate().filter(|(_, c)| CHATS_V2.contains(&c.name)) {
        if existing.iter().any(|e| e.id == chat_seed_id(c.name) || e.name == c.name) {
            continue;
        }
        let mut agent_ids = Vec::new();
        for slug in ["roger", "lincoln"].into_iter().chain(c.agents.iter().copied()) {
            if let Ok(a) = db.agent_by_slug(slug) {
                agent_ids.push(a.id);
            }
        }
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
        any = true;
    }
    db.meta_set(CHATS_V2_FLAG, "1")?;
    Ok(any)
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
        assert_eq!(agents.len(), 24);
        let roger = db.agent_by_slug("roger").unwrap();
        let lincoln = db.agent_by_slug("lincoln").unwrap();
        let chats = db.list_chats().unwrap();
        assert_eq!(chats.len(), 8);
        for c in &chats {
            assert!(c.agent_ids.contains(&roger.id) && c.agent_ids.contains(&lincoln.id), "{}", c.name);
        }
        let general = chats.iter().find(|c| c.name == "General").unwrap();
        assert_eq!(general.agent_ids.len(), 15);
    }

    /// Simulates a pod seeded by the first release: no v2 agents, default chats without them.
    fn old_pod() -> Db {
        let db = Db::open_in_memory().unwrap();
        seed_if_needed(&db).unwrap();
        for slug in AGENTS_V2.iter().chain(AGENTS_V3) {
            let id = agent_seed_id(slug);
            db.conn().execute("DELETE FROM chat_agents WHERE agent_id = ?1", [&id]).unwrap();
            db.conn().execute("DELETE FROM agents WHERE id = ?1", [&id]).unwrap();
        }
        for flag in [V2_FLAG, V3_FLAG] {
            db.conn().execute("DELETE FROM meta WHERE key = ?1", [flag]).unwrap();
        }
        db
    }

    /// Simulates a pod from before the development team: no v4 agents, no development chats.
    fn pod_without_the_dev_team() -> Db {
        let db = Db::open_in_memory().unwrap();
        seed_if_needed(&db).unwrap();
        for slug in AGENTS_V4 {
            let id = agent_seed_id(slug);
            db.conn().execute("DELETE FROM chat_agents WHERE agent_id = ?1", [&id]).unwrap();
            db.conn().execute("DELETE FROM agents WHERE id = ?1", [&id]).unwrap();
        }
        for name in CHATS_V2 {
            let id = chat_seed_id(name);
            db.conn().execute("DELETE FROM chat_agents WHERE chat_id = ?1", [&id]).unwrap();
            db.conn().execute("DELETE FROM chats WHERE id = ?1", [&id]).unwrap();
        }
        for flag in [V4_FLAG, CHATS_V2_FLAG] {
            db.conn().execute("DELETE FROM meta WHERE key = ?1", [flag]).unwrap();
        }
        db
    }

    #[test]
    fn existing_pods_get_the_new_agents_once() {
        let db = old_pod();
        assert_eq!(db.list_agents().unwrap().len(), 19);
        let general_id = chat_seed_id("General");
        let mut general = db.get_chat(&general_id).unwrap();
        general.workspace_path = Some("/work".into());
        db.upsert_chat(&general).unwrap();

        assert!(seed_new_agents_if_needed(&db).unwrap());
        assert_eq!(db.list_agents().unwrap().len(), 24);
        let general = db.get_chat(&general_id).unwrap();
        assert_eq!(general.agent_ids.len(), 15);
        assert_eq!(general.workspace_path.as_deref(), Some("/work"), "chat edits survive");
        let coding = db.get_chat(&chat_seed_id("Coding")).unwrap();
        assert!(coding.agent_ids.contains(&agent_seed_id("robert")) && coding.agent_ids.contains(&agent_seed_id("amelia")));
        assert!(!coding.agent_ids.contains(&agent_seed_id("ruby")));
        assert!(coding.agent_ids.contains(&agent_seed_id("council")));
        assert!(!db.get_chat(&chat_seed_id("Creative")).unwrap().agent_ids.contains(&agent_seed_id("council")));
        let council = db.agent_by_slug("council").unwrap();
        assert!(!council.learning, "the council does not take notes about the user");

        // Second run is a no-op, and a deleted agent stays deleted.
        assert!(!seed_new_agents_if_needed(&db).unwrap());
        db.conn().execute("UPDATE agents SET deleted = 1 WHERE slug = 'ruby'", []).unwrap();
        assert!(!seed_new_agents_if_needed(&db).unwrap());
        assert_eq!(db.list_agents().unwrap().len(), 23);
    }

    #[test]
    fn a_pod_that_only_lacks_the_council_gets_just_the_council() {
        let db = old_pod();
        seed_new_agents_if_needed(&db).unwrap();
        db.conn().execute("DELETE FROM chat_agents WHERE agent_id = ?1", [agent_seed_id("council")]).unwrap();
        db.conn().execute("DELETE FROM agents WHERE slug = 'council'", []).unwrap();
        db.conn().execute("DELETE FROM meta WHERE key = ?1", [V3_FLAG]).unwrap();
        assert_eq!(db.list_agents().unwrap().len(), 23);
        assert!(seed_new_agents_if_needed(&db).unwrap());
        assert_eq!(db.list_agents().unwrap().len(), 24);
        assert!(db.get_chat(&chat_seed_id("Long Horizon Agentic")).unwrap().agent_ids.contains(&agent_seed_id("council")));
    }

    #[test]
    fn fresh_pods_skip_the_upgrade_step() {
        let db = Db::open_in_memory().unwrap();
        seed_if_needed(&db).unwrap();
        assert!(!seed_new_agents_if_needed(&db).unwrap());
        assert!(!seed_new_chats_if_needed(&db).unwrap());
        assert_eq!(db.list_agents().unwrap().len(), 24);
        assert_eq!(db.list_chats().unwrap().len(), 8);
    }

    #[test]
    fn existing_pods_get_the_development_team_and_chats_once() {
        let db = pod_without_the_dev_team();
        assert_eq!(db.list_agents().unwrap().len(), 15);
        assert_eq!(db.list_chats().unwrap().len(), 5);

        assert!(seed_new_agents_if_needed(&db).unwrap());
        assert_eq!(db.list_agents().unwrap().len(), 24);
        assert!(seed_new_chats_if_needed(&db).unwrap());
        let chats = db.list_chats().unwrap();
        assert_eq!(chats.len(), 8);

        let roger = agent_seed_id("roger");
        let game = db.get_chat(&chat_seed_id("Game Development")).unwrap();
        assert_eq!(game.agent_ids[0], roger, "Roger first");
        for slug in ["vera", "nolan", "priya", "mira", "kai", "rick", "tilda", "jules", "sonia", "wren"] {
            assert!(game.agent_ids.contains(&agent_seed_id(slug)), "{slug}");
        }
        let web = db.get_chat(&chat_seed_id("Web Development")).unwrap();
        assert!(web.agent_ids.contains(&agent_seed_id("vera")) && !web.agent_ids.contains(&agent_seed_id("tilda")));
        // The game specialists and the director stay out of the older chats.
        let coding = db.get_chat(&chat_seed_id("Coding")).unwrap();
        assert!(!coding.agent_ids.contains(&agent_seed_id("vera")) && !coding.agent_ids.contains(&agent_seed_id("nolan")));
        assert!(!db.agent_by_slug("vera").unwrap().learning, "the director does not take notes about the user");
        assert!(db.agent_by_slug("nolan").unwrap().learning);

        // Second run is a no-op, and a deleted chat stays deleted.
        assert!(!seed_new_chats_if_needed(&db).unwrap());
        db.delete_chat(&chat_seed_id("Web Development")).unwrap();
        assert!(!seed_new_chats_if_needed(&db).unwrap());
        assert_eq!(db.list_chats().unwrap().len(), 7);
    }

    #[test]
    fn a_chat_with_the_same_name_is_not_replaced() {
        let db = pod_without_the_dev_team();
        seed_new_agents_if_needed(&db).unwrap();
        let mut mine = db.get_chat(&chat_seed_id("Coding")).unwrap();
        mine.id = "mine".into();
        mine.name = "Web Development".into();
        mine.agent_ids.truncate(2);
        db.upsert_chat(&mine).unwrap();
        seed_new_chats_if_needed(&db).unwrap();
        assert_eq!(db.list_chats().unwrap().len(), 8);
        assert_eq!(db.get_chat("mine").unwrap().agent_ids.len(), 2);
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
