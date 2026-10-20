//! On-device inference: a model that runs on the phone itself. The core only defines the seam; the iOS app
//! plugs in a runtime (MLX) that knows how to load and run its curated list of small reasoning models.
//!
//! The provider is virtual: it has a fixed id, is never stored or synced, and exists only while a runtime is
//! registered, so the desktop never sees it.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::{ChatRequest, EventStream, Part, Provider, ProviderEvent, Role, Usage};
use crate::error::Result;
use crate::model::{EffortControl, ModelInfo, ProviderKind};

/// The id of the virtual provider (also the `provider_id` of an agent's on-device `ModelRef`).
pub const ON_DEVICE_PROVIDER_ID: &str = "on-device";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnDeviceModel {
    pub id: String,
    pub name: String,
    /// Download size, for the picker.
    pub size_mb: u32,
    pub context_window: u32,
    /// Weights are on the phone already.
    pub downloaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnDeviceMessage {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnDeviceRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<OnDeviceMessage>,
    pub want_reasoning: bool,
    pub max_tokens: Option<u32>,
}

/// Where a runtime writes what it generates.
pub trait OnDeviceSink: Send + Sync {
    fn text(&self, delta: &str);
    fn reasoning(&self, delta: &str);
    fn finish(&self, input_tokens: i64, output_tokens: i64);
    fn fail(&self, message: &str);
    /// True once the user stopped the run; a runtime should stop generating.
    fn is_cancelled(&self) -> bool;
}

#[async_trait]
pub trait OnDeviceRuntime: Send + Sync {
    /// The curated models this phone can run.
    fn models(&self) -> Vec<OnDeviceModel>;
    /// Generate a reply, streaming into `sink`. Returning without calling `finish` counts as finished.
    async fn generate(&self, request: OnDeviceRequest, sink: Arc<dyn OnDeviceSink>);
}

pub struct OnDeviceProvider {
    runtime: Arc<dyn OnDeviceRuntime>,
}

impl OnDeviceProvider {
    pub fn new(runtime: Arc<dyn OnDeviceRuntime>) -> Self {
        Self { runtime }
    }
}

struct ChannelSink {
    tx: mpsc::UnboundedSender<ProviderEvent>,
}

impl OnDeviceSink for ChannelSink {
    fn text(&self, delta: &str) {
        let _ = self.tx.send(ProviderEvent::TextDelta(delta.into()));
    }
    fn reasoning(&self, delta: &str) {
        let _ = self.tx.send(ProviderEvent::ReasoningDelta(delta.into()));
    }
    fn finish(&self, input_tokens: i64, output_tokens: i64) {
        let _ = self.tx.send(ProviderEvent::Usage(Usage { input_tokens, output_tokens }));
        let _ = self.tx.send(ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None });
    }
    fn fail(&self, message: &str) {
        let _ = self.tx.send(ProviderEvent::Error(message.into()));
    }
    fn is_cancelled(&self) -> bool {
        self.tx.is_closed()
    }
}

#[async_trait]
impl Provider for OnDeviceProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OnDevice
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(self
            .runtime
            .models()
            .into_iter()
            .map(|m| ModelInfo {
                id: m.id,
                name: format!("{} ({} MB{})", m.name, m.size_mb, if m.downloaded { "" } else { ", downloads on first use" }),
                reasoning: true,
                reason: None,
                effort: EffortControl::None,
                vision: false,
                // Small local models don't get tool definitions; they answer in text.
                tools: false,
                image_output: false,
                context_window: Some(m.context_window),
                free: true,
            })
            .collect())
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<EventStream> {
        let (tx, rx) = mpsc::unbounded_channel();
        let request = OnDeviceRequest {
            model: req.model.clone(),
            system: req.system.clone(),
            messages: req
                .messages
                .iter()
                .filter(|m| m.role != Role::Tool)
                .map(|m| OnDeviceMessage {
                    role: if m.role == Role::Assistant { "assistant".into() } else { "user".into() },
                    text: m.parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n"),
                })
                .collect(),
            want_reasoning: req.want_reasoning,
            max_tokens: req.max_tokens,
        };
        let runtime = self.runtime.clone();
        let sink: Arc<dyn OnDeviceSink> = Arc::new(ChannelSink { tx: tx.clone() });
        tokio::spawn(async move {
            runtime.generate(request, sink).await;
            // A runtime that returns without `finish` still ends the stream cleanly.
            let _ = tx.send(ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None });
        });
        Ok(Box::pin(UnboundedReceiverStream::new(rx)))
    }
}
