//! Delegation: a coding or agentic agent hands a task to Robert, Amelia or Luna in the middle of its run.
//! The delegate answers in the chat under its own name and the answer comes back to the caller as the tool result.

use futures::future::BoxFuture;

use super::*;
use crate::router::fallback;

impl Core {
    /// Run `args.agent` on `args.task` for `caller`. Boxed because it recurses: run_agent → delegate → run_agent.
    pub(super) fn delegate<'a>(
        self: &'a Arc<Self>,
        turn: &'a Turn,
        caller: &'a Agent,
        level: PermissionLevel,
        delegates: &'a [Agent],
        args: &'a Value,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            if args.get("_invalid_json").is_some() {
                return Err(CoreError::invalid("the tool arguments were not valid JSON; send them again"));
            }
            let who = args["agent"].as_str().unwrap_or("").trim().trim_start_matches('@').to_lowercase();
            let task_text = args["task"].as_str().unwrap_or("").trim();
            if task_text.is_empty() {
                return Err(CoreError::invalid("missing 'task': describe what the colleague should do"));
            }
            let Some(target) = delegates.iter().find(|a| a.name.to_lowercase() == who || a.slug == who) else {
                let names: Vec<&str> = delegates.iter().map(|a| a.name.as_str()).collect();
                return Err(CoreError::invalid(format!("'{who}' is not available here; you can delegate to: {}", names.join(", "))));
            };
            // Amelia only checks work the user asked to have checked.
            if target.specialty == "verifier" && !fallback::wants_verification(&turn.user_msg.content) {
                return Err(CoreError::Permission(format!(
                    "{} only double-checks work when the user explicitly asks for it. Do not call her unless they did.",
                    target.name
                )));
            }
            let mut task = AgentTask::new(
                Mode::Implement,
                Some(format!("{} asked you to handle this:\n\n{task_text}", caller.name)),
                false,
                json!({ "via": "delegation", "from": caller.name, "task": task_text }),
            );
            task.level_override = Some(level);
            task.delegated_by = Some(caller.name.clone());
            let m = self.run_agent(turn, target, task).await?;
            match m.status {
                MessageStatus::Done => Ok(format!("{} replied:\n{}", target.name, truncate(&m.content, 12_000))),
                MessageStatus::Cancelled => Err(CoreError::Cancelled),
                _ => Err(CoreError::invalid(format!("{} could not do it: {}", target.name, m.error.unwrap_or_else(|| "unknown error".into())))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use crate::db::Db;
    use crate::providers::{EventStream, Provider, ProviderEvent, Role, ToolCall};

    use super::*;

    struct Script;

    #[async_trait]
    impl Provider for Script {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Ollama
        }
        async fn list_models(&self) -> Result<Vec<ModelInfo>> {
            Ok(vec![])
        }
        async fn stream_chat(&self, req: ChatRequest) -> Result<EventStream> {
            let rick = req.system.starts_with("You are Rick");
            let mut ev = if rick && !req.messages.iter().any(|m| m.role == Role::Tool) {
                vec![ProviderEvent::ToolCall(ToolCall { id: "d".into(), name: "delegate_to_agent".into(), arguments: json!({ "agent": "Robert", "task": "status" }) })]
            } else {
                vec![ProviderEvent::TextDelta("done".into())]
            };
            ev.push(ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None });
            Ok(Box::pin(futures::stream::iter(ev)))
        }
    }

    /// A provider with a single free slot: the caller holds it, so the delegate can only run if the caller lets go.
    #[tokio::test]
    async fn a_delegate_runs_even_when_the_provider_has_one_slot_left() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(crate::secrets::MemoryStore::default())).unwrap();
        let cfg = core
            .save_provider(ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 }, None)
            .unwrap();
        core.override_provider(&cfg.id, Arc::new(Script));
        for slug in ["rick", "robert"] {
            let mut a = core.agent_by_slug(slug).unwrap();
            a.model = Some(ModelRef { provider_id: cfg.id.clone(), model_id: "m".into(), effort: None, vision: false, tools: true, context_window: None });
            core.update_agent(a).unwrap();
        }
        let (pcfg, _) = core.provider(&cfg.id).unwrap();
        let sem = core.providers.semaphore(&pcfg);
        let _taken = sem.acquire_many(sem.available_permits() as u32 - 1).await.unwrap();
        assert_eq!(sem.available_permits(), 1);

        let coding = core.list_chats().unwrap().into_iter().find(|c| c.name == "Coding").unwrap();
        let rick = core.agent_by_slug("rick").unwrap();
        core.send_message(&coding.id, "@Rick check the repo", vec![]).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let msgs = core.list_messages(&coding.id, 50).unwrap();
                if msgs.iter().any(|m| m.agent_id.as_deref() == Some(&rick.id) && m.status == MessageStatus::Done) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("deadlock: the delegate waited for a slot the caller was holding");
        let msgs = core.list_messages(&coding.id, 50).unwrap();
        let robert = core.agent_by_slug("robert").unwrap();
        assert!(msgs.iter().any(|m| m.agent_id.as_deref() == Some(&robert.id) && m.status == MessageStatus::Done));
    }
}
