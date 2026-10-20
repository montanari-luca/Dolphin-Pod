//! The nearby-network path: a WebSocket whose payload is a Noise `NNpsk0` channel keyed from the pod key.
//!
//! Only a device that holds the pod key can finish the handshake (it is the pre-shared key), and the channel is
//! encrypted with fresh ephemeral keys. Who is asking is then decided by the signed request inside, exactly as
//! on the cloud path. The desktop advertises itself over Bonjour (`_dolphinpod._tcp`); finding it is the
//! platform's job on the phone (Network.framework), so this module only needs a host and a port.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use snow::{Builder, TransportState};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::{WebSocketStream, accept_async, client_async};

use super::{LanEndpoint, RemoteRequest};
use crate::core::{Core, CoreRef};
use crate::error::{CoreError, Result};
use crate::events::{AgentActivity, CoreEvent};
use crate::model::Message;
use crate::sync::SyncEngine;

pub const SERVICE_TYPE: &str = "_dolphinpod._tcp.local.";
const PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const CHUNK: usize = 48 * 1024;
const MAX_FRAME: usize = 8 * 1024 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(2500);
const RUN_LIMIT: Duration = Duration::from_secs(20 * 60);

fn net(e: impl std::fmt::Display) -> CoreError {
    CoreError::Provider(format!("nearby connection: {e}"))
}

// ---------------------------------------------------------------------------------------------- channel

struct Channel<S> {
    ws: WebSocketStream<S>,
    noise: TransportState,
}

type Stream = tokio_tungstenite::MaybeTlsStream<TcpStream>;

async fn next_binary<S>(ws: &mut WebSocketStream<S>) -> Result<Vec<u8>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        match ws.next().await {
            Some(Ok(Ws::Binary(b))) => return Ok(b.to_vec()),
            Some(Ok(Ws::Ping(_) | Ws::Pong(_))) => continue,
            Some(Ok(Ws::Close(_))) | None => return Err(net("closed")),
            Some(Ok(_)) => return Err(net("unexpected frame")),
            Some(Err(e)) => return Err(net(e)),
        }
    }
}

async fn handshake_initiator<S>(mut ws: WebSocketStream<S>, psk: &[u8; 32]) -> Result<Channel<S>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut hs = Builder::new(PATTERN.parse().map_err(net)?).psk(0, psk).map_err(net)?.build_initiator().map_err(net)?;
    let mut buf = vec![0u8; 1024];
    let n = hs.write_message(&[], &mut buf).map_err(net)?;
    ws.send(Ws::Binary(buf[..n].to_vec().into())).await.map_err(net)?;
    let reply = tokio::time::timeout(HANDSHAKE_TIMEOUT, next_binary(&mut ws)).await.map_err(|_| net("handshake timed out"))??;
    hs.read_message(&reply, &mut buf).map_err(|_| net("handshake failed (is this device in the same pod?)"))?;
    Ok(Channel { ws, noise: hs.into_transport_mode().map_err(net)? })
}

async fn handshake_responder<S>(mut ws: WebSocketStream<S>, psk: &[u8; 32]) -> Result<Channel<S>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut hs = Builder::new(PATTERN.parse().map_err(net)?).psk(0, psk).map_err(net)?.build_responder().map_err(net)?;
    let mut buf = vec![0u8; 1024];
    let first = tokio::time::timeout(HANDSHAKE_TIMEOUT, next_binary(&mut ws)).await.map_err(|_| net("handshake timed out"))??;
    hs.read_message(&first, &mut buf).map_err(|_| net("handshake failed"))?;
    let n = hs.write_message(&[], &mut buf).map_err(net)?;
    ws.send(Ws::Binary(buf[..n].to_vec().into())).await.map_err(net)?;
    Ok(Channel { ws, noise: hs.into_transport_mode().map_err(net)? })
}

