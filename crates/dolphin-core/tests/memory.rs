//! Per-agent memory end to end with a scripted provider: what each agent is sent, where it sits in the
//! request, which project it applies to, and the Core operations behind the Memory list.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use dolphin_core::db::Db;
use dolphin_core::events::CoreEvent;
use dolphin_core::model::*;
use dolphin_core::providers::{ChatRequest, EventStream, Provider, ProviderEvent, Usage};
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreRef};
use serde_json::json;

#[derive(Default)]
struct Scripted {
    requests: Mutex<Vec<ChatRequest>>,
    /// What each embeddings call was asked to embed.
    embed_calls: Mutex<Vec<Vec<String>>>,
    embed_fails: std::sync::atomic::AtomicBool,
}

/// A stand-in embedding model: a text becomes counts of the words from a few topics, so "sql" and "PostgreSQL" are
/// close in meaning here though they share no word.
fn fake_embedding(text: &str) -> Vec<f32> {
    const TOPICS: [&[&str]; 4] = [&["sql", "database", "postgresql", "query", "queries"], &["emoji", "terse", "brief", "short"], &["cat", "pet", "miso"], &["tests", "committing", "commit"]];
    let lower = text.to_lowercase();
    let mut v: Vec<f32> = TOPICS.iter().map(|words| words.iter().filter(|w| lower.contains(**w)).count() as f32).collect();
    v.push(0.01);
    v
}

