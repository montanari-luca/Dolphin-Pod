//! On the phone an agent uses its iPhone model, and a computer-only model is reported as unavailable.

use std::sync::Arc;

use dolphin_core::db::Db;
use dolphin_core::model::*;
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreRef};
use serde_json::json;

fn provider(core: &CoreRef, kind: ProviderKind, base_url: &str) -> ProviderConfig {
    core.save_provider(
        ProviderConfig { id: String::new(), kind, name: kind.label().into(), base_url: base_url.into(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
        Some("key".into()),
    )
    .unwrap()
}

fn model(p: &ProviderConfig, id: &str) -> ModelRef {
    ModelRef { provider_id: p.id.clone(), model_id: id.into(), effort: None, vision: false, tools: true, context_window: None }
}

#[test]
fn ios_prefers_its_override_and_skips_models_it_cannot_reach() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let ollama = provider(&core, ProviderKind::Ollama, "http://localhost:11434");
    let router = provider(&core, ProviderKind::Openrouter, "https://openrouter.ai/api/v1");
    let lan_server = provider(&core, ProviderKind::CustomOpenai, "http://192.168.1.20:8000/v1");
    let loopback = provider(&core, ProviderKind::CustomOpenai, "http://127.0.0.1:8000/v1");

    let mut rick = core.list_agents().unwrap().into_iter().find(|a| a.slug == "rick").unwrap();
    rick.model = Some(model(&ollama, "qwen3.5:2b"));
    let rick = core.update_agent(rick).unwrap();

    // Desktop: the desktop model, always.
    assert_eq!(core.model_for(&rick).unwrap().model_id, "qwen3.5:2b");

    core.set_platform(Platform::Ios);
    assert!(core.model_for(&rick).is_none(), "a local Ollama model is unavailable on the phone");
    assert!(core.no_model_reason_for_tests(&rick).contains("runs on your computer"));

    let mut with_override = rick.clone();
    with_override.ios_model = Some(model(&router, "some/reasoning-model"));
    let with_override = core.update_agent(with_override).unwrap();
    assert_eq!(core.model_for(&with_override).unwrap().model_id, "some/reasoning-model");

    // A cloud desktop model is fine on the phone when there is no override; an override wins when there is one.
    let mut cloud = with_override.clone();
    cloud.model = Some(model(&router, "big/cloud-model"));
    cloud.ios_model = None;
    assert_eq!(core.model_for(&cloud).unwrap().model_id, "big/cloud-model");
    cloud.ios_model = Some(model(&router, "small/phone-model"));
    assert_eq!(core.model_for(&cloud).unwrap().model_id, "small/phone-model");

    // A server elsewhere on the network works; one on the PC's own localhost does not.
    let mut custom = cloud.clone();
    custom.ios_model = None;
    custom.model = Some(model(&lan_server, "m"));
    assert!(core.model_for(&custom).is_some());
    custom.model = Some(model(&loopback, "m"));
    assert!(core.model_for(&custom).is_none());
}

mod on_device {
    use std::sync::Mutex;
    use std::time::Duration;

    use async_trait::async_trait;
    use dolphin_core::providers::ondevice::*;

    use super::*;

    /// A stand-in for the MLX runtime: answers the moderator with JSON and everything else with text.
    struct Fake {
        requests: Mutex<Vec<OnDeviceRequest>>,
    }

    #[async_trait]
    impl OnDeviceRuntime for Fake {
        fn models(&self) -> Vec<OnDeviceModel> {
            vec![OnDeviceModel { id: "tiny-reasoner-4bit".into(), name: "Tiny reasoner".into(), size_mb: 900, context_window: 8192, downloaded: true }]
        }

        async fn generate(&self, request: OnDeviceRequest, sink: Arc<dyn OnDeviceSink>) {
            self.requests.lock().unwrap().push(request.clone());
            if request.system.contains("moderator of \"Dolphin Pod\"") {
                sink.text(&json!({"intent": "greeting", "mode": "answer", "targets": []}).to_string());
            } else {
                sink.reasoning("the user said hello");
                sink.text("Hello from the phone itself.");
            }
            sink.finish(12, 6);
        }
    }

    #[tokio::test]
    async fn agents_can_run_on_a_model_inside_the_phone() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
        core.set_platform(Platform::Ios);
        // Without a runtime there is no such provider.
        assert!(core.list_providers().unwrap().iter().all(|p| p.kind != ProviderKind::OnDevice));
        let fake = Arc::new(Fake { requests: Mutex::new(vec![]) });
        core.set_on_device_runtime(fake.clone());

        let listed = core.list_providers().unwrap();
        let virtual_provider = listed.iter().find(|p| p.kind == ProviderKind::OnDevice).expect("virtual provider");
        assert_eq!(virtual_provider.id, ON_DEVICE_PROVIDER_ID);
        assert!(core.provider_kinds().iter().all(|k| k.kind != ProviderKind::OnDevice), "not something to add by hand");
        let models = core.list_models(ON_DEVICE_PROVIDER_ID).await.unwrap();
        assert_eq!(models.len(), 1);
        assert!(models[0].reasoning && !models[0].tools);
        assert!(core.save_provider(virtual_provider.clone(), None).is_err());

        let pick = ModelRef { provider_id: ON_DEVICE_PROVIDER_ID.into(), model_id: "tiny-reasoner-4bit".into(), effort: None, vision: false, tools: false, context_window: Some(8192) };
        for slug in ["roger", "lincoln"] {
            let mut a = core.list_agents().unwrap().into_iter().find(|a| a.slug == slug).unwrap();
            a.ios_model = Some(pick.clone());
            core.update_agent(a).unwrap();
        }
        let roger = core.list_agents().unwrap().into_iter().find(|a| a.slug == "roger").unwrap();
        assert_eq!(core.model_for(&roger), Some(pick.clone()));

        let chat = core.list_chats().unwrap().remove(0);
        core.send_message(&chat.id, "hello", vec![]).await.unwrap();
        let mut answer = None;
        for _ in 0..200 {
            answer = core.list_messages(&chat.id, 50).unwrap().into_iter().find(|m| m.content.contains("Hello from the phone itself."));
            if answer.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let answer = answer.expect("the on-device model answered");
        assert_eq!(answer.reasoning.as_deref(), Some("the user said hello"));
        assert!(fake.requests.lock().unwrap().iter().any(|r| r.model == "tiny-reasoner-4bit"));

        // On the desktop the same model reference is simply unusable.
        core.set_platform(Platform::Desktop);
        assert!(!core.model_usable_here(&pick));
    }
}