impl<S> Channel<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    /// One JSON value, split across as many encrypted frames as needed (first byte: 1 = more follows).
    async fn send(&mut self, v: &Value) -> Result<()> {
        let bytes = serde_json::to_vec(v)?;
        let chunks: Vec<&[u8]> = bytes.chunks(CHUNK).collect();
        let mut out = vec![0u8; CHUNK + 64];
        for (i, part) in chunks.iter().enumerate() {
            let mut plain = Vec::with_capacity(part.len() + 1);
            plain.push(u8::from(i + 1 < chunks.len()));
            plain.extend_from_slice(part);
            let n = self.noise.write_message(&plain, &mut out).map_err(net)?;
            self.ws.send(Ws::Binary(out[..n].to_vec().into())).await.map_err(net)?;
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<Value> {
        let mut assembled: Vec<u8> = Vec::new();
        let mut out = vec![0u8; CHUNK + 128];
        loop {
            let frame = next_binary(&mut self.ws).await?;
            let n = self.noise.read_message(&frame, &mut out).map_err(|_| net("a frame failed authentication"))?;
            if n == 0 {
                return Err(net("empty frame"));
            }
            assembled.extend_from_slice(&out[1..n]);
            if assembled.len() > MAX_FRAME {
                return Err(net("message too large"));
            }
            if out[0] == 0 {
                return Ok(serde_json::from_slice(&assembled)?);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- events on the wire

/// The slice of core events that make sense to stream to the device that asked for the work.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum WireEvent {
    MessageCreated { message: Message },
    MessageUpdated { message: Message },
    MessageDelta { chat_id: String, message_id: String, text: Option<String>, reasoning: Option<String> },
    MessagesRemoved { chat_id: String, message_ids: Vec<String>, all: bool },
    AgentActivity { chat_id: String, agent_id: String, activity: AgentActivity, detail: Option<String> },
}

fn to_wire(ev: &CoreEvent, chat: &str) -> Option<WireEvent> {
    Some(match ev {
        CoreEvent::MessageCreated { message } if message.chat_id == chat => WireEvent::MessageCreated { message: message.clone() },
        CoreEvent::MessageUpdated { message } if message.chat_id == chat => WireEvent::MessageUpdated { message: message.clone() },
        CoreEvent::MessageDelta { chat_id, message_id, text, reasoning } if chat_id == chat => WireEvent::MessageDelta {
            chat_id: chat_id.clone(),
            message_id: message_id.clone(),
            text: text.clone(),
            reasoning: reasoning.clone(),
        },
        CoreEvent::MessagesRemoved { chat_id, message_ids, all } if chat_id == chat => {
            WireEvent::MessagesRemoved { chat_id: chat_id.clone(), message_ids: message_ids.clone(), all: *all }
        }
        CoreEvent::AgentActivity { chat_id, agent_id, activity, detail } if chat_id == chat => {
            WireEvent::AgentActivity { chat_id: chat_id.clone(), agent_id: agent_id.clone(), activity: *activity, detail: detail.clone() }
        }
        _ => return None,
    })
}

/// Client side: store finished messages locally (without queueing them for sync) and tell the UI.
fn apply_wire(core: &Core, ev: WireEvent) {
    let event = match ev {
        WireEvent::MessageCreated { message } => {
            let _ = core.db.ingest_message(&message);
            CoreEvent::MessageCreated { message }
        }
        WireEvent::MessageUpdated { message } => {
            let _ = core.db.ingest_message(&message);
            CoreEvent::MessageUpdated { message }
        }
        WireEvent::MessageDelta { chat_id, message_id, text, reasoning } => CoreEvent::MessageDelta { chat_id, message_id, text, reasoning },
        WireEvent::MessagesRemoved { chat_id, message_ids, all } => CoreEvent::MessagesRemoved { chat_id, message_ids, all },
        WireEvent::AgentActivity { chat_id, agent_id, activity, detail } => CoreEvent::AgentActivity { chat_id, agent_id, activity, detail },
    };
    core.announce(event);
}

// ---------------------------------------------------------------------------------------------- host

pub struct LanServer {
    port: u16,
    task: JoinHandle<()>,
    mdns: Option<(mdns_sd::ServiceDaemon, String)>,
}

impl LanServer {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn start(core: CoreRef, psk: [u8; 32], advertise: bool) -> Result<Self> {
        let listener = TcpListener::bind("0.0.0.0:0").await.map_err(net)?;
        let port = listener.local_addr().map_err(net)?.port();
        let accept_core = core.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, peer)) = listener.accept().await else { break };
                let core = accept_core.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve(core, psk, stream).await {
                        tracing::debug!("nearby connection from {peer} ended: {e}");
                    }
                });
            }
        });
        let mdns = if advertise {
            match advertise_service(core.db.device_id(), port) {
                Ok(m) => Some(m),
                Err(e) => {
                    tracing::warn!("could not advertise on the local network: {e}");
                    None
                }
            }
        } else {
            None
        };
        Ok(Self { port, task, mdns })
    }
}