#[async_trait]
impl Provider for Scripted {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }
    async fn list_models(&self) -> dolphin_core::Result<Vec<ModelInfo>> {
        Ok(vec![])
    }
    async fn embed(&self, _model: &str, inputs: &[String]) -> dolphin_core::Result<Vec<Vec<f32>>> {
        self.embed_calls.lock().unwrap().push(inputs.to_vec());
        if self.embed_fails.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(dolphin_core::CoreError::provider("the embedding server is down"));
        }
        Ok(inputs.iter().map(|t| fake_embedding(t)).collect())
    }

    async fn stream_chat(&self, req: ChatRequest) -> dolphin_core::Result<EventStream> {
        self.requests.lock().unwrap().push(req);
        let events = vec![
            ProviderEvent::TextDelta("ok".into()),
            ProviderEvent::Usage(Usage { input_tokens: 10, output_tokens: 1 }),
            ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None },
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

struct Pod {
    core: CoreRef,
    provider: Arc<Scripted>,
    provider_id: String,
    dir: tempfile::TempDir,
}

fn pod() -> Pod {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let cfg = core
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .unwrap();
    let provider = Arc::new(Scripted::default());
    core.override_provider(&cfg.id, provider.clone());
    for a in core.list_agents().unwrap() {
        if ["roger", "lincoln", "rick", "prof"].contains(&a.slug.as_str()) {
            let mut a = a;
            a.model = Some(ModelRef { provider_id: cfg.id.clone(), model_id: "qwen3.5:2b".into(), effort: Some(EffortValue::Toggle(true)), vision: false, tools: true, context_window: None });
            core.update_agent(a).unwrap();
        }
    }
    // These tests are about other things; the council has its own tests in council.rs.
    core.set_setting("council.enabled", None, json!(false)).unwrap();
    Pod { core, provider, provider_id: cfg.id, dir }
}

fn chat(core: &CoreRef, name: &str) -> Chat {
    core.list_chats().unwrap().into_iter().find(|c| c.name == name).unwrap()
}

fn agent(core: &CoreRef, slug: &str) -> Agent {
    core.list_agents().unwrap().into_iter().find(|a| a.slug == slug).unwrap()
}

fn answered(msgs: &[Message]) -> usize {
    msgs.iter().filter(|m| m.agent_id.is_some() && m.kind == MessageKind::Text && m.status == MessageStatus::Done).count()
}

async fn wait_for_answers(core: &CoreRef, chat_id: &str, n: usize) {
    for _ in 0..300 {
        if answered(&core.list_messages(chat_id, 500).unwrap()) >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {n} answers");
}

/// The request an agent was last sent: (system prompt, last message, number of stable messages, message count).
fn last_request(p: &Pod, who: &str) -> (String, String, Option<usize>, usize) {
    let reqs = p.provider.requests.lock().unwrap();
    let r = reqs.iter().rev().find(|r| r.system.starts_with(&format!("You are {who}"))).unwrap_or_else(|| panic!("no request for {who}"));
    (r.system.clone(), r.messages.last().unwrap().text_content(), r.stable_messages, r.messages.len())
}

#[tokio::test]
async fn an_agent_is_sent_its_own_memory_after_the_history_and_never_anothers() {
    let p = pod();
    let (rick, roger) = (agent(&p.core, "rick"), agent(&p.core, "roger"));
    p.core.add_agent_memory(&rick.id, MemoryKind::Style, "Keep answers under three sentences", None).unwrap();
    p.core.add_agent_memory(&roger.id, MemoryKind::Fact, "Roger-only: the user keeps bees", None).unwrap();
    let coding = chat(&p.core, "Coding");

    p.core.send_message(&coding.id, "@Rick how do I reverse a string in Rust?", vec![]).await.unwrap();
    wait_for_answers(&p.core, &coding.id, 1).await;
    let (system, tail, stable, len) = last_request(&p, "Rick");
    assert!(!system.contains("three sentences"), "the system prompt stays identical between turns: memory is not part of it");
    assert!(tail.contains("- Keep answers under three sentences"), "{tail}");
    assert!(!tail.contains("bees"), "Roger's memory is not Rick's");
    assert_eq!(stable, Some(len - 1), "memory is volatile: the history before it stays cacheable");
    assert_eq!(p.core.list_agent_memories(&rick.id).unwrap()[0].hits, 1, "using it counts a hit");

    p.core.send_message(&coding.id, "@Roger hello", vec![]).await.unwrap();
    wait_for_answers(&p.core, &coding.id, 2).await;
    let (_, tail, _, _) = last_request(&p, "Roger");
    assert!(tail.contains("the user keeps bees") && !tail.contains("three sentences"), "{tail}");
}

#[tokio::test]
async fn a_chat_without_memories_sends_nothing_extra() {
    let p = pod();
    let general = chat(&p.core, "General");
    p.core.send_message(&general.id, "@Roger hello", vec![]).await.unwrap();
    wait_for_answers(&p.core, &general.id, 1).await;
    let (_, tail, stable, len) = last_request(&p, "Roger");
    assert!(!tail.contains("learned about this user"), "{tail}");
    assert_eq!(stable, Some(len), "no volatile message was added: everything in the request is cacheable");
}

#[tokio::test]
async fn project_memories_apply_in_that_workspace_and_nowhere_else() {
    let p = pod();
    let roger = agent(&p.core, "roger");
    let ws = p.dir.path().join("proj-alpha");
    std::fs::create_dir(&ws).unwrap();
    let coding = chat(&p.core, "Coding");
    let general = chat(&p.core, "General");
    p.core.set_setting("chat.workspace_path", Some("Coding"), json!(ws.to_string_lossy())).unwrap();
    p.core.add_agent_memory(&roger.id, MemoryKind::Workflow, "Everywhere: run the linter", None).unwrap();
    p.core.add_agent_memory(&roger.id, MemoryKind::Workflow, "Alpha only: use tabs", Some("proj-alpha")).unwrap();
    p.core.add_agent_memory(&roger.id, MemoryKind::Workflow, "Zeta only: use spaces", Some("proj-zeta")).unwrap();

    p.core.send_message(&coding.id, "@Roger hello", vec![]).await.unwrap();
    wait_for_answers(&p.core, &coding.id, 1).await;
    let (_, tail, _, _) = last_request(&p, "Roger");
    assert!(tail.contains("run the linter") && tail.contains("use tabs"), "{tail}");
    assert!(!tail.contains("use spaces"), "another project's rule stays out: {tail}");

    p.core.send_message(&general.id, "@Roger hello", vec![]).await.unwrap();
    wait_for_answers(&p.core, &general.id, 1).await;
    let (_, tail, _, _) = last_request(&p, "Roger");
    assert!(tail.contains("run the linter") && !tail.contains("use tabs") && !tail.contains("use spaces"), "no workspace: only the everywhere rule: {tail}");
}

#[test]
fn the_memory_list_adds_edits_pins_and_forgets() {
    let p = pod();
    let rick = agent(&p.core, "rick");
    let mut events = p.core.subscribe();

    let m = p.core.add_agent_memory(&rick.id, MemoryKind::Avoid, "  Don't rewrite\n the whole file  ", Some("  ")).unwrap();
    assert_eq!((m.text.as_str(), m.project.as_deref(), m.source, m.pinned), ("Don't rewrite the whole file", None, MemorySource::Explicit, false));

    // Pinning alone keeps the source; rewording or re-scoping makes it the user's own.
    let pinned = p.core.update_agent_memory(AgentMemory { pinned: true, ..m.clone() }).unwrap();
    assert!(pinned.pinned);
    assert_eq!(pinned.source, MemorySource::Explicit);
    let reworded = p.core.update_agent_memory(AgentMemory { text: "Don't rewrite whole files".into(), project: Some("api".into()), ..pinned }).unwrap();
    assert_eq!((reworded.source, reworded.project.as_deref()), (MemorySource::Edited, Some("api")));
    assert_eq!(p.core.list_agent_memories(&rick.id).unwrap().len(), 1);

    assert!(p.core.add_agent_memory(&rick.id, MemoryKind::Fact, "   ", None).is_err(), "blank");
    assert!(p.core.add_agent_memory(&rick.id, MemoryKind::Fact, &"x".repeat(301), None).is_err(), "one thought, not a paragraph");
    assert!(p.core.update_agent_memory(AgentMemory { text: String::new(), ..reworded.clone() }).is_err());
    assert!(p.core.add_agent_memory("no-such-agent", MemoryKind::Fact, "x", None).is_err());
    assert_eq!(p.core.list_agent_memories(&rick.id).unwrap()[0].text, "Don't rewrite whole files", "failed edits change nothing");

    p.core.delete_agent_memory(&m.id).unwrap();
    assert!(p.core.list_agent_memories(&rick.id).unwrap().is_empty());
    assert!(p.core.delete_agent_memory(&m.id).is_err(), "already gone");

    let mut changed = 0;
    while let Ok(e) = events.try_recv() {
        if matches!(e, CoreEvent::MemoriesChanged { ref agent_id } if *agent_id == rick.id) {
            changed += 1;
        }
    }
    assert_eq!(changed, 4, "add, pin, reword and delete each tell the UI");
}

#[test]
fn forgetting_everything_touches_only_that_agent_and_the_cap_holds() {
    let p = pod();
    let (rick, roger) = (agent(&p.core, "rick"), agent(&p.core, "roger"));
    p.core.add_agent_memory(&roger.id, MemoryKind::Fact, "Roger's fact", None).unwrap();
    for i in 0..40 {
        p.core.add_agent_memory(&rick.id, MemoryKind::Fact, &format!("fact {i}"), None).unwrap();
    }
    let err = p.core.add_agent_memory(&rick.id, MemoryKind::Fact, "one too many", None).unwrap_err();
    assert!(err.to_string().contains("forget some first"), "{err}");

    assert_eq!(p.core.forget_agent_memories(&rick.id).unwrap(), 40);
    assert_eq!(p.core.forget_agent_memories(&rick.id).unwrap(), 0);
    assert!(p.core.list_agent_memories(&rick.id).unwrap().is_empty());
    assert_eq!(p.core.list_agent_memories(&roger.id).unwrap().len(), 1, "the other agent keeps its own");
    p.core.add_agent_memory(&rick.id, MemoryKind::Fact, "room again", None).unwrap();
}

// ---------------------------------------------------------------- sharing a note with other agents

/// Rick's note, shared as asked.
fn shared_note(p: &Pod, text: &str, share: MemoryShare) -> AgentMemory {
    let rick = agent(&p.core, "rick");
    let m = p.core.add_agent_memory(&rick.id, MemoryKind::Style, text, None).unwrap();
    p.core.update_agent_memory(AgentMemory { share, ..m }).unwrap()
}

async fn ask(p: &Pod, chat_name: &str, who: &str, n: usize) -> String {
    let c = chat(&p.core, chat_name);
    p.core.send_message(&c.id, &format!("@{who} hello"), vec![]).await.unwrap();
    wait_for_answers(&p.core, &c.id, n).await;
    last_request(p, who).1
}

#[tokio::test]
async fn a_shared_note_reaches_the_agents_it_is_shared_with_and_nobody_else() {
    let p = pod();
    let (prof, roger) = (agent(&p.core, "prof"), agent(&p.core, "roger"));
    shared_note(&p, "Everyone: keep it terse", MemoryShare::All);
    shared_note(&p, "Only Prof: explain with examples", MemoryShare::Agents { ids: vec![prof.id.clone()] });
    p.core.add_agent_memory(&agent(&p.core, "rick").id, MemoryKind::Fact, "Private to Rick", None).unwrap();

    let to_roger = ask(&p, "Coding", "Roger", 1).await;
    assert!(to_roger.contains("Everyone: keep it terse"), "{to_roger}");
    assert!(!to_roger.contains("Only Prof") && !to_roger.contains("Private to Rick"), "{to_roger}");

    let to_prof = ask(&p, "Coding", "Prof", 2).await;
    assert!(to_prof.contains("Everyone: keep it terse") && to_prof.contains("Only Prof: explain with examples"), "{to_prof}");
    assert!(!to_prof.contains("Private to Rick"));

    let to_rick = ask(&p, "Coding", "Rick", 3).await;
    assert_eq!(to_rick.matches("Everyone: keep it terse").count(), 1, "the owner is sent its own note once, not twice");
    assert!(to_rick.contains("Private to Rick"));
    assert!(p.core.list_agent_memories(&roger.id).unwrap().is_empty(), "sharing sends a note, it does not copy it into the other agent's list");
}

#[tokio::test]
async fn project_scope_still_applies_and_a_duplicate_is_sent_once() {
    let p = pod();
    let roger = agent(&p.core, "roger");
    let rick = agent(&p.core, "rick");
    let m = p.core.add_agent_memory(&rick.id, MemoryKind::Workflow, "Alpha: use tabs", Some("proj-alpha")).unwrap();
    p.core.update_agent_memory(AgentMemory { share: MemoryShare::All, ..m }).unwrap();
    shared_note(&p, "Be polite", MemoryShare::All);
    p.core.add_agent_memory(&roger.id, MemoryKind::Style, "be polite", None).unwrap();

    let in_general = ask(&p, "General", "Roger", 1).await;
    assert!(!in_general.contains("Alpha: use tabs"), "a project note is for that project even when shared: {in_general}");
    assert_eq!(in_general.to_lowercase().matches("be polite").count(), 1, "Roger already has this note: {in_general}");
}

#[test]
fn sharing_is_checked_and_every_agent_it_reaches_is_told() {
    let p = pod();
    let (rick, roger, prof) = (agent(&p.core, "rick"), agent(&p.core, "roger"), agent(&p.core, "prof"));
    let m = p.core.add_agent_memory(&rick.id, MemoryKind::Style, "Be brief", None).unwrap();
    let mut events = p.core.subscribe();
    let told = |events: &mut tokio::sync::broadcast::Receiver<CoreEvent>| {
        let mut ids: Vec<String> = Vec::new();
        while let Ok(e) = events.try_recv() {
            if let CoreEvent::MemoriesChanged { agent_id } = e {
                ids.push(agent_id);
            }
        }
        ids.sort();
        ids.dedup();
        ids
    };

    // Unknown agents, the owner itself and repeats are dropped; nothing left means no sharing.
    let junk = MemoryShare::Agents { ids: vec![rick.id.clone(), "nobody".into(), rick.id.clone()] };
    assert_eq!(p.core.update_agent_memory(AgentMemory { share: junk, ..m.clone() }).unwrap().share, MemoryShare::None);
    let some = MemoryShare::Agents { ids: vec![prof.id.clone(), prof.id.clone(), "nobody".into()] };
    let saved = p.core.update_agent_memory(AgentMemory { share: some, ..m.clone() }).unwrap();
    assert_eq!(saved.share, MemoryShare::Agents { ids: vec![prof.id.clone()] });
    assert_eq!(saved.source, MemorySource::Explicit, "sharing does not make it an edited note");
    let mut want = vec![rick.id.clone(), prof.id.clone()];
    want.sort();
    assert_eq!(told(&mut events), want, "the owner's list and Prof's list changed");
    assert_eq!(p.core.list_shared_memories(&prof.id).unwrap().len(), 1);
    assert!(p.core.list_shared_memories(&roger.id).unwrap().is_empty());
    assert!(p.core.list_shared_memories(&rick.id).unwrap().is_empty(), "an agent is not sent its own note as a shared one");

    // Widening to everyone tells everyone; taking it back tells everyone who had it.
    p.core.update_agent_memory(AgentMemory { share: MemoryShare::All, ..saved.clone() }).unwrap();
    assert!(told(&mut events).contains(&roger.id));
    p.core.update_agent_memory(AgentMemory { share: MemoryShare::None, ..saved }).unwrap();
    assert!(told(&mut events).contains(&roger.id) && p.core.list_shared_memories(&roger.id).unwrap().is_empty());

    // Deleting a shared note, or forgetting its owner's memory, removes it from every list.
    let again = p.core.update_agent_memory(AgentMemory { share: MemoryShare::All, ..p.core.list_agent_memories(&rick.id).unwrap().remove(0) }).unwrap();
    told(&mut events);
    p.core.delete_agent_memory(&again.id).unwrap();
    assert!(told(&mut events).contains(&roger.id) && p.core.list_shared_memories(&roger.id).unwrap().is_empty());
    let m2 = p.core.add_agent_memory(&rick.id, MemoryKind::Fact, "Fact", None).unwrap();
    p.core.update_agent_memory(AgentMemory { share: MemoryShare::All, ..m2 }).unwrap();
    assert_eq!(p.core.list_shared_memories(&roger.id).unwrap().len(), 1);
    p.core.forget_agent_memories(&rick.id).unwrap();
    assert!(p.core.list_shared_memories(&roger.id).unwrap().is_empty());
}

// ---------------------------------------------------------------- ranking notes by meaning

const FILLER: &str = " and some more words to make this note long enough to count";

/// Rick with a memory too big to send whole: one standing rule and four notes, one of them about his database.
fn crowded_memory(p: &Pod) -> Vec<AgentMemory> {
    let rick = agent(&p.core, "rick").id;
    let add = |kind, text: &str| p.core.add_agent_memory(&rick, kind, &format!("{text}{}", FILLER.repeat(4)), None).unwrap();
    vec![
        add(MemoryKind::Style, "Terse answers, no emoji"),
        add(MemoryKind::Fact, "The database is PostgreSQL"),
        add(MemoryKind::Fact, "Owns a cat called Miso"),
        add(MemoryKind::Workflow, "Runs the tests before committing"),
        add(MemoryKind::Fact, "Works from Berlin"),
    ]
}

fn use_embedding_model(p: &Pod) {
    let model = ModelRef { provider_id: p.provider_id.clone(), model_id: "nomic-embed-text".into(), effort: None, vision: false, tools: false, context_window: None };
    p.core.set_setting("memory.embedding_model", None, serde_json::to_value(model).unwrap()).unwrap();
}

async fn ask_rick(p: &Pod, text: &str, n: usize) -> String {
    let c = chat(&p.core, "Coding");
    p.core.send_message(&c.id, &format!("@Rick {text}"), vec![]).await.unwrap();
    wait_for_answers(&p.core, &c.id, n).await;
    last_request(p, "Rick").1
}

fn embedded_texts(p: &Pod) -> Vec<Vec<String>> {
    p.provider.embed_calls.lock().unwrap().clone()
}

#[tokio::test]
async fn meaning_picks_the_note_that_shared_words_would_miss() {
    let p = pod();
    crowded_memory(&p);

    // Without an embedding model the words decide: nothing in "why is my sql query slow" matches the PostgreSQL note.
    let by_words = ask_rick(&p, "why is my sql slow?", 1).await;
    assert!(by_words.contains("Terse answers") && !by_words.contains("PostgreSQL"), "{by_words}");
    assert!(embedded_texts(&p).is_empty(), "no model chosen: nothing is embedded");

    use_embedding_model(&p);
    let by_meaning = ask_rick(&p, "why is my sql slow?", 2).await;
    assert!(by_meaning.contains("The database is PostgreSQL"), "{by_meaning}");
    assert!(by_meaning.contains("Terse answers"), "the standing rule is still sent");
    assert!(!by_meaning.contains("Owns a cat") && !by_meaning.contains("Berlin"), "unrelated notes are not: {by_meaning}");
}

#[tokio::test]
async fn each_note_is_embedded_once_and_again_only_when_reworded() {
    let p = pod();
    let notes = crowded_memory(&p);
    use_embedding_model(&p);

    ask_rick(&p, "why is my sql slow?", 1).await;
    let first = embedded_texts(&p);
    assert_eq!(first.len(), 1, "one batched call");
    assert_eq!(first[0].len(), 5, "the request and the four notes that are ranked: the standing rule is always sent and needs no vector");

    ask_rick(&p, "which sql query is slowest?", 2).await;
    let second = embedded_texts(&p);
    assert_eq!(second[1], vec!["which sql query is slowest?".to_string()], "notes already embedded are not embedded again");

    let db_note = &notes[1];
    p.core.update_agent_memory(AgentMemory { text: format!("The database is MySQL{}", FILLER.repeat(4)), ..db_note.clone() }).unwrap();
    ask_rick(&p, "why is my sql slow?", 3).await;
    let third = embedded_texts(&p);
    assert_eq!(third[2].len(), 2, "the request and the one note that changed: {:?}", third[2]);
    assert!(third[2][1].starts_with("The database is MySQL"));
}

#[tokio::test]
async fn a_failing_embedding_model_costs_nothing_but_the_better_ranking() {
    let p = pod();
    crowded_memory(&p);
    use_embedding_model(&p);
    p.provider.embed_fails.store(true, std::sync::atomic::Ordering::SeqCst);

    let tail = ask_rick(&p, "write a function that names my cat Miso", 1).await;
    assert!(tail.contains("Owns a cat called Miso"), "shared words still work when the model is down: {tail}");
    assert!(tail.contains("Terse answers"));
    let coding = chat(&p.core, "Coding");
    let msgs = p.core.list_messages(&coding.id, 50).unwrap();
    assert!(msgs.iter().any(|m| m.agent_id.is_some() && m.status == MessageStatus::Done && m.content == "ok"), "the request itself was answered");
}

#[tokio::test]
async fn a_small_memory_never_asks_the_embedding_model() {
    let p = pod();
    p.core.add_agent_memory(&agent(&p.core, "rick").id, MemoryKind::Fact, "The database is PostgreSQL", None).unwrap();
    use_embedding_model(&p);
    let tail = ask_rick(&p, "why is my sql slow?", 1).await;
    assert!(tail.contains("PostgreSQL"), "a few notes are sent whole");
    assert!(embedded_texts(&p).is_empty());
}

#[tokio::test]
async fn the_test_button_says_whether_the_embedding_model_works() {
    let p = pod();
    assert!(p.core.test_embedding_model().await.unwrap_err().to_string().contains("pick an embedding model"));
    use_embedding_model(&p);
    assert_eq!(p.core.test_embedding_model().await.unwrap(), 5, "the size of the vectors");
    p.provider.embed_fails.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(p.core.test_embedding_model().await.unwrap_err().to_string().contains("embedding server is down"));
}