impl Drop for LanServer {
    fn drop(&mut self) {
        self.task.abort();
        if let Some((daemon, fullname)) = self.mdns.take() {
            let _ = daemon.unregister(&fullname);
            let _ = daemon.shutdown();
        }
    }
}

/// `dolphinpod-<8 chars>` with the device id in the TXT record, so the phone can match it to a target.
fn advertise_service(device_id: &str, port: u16) -> std::result::Result<(mdns_sd::ServiceDaemon, String), mdns_sd::Error> {
    let daemon = mdns_sd::ServiceDaemon::new()?;
    let short: String = device_id.chars().filter(|c| c.is_ascii_alphanumeric()).rev().take(8).collect();
    let instance = format!("dolphinpod-{short}");
    let host = format!("{instance}.local.");
    let props = [("id", device_id), ("v", "1")];
    let info = mdns_sd::ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", port, &props[..])?.enable_addr_auto();
    let fullname = info.get_fullname().to_string();
    daemon.register(info)?;
    Ok((daemon, fullname))
}

async fn serve(core: CoreRef, psk: [u8; 32], stream: TcpStream) -> Result<()> {
    let ws = tokio::time::timeout(HANDSHAKE_TIMEOUT, accept_async(stream)).await.map_err(|_| net("upgrade timed out"))?.map_err(net)?;
    let mut ch = handshake_responder(ws, &psk).await?;
    let first = tokio::time::timeout(Duration::from_secs(10), ch.recv()).await.map_err(|_| net("no request"))??;
    if first["t"] != "command" {
        return Err(net("expected a command"));
    }
    let req: RemoteRequest = serde_json::from_value(first["req"].clone())?;
    let sig = first["sig"].as_str().unwrap_or_default();
    if let Err(e) = core.remote_admit(&req, sig) {
        ch.send(&json!({ "t": "rejected", "reason": e.to_string() })).await?;
        return Err(e);
    }
    // Subscribe before the run starts so nothing the agents write is missed.
    let mut events = core.subscribe();
    let from = core.sync_device_name(&req.from_device_id).await;
    let run = match core.remote_execute(&req, &from).await {
        Ok(h) => h,
        Err(e) => {
            ch.send(&json!({ "t": "rejected", "reason": e.to_string() })).await?;
            return Err(e);
        }
    };
    ch.send(&json!({ "t": "accepted" })).await?;
    tokio::pin!(run);
    let deadline = tokio::time::sleep(RUN_LIMIT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(ev) => {
                    if let Some(w) = to_wire(&ev, &req.chat_id) {
                        ch.send(&event_frame(&w)?).await?;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            _ = &mut run => {
                // The pipeline is done; give the last events a moment to flush, then close.
                tokio::time::sleep(Duration::from_millis(150)).await;
                while let Ok(ev) = events.try_recv() {
                    if let Some(w) = to_wire(&ev, &req.chat_id) {
                        ch.send(&event_frame(&w)?).await?;
                    }
                }
                ch.send(&json!({ "t": "done" })).await?;
                break;
            }
            _ = &mut deadline => break,
        }
    }
    let _ = ch.ws.close(None).await;
    Ok(())
}

fn event_frame(w: &WireEvent) -> Result<Value> {
    Ok(json!({ "t": "event", "e": serde_json::to_value(w)? }))
}

// ---------------------------------------------------------------------------------------------- client

fn ws_url(ep: &LanEndpoint) -> String {
    if ep.host.contains(':') && !ep.host.starts_with('[') { format!("ws://[{}]:{}/", ep.host, ep.port) } else { format!("ws://{}:{}/", ep.host, ep.port) }
}

/// Deliver `req` to the desktop at `ep`. Returns once the desktop has accepted it; the run's events are then
/// applied in the background until it finishes. `Permission` errors mean the desktop refused: don't retry elsewhere.
pub async fn send(core: &CoreRef, engine: &Arc<SyncEngine>, ep: &LanEndpoint, req: &RemoteRequest, sig: &str) -> Result<()> {
    let url = ws_url(ep);
    let connect = async {
        let tcp = TcpStream::connect((ep.host.as_str(), ep.port)).await.map_err(net)?;
        let _ = tcp.set_nodelay(true);
        let (ws, _) = client_async(url.as_str(), tokio_tungstenite::MaybeTlsStream::Plain(tcp)).await.map_err(net)?;
        let ws: WebSocketStream<Stream> = ws;
        handshake_initiator(ws, &engine.key().link_key()).await
    };
    let mut ch = tokio::time::timeout(HANDSHAKE_TIMEOUT, connect).await.map_err(|_| net("timed out"))??;
    ch.send(&json!({ "t": "command", "req": req, "sig": sig })).await?;
    let reply = tokio::time::timeout(Duration::from_secs(4), ch.recv()).await.map_err(|_| net("no answer"))??;
    match reply["t"].as_str() {
        Some("accepted") => {}
        Some("rejected") => return Err(CoreError::Permission(reply["reason"].as_str().unwrap_or("refused").to_string())),
        _ => return Err(net("unexpected answer")),
    }
    let core = core.clone();
    tokio::spawn(async move {
        loop {
            match ch.recv().await {
                Ok(v) if v["t"] == "event" => {
                    if let Ok(w) = serde_json::from_value::<WireEvent>(v["e"].clone()) {
                        apply_wire(&core, w);
                    }
                }
                Ok(v) if v["t"] == "done" => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::protocol::Role;

    async fn pair(psk_a: [u8; 32], psk_b: [u8; 32]) -> (Result<Channel<tokio::io::DuplexStream>>, Result<Channel<tokio::io::DuplexStream>>) {
        let (a, b) = tokio::io::duplex(1 << 20);
        let client = WebSocketStream::from_raw_socket(a, Role::Client, None).await;
        let server = WebSocketStream::from_raw_socket(b, Role::Server, None).await;
        tokio::join!(handshake_initiator(client, &psk_a), handshake_responder(server, &psk_b))
    }

    #[tokio::test]
    async fn large_messages_are_chunked_and_reassembled_over_the_noise_channel() {
        let (c, s) = pair([7; 32], [7; 32]).await;
        let (mut c, mut s) = (c.unwrap(), s.unwrap());
        let big = json!({ "text": "x".repeat(300_000), "n": 1 });
        let (sent, got) = tokio::join!(c.send(&big), s.recv());
        sent.unwrap();
        assert_eq!(got.unwrap(), big);
        let ok = json!({ "t": "ok" });
        let (sent, got) = tokio::join!(s.send(&ok), c.recv());
        sent.unwrap();
        assert_eq!(got.unwrap(), ok);
    }

    #[tokio::test]
    async fn a_device_without_the_pod_key_cannot_finish_the_handshake() {
        let (c, s) = pair([1; 32], [2; 32]).await;
        assert!(c.is_err() || s.is_err(), "different pre-shared keys must not produce a channel");
    }
}

#[cfg(test)]
mod bonjour {
    use super::*;

    /// Needs a network interface that allows multicast, so it is opt-in: `cargo test -p dolphin-core bonjour -- --ignored`.
    #[test]
    #[ignore = "needs multicast networking"]
    fn the_desktop_is_discoverable_as_a_dolphinpod_service() {
        let (daemon, _full) = advertise_service("019f2b3c-4d5e-7000-8000-0123456789ab", 48123).expect("advertise");
        let browser = mdns_sd::ServiceDaemon::new().unwrap();
        let rx = browser.browse(SERVICE_TYPE).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if let Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) = rx.recv_timeout(Duration::from_millis(500)) {
                assert_eq!(info.get_port(), 48123);
                assert_eq!(info.get_property_val_str("id"), Some("019f2b3c-4d5e-7000-8000-0123456789ab"));
                let _ = daemon.shutdown();
                return;
            }
        }
        panic!("the service was never discovered");
    }
}
