//! `com.listo.mqtt-client` — native process block.
//!
//! Three kinds:
//!
//!   * `com.listo.mqtt-client.client`  — holds broker settings + session.
//!     The manifest declares `state` / `detail` status slots (the Studio
//!     reads them for the PLC-style node indicator), but the process-block
//!     SDK's `GraphAccess` is a stub today (see
//!     `agent-sdk/blocks-sdk/src/process.rs` — `StubGraph`), so the
//!     event-loop task can only log. As soon as the slot-write RPC lands
//!     (`NODE-RED-MODEL.md` Stage 3b/3c) the event-loop branch below
//!     switches to `graph.write_slot`.
//!   * `com.listo.mqtt-client.pub`     — publishes inbound msgs to a topic.
//!     Supports Node-RED-style per-msg topic/qos/retain overrides and emits
//!     a stats msg on `out` synchronously after the publish is queued.
//!   * `com.listo.mqtt-client.sub`     — subscribes to a topic (output wiring
//!     pending the same streaming-emit RPC as the client-state watcher).
//!
//! All three share a process-wide `Registry` of MQTT sessions keyed by
//! the client node's path, so pub/sub look their parent client's
//! session up by walking one path segment up.
//!
//! Stateless-behaviour contract (per NODE-SCOPE.md): the `NodeBehavior`
//! impls are unit structs; the connection pool is in a `OnceLock` —
//! process-wide state, legitimate because it's operational, not per-
//! instance.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use rumqttc::{
    matches as topic_matches, AsyncClient, ConnectionError, Event, EventLoop, Incoming,
    MqttOptions, Publish as MqttPublish, QoS,
};
use serde::Deserialize;
use serde_json::json;

use blocks_sdk::{
    ctx::NodeCtx,
    error::NodeError,
    node::{InputPort, NodeBehavior},
    process::{publish_slot_event, run_process_plugin, BlockIdentity},
    Msg, NodeKind, NodePath,
};
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Kind — client
// ---------------------------------------------------------------------------

#[derive(NodeKind)]
#[node(
    kind = "com.listo.mqtt-client.client",
    manifest = "../kinds/client.yaml",
    behavior = "custom"
)]
pub struct Client;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ClientConfig {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub keep_alive_secs: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 1883,
            client_id: String::new(),
            username: None,
            password: None,
            keep_alive_secs: 60,
        }
    }
}

impl NodeBehavior for Client {
    type Config = ClientConfig;

    fn on_init(&self, ctx: &NodeCtx, cfg: &Self::Config) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
        let cfg = cfg.clone();
        tracing::info!(node = %path.as_str(), host = %cfg.host, port = cfg.port, "client on_init — opening session");
        publish_client_state(&path, "CONNECTING", "opening connection");
        tokio::spawn(async move {
            registry().connect(path, cfg).await;
        });
        Ok(())
    }

    fn on_message(&self, _ctx: &NodeCtx, _port: InputPort, _msg: Msg) -> Result<(), NodeError> {
        // Client has no input ports — this is unreachable in practice.
        Ok(())
    }

    fn on_config_change(&self, ctx: &NodeCtx, cfg: &Self::Config) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
        let cfg = cfg.clone();
        tracing::info!(node = %path.as_str(), host = %cfg.host, port = cfg.port, "client on_config_change — reopening session");
        tokio::spawn(async move {
            registry().disconnect(&path).await;
            registry().connect(path, cfg).await;
        });
        Ok(())
    }

    fn on_shutdown(&self, ctx: &NodeCtx) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
        publish_client_state(&path, "OFF", "shut down");
        tokio::spawn(async move {
            registry().disconnect(&path).await;
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Kind — publish
// ---------------------------------------------------------------------------

#[derive(NodeKind)]
#[node(
    kind = "com.listo.mqtt-client.pub",
    manifest = "../kinds/pub.yaml",
    behavior = "custom"
)]
pub struct Publish;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PublishConfig {
    pub topic: String,
    pub qos: u8,
    pub retain: bool,
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            topic: String::new(),
            qos: 0,
            retain: false,
        }
    }
}

impl NodeBehavior for Publish {
    type Config = PublishConfig;

    fn on_message(&self, ctx: &NodeCtx, port: InputPort, msg: Msg) -> Result<(), NodeError> {
        if port != "in" {
            return Err(NodeError::runtime(format!("unexpected port `{port}`")));
        }

        // Resolve settings honouring msg_overrides declared in the
        // manifest (qos / retain / topic from msg.metadata). Topic also
        // accepts `msg.topic` top-level per Node-RED convention.
        // Node-RED mqtt-out parity: settings on the node are defaults;
        // matching fields on the inbound msg override per-message.
        //
        //   msg.payload   → bytes to publish (string → UTF-8; other → JSON)
        //   msg.topic     → topic (top-level on Msg; beats cfg.topic)
        //   msg.qos       → qos 0/1/2 (flattens into metadata → resolve_settings)
        //   msg.retain    → retained flag   (same path as qos)
        //
        // `msg.qos`/`msg.retain` land in `msg.metadata` thanks to the
        // flatten-on-deserialize on `spi::Msg`, so the manifest's
        // `msg_overrides` picks them up. `msg.topic` is separate
        // because the Msg struct models it as a first-class field, not
        // an arbitrary metadata key.
        let cfg: PublishConfig = ctx.resolve_settings::<PublishConfig>(&msg)?.into_inner();
        let topic = msg
            .topic
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or(cfg.topic);
        if topic.is_empty() {
            let err = "pub: no topic configured and msg.topic is empty".to_string();
            emit_stats(ctx, false, "", cfg.qos, cfg.retain, 0, Some(&err));
            return Err(NodeError::runtime(err));
        }

        // Pub/Sub share the parent client's MQTT session.
        let Some(parent_path) = ctx.node_path().parent() else {
            return Err(NodeError::runtime(
                "pub node must live under a client — has no parent",
            ));
        };

        let qos = parse_qos(cfg.qos);
        let payload = payload_bytes(&msg);
        let bytes = payload.len() as u64;

        // `client.publish()` on rumqttc just queues onto an in-process
        // mpsc channel (the event loop does the network work), so it's
        // fast and safe to block_on inside an RPC dispatch. Running it
        // synchronously is what lets us emit the stats envelope on `out`
        // within the same `on_message` call — the process-block SDK
        // only returns emits captured during the sync dispatch (see
        // agent-sdk/blocks-sdk/src/process.rs::CapturingEmitSink).
        let publish_result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                match registry().handle(&parent_path).await {
                    Some(client) => client.publish(&topic, qos, cfg.retain, payload).await
                        .map_err(|e| e.to_string()),
                    None => Err(format!(
                        "parent client `{}` not connected",
                        parent_path.as_str()
                    )),
                }
            })
        });

        match publish_result {
            Ok(()) => {
                emit_stats(ctx, true, &topic, cfg.qos, cfg.retain, bytes, None);
                Ok(())
            }
            Err(err) => {
                tracing::warn!(%topic, error = %err, "mqtt publish failed");
                emit_stats(ctx, false, &topic, cfg.qos, cfg.retain, bytes, Some(&err));
                // Don't propagate — the stats envelope on `out` is the
                // error surface for downstream. Bubbling here would log
                // twice and Node-RED pub nodes swallow transport errors.
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Kind — subscribe (streaming-emit wiring pending)
// ---------------------------------------------------------------------------

#[derive(NodeKind)]
#[node(
    kind = "com.listo.mqtt-client.sub",
    manifest = "../kinds/sub.yaml",
    behavior = "custom"
)]
pub struct Subscribe;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SubscribeConfig {
    pub topic: String,
    pub qos: u8,
}

impl Default for SubscribeConfig {
    fn default() -> Self {
        Self {
            topic: String::new(),
            qos: 0,
        }
    }
}

impl NodeBehavior for Subscribe {
    type Config = SubscribeConfig;

    fn on_init(&self, ctx: &NodeCtx, cfg: &Self::Config) -> Result<(), NodeError> {
        if cfg.topic.is_empty() {
            return Ok(());
        }
        let Some(parent_path) = ctx.node_path().parent() else {
            return Err(NodeError::runtime(
                "sub node must live under a client — has no parent",
            ));
        };
        let sub_path = ctx.node_path().clone();
        let topic = cfg.topic.clone();
        let qos = parse_qos(cfg.qos);

        // Register this sub with its client's fan-out table BEFORE the
        // broker ACKs the subscription, so a Publish delivered in the
        // same tick doesn't race us and drop. The event-loop pump
        // (drive_event_loop) matches every incoming Publish against
        // the filter we register here.
        let parent_for_reg = parent_path.clone();
        let topic_for_reg = topic.clone();
        tokio::spawn(async move {
            // Always register the filter; the event loop replays all
            // registered subs to the broker on every `ConnAck` (see
            // `drive_event_loop`). So if the client isn't up yet, the
            // subscribe still lands as soon as the session connects —
            // no second init call needed.
            registry()
                .register_sub(&parent_for_reg, sub_path, topic_for_reg, qos)
                .await;
            // Opportunistic fast path: if the client is already up,
            // send the subscribe now instead of waiting for the next
            // ConnAck (which might never come if we connected long ago
            // and are sitting healthy).
            if let Some(client) = registry().handle(&parent_for_reg).await {
                if let Err(e) = client.subscribe(&topic, qos).await {
                    tracing::warn!(%topic, error = %e, "mqtt subscribe failed");
                }
            }
        });
        Ok(())
    }

    fn on_message(&self, _ctx: &NodeCtx, _port: InputPort, _msg: Msg) -> Result<(), NodeError> {
        // `sub` has no inputs. Inbound MQTT messages arrive on the
        // parent client's event loop and are forwarded via
        // `publish_slot_event(sub_path, "out", msg_json)`.
        Ok(())
    }

    fn on_shutdown(&self, ctx: &NodeCtx) -> Result<(), NodeError> {
        let Some(parent_path) = ctx.node_path().parent() else {
            return Ok(());
        };
        let sub_path = ctx.node_path().clone();
        tokio::spawn(async move {
            registry().unregister_sub(&parent_path, &sub_path).await;
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Process-wide MQTT registry
// ---------------------------------------------------------------------------

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(Registry::new)
}

struct Registry {
    sessions: Mutex<HashMap<NodePath, Session>>,
    /// Sub-node fan-out table, keyed by parent client path. The event
    /// loop walks the vec for each incoming Publish and dispatches to
    /// matching filters. A `Mutex` (not `RwLock`) is fine: churn is
    /// low — writes only on sub create/destroy — and reads are on the
    /// MQTT event loop which is already `.await`-heavy.
    subs: Mutex<HashMap<NodePath, Vec<SubEntry>>>,
}

#[derive(Clone)]
struct SubEntry {
    sub_path: NodePath,
    filter: String,
    qos: QoS,
}

struct Session {
    client: AsyncClient,
    /// Handle for the event-loop task. We call `abort()` on it
    /// explicitly when replacing the session — dropping a tokio
    /// `JoinHandle` *detaches* the task rather than cancelling it, so
    /// without this the old event loop keeps polling its socket and
    /// reconnecting with the same `client_id`, fighting the new
    /// session and producing the kick-loop we saw in prod.
    task: tokio::task::JoinHandle<()>,
}

impl Session {
    /// Stop the MQTT session cleanly: send a DISCONNECT packet so the
    /// broker frees the client-id slot immediately, then abort the
    /// event-loop task. Both steps are best-effort — a dead socket is
    /// already "disconnected" from the broker's POV, and an aborted
    /// task is dropped right after.
    async fn shutdown(self) {
        let _ = self.client.disconnect().await;
        self.task.abort();
    }
}

impl Registry {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            subs: Mutex::new(HashMap::new()),
        }
    }

    async fn register_sub(
        &self,
        client_path: &NodePath,
        sub_path: NodePath,
        filter: String,
        qos: QoS,
    ) {
        let mut subs = self.subs.lock().await;
        let list = subs.entry(client_path.clone()).or_default();
        // Replace existing entry for this sub_path (on_init re-run after
        // settings change) rather than duplicating.
        list.retain(|e| e.sub_path != sub_path);
        list.push(SubEntry {
            sub_path,
            filter,
            qos,
        });
    }

    async fn unregister_sub(&self, client_path: &NodePath, sub_path: &NodePath) {
        let mut subs = self.subs.lock().await;
        if let Some(list) = subs.get_mut(client_path) {
            list.retain(|e| e.sub_path != *sub_path);
            if list.is_empty() {
                subs.remove(client_path);
            }
        }
    }

    async fn subs_for(&self, client_path: &NodePath) -> Vec<SubEntry> {
        let subs = self.subs.lock().await;
        subs.get(client_path).cloned().unwrap_or_default()
    }

    /// Open a connection for a client node. A second call under the same
    /// path replaces the previous session (on_config_change case).
    async fn connect(&self, path: NodePath, cfg: ClientConfig) {
        let client_id = if cfg.client_id.is_empty() {
            path.as_str().to_owned()
        } else {
            cfg.client_id
        };

        let mut opts = MqttOptions::new(client_id, &cfg.host, cfg.port);
        opts.set_keep_alive(Duration::from_secs(cfg.keep_alive_secs));
        if let (Some(u), Some(p)) = (cfg.username.as_deref(), cfg.password.as_deref()) {
            if !u.is_empty() {
                opts.set_credentials(u, p);
            }
        }

        let (client, mut event_loop) = AsyncClient::new(opts, 64);

        let path_for_task = path.clone();
        let task = tokio::spawn(async move {
            drive_event_loop(path_for_task, &mut event_loop).await;
        });

        // Take the old session out of the map UNDER the lock, then
        // release the lock before calling `shutdown().await` — the
        // disconnect-send is a point of async work we don't want
        // holding the Mutex while every other caller waits.
        let previous = {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(path.clone(), Session { client, task })
        };
        if let Some(old) = previous {
            old.shutdown().await;
        }
    }

    async fn disconnect(&self, path: &NodePath) {
        let previous = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(path)
        };
        if let Some(old) = previous {
            old.shutdown().await;
        }
    }

    async fn handle(&self, path: &NodePath) -> Option<ClientHandle> {
        let sessions = self.sessions.lock().await;
        sessions.get(path).map(|s| ClientHandle {
            client: s.client.clone(),
        })
    }
}

struct ClientHandle {
    client: AsyncClient,
}

impl ClientHandle {
    async fn publish(
        &self,
        topic: &str,
        qos: QoS,
        retain: bool,
        payload: Vec<u8>,
    ) -> Result<(), rumqttc::ClientError> {
        self.client.publish(topic, qos, retain, payload).await
    }

    async fn subscribe(&self, topic: &str, qos: QoS) -> Result<(), rumqttc::ClientError> {
        self.client.subscribe(topic, qos).await
    }
}

/// Event-loop pump. rumqttc handles TCP reconnect internally; the branches
/// below classify each transition for logs today and for slot-state writes
/// once the process-side `GraphAccess` RPC lands (see module docs).
async fn drive_event_loop(path: NodePath, el: &mut EventLoop) {
    loop {
        match el.poll().await {
            Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                tracing::info!(node = %path.as_str(), state = "OK", "mqtt connected");
                publish_client_state(&path, "OK", "connected");
                // Replay every registered sub filter on (re)connect.
                // Covers two cases:
                //   1. A sub node's `on_init` fired before the client's
                //      async connect completed (boot-time race).
                //   2. The broker dropped us and rumqttc reconnected —
                //      MQTT clean_session=true (our default) means the
                //      broker forgot our subscriptions, so re-send them.
                let path_for_replay = path.clone();
                tokio::spawn(async move {
                    replay_subs(&path_for_replay).await;
                });
            }
            Ok(Event::Incoming(Incoming::Publish(p))) => {
                dispatch_publish(&path, &p).await;
            }
            Ok(Event::Incoming(Incoming::Disconnect)) => {
                tracing::warn!(node = %path.as_str(), state = "WARNING", "broker disconnected");
                publish_client_state(&path, "WARNING", "broker disconnected");
            }
            Ok(event) => {
                tracing::debug!(node = %path.as_str(), ?event, "mqtt event");
            }
            Err(e) => {
                let (state, detail) = classify_error(&e);
                tracing::warn!(node = %path.as_str(), state, error = %e, "mqtt connection error");
                publish_client_state(&path, state, &detail);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Re-send every sub filter registered under this client. Called from
/// the event loop on each `ConnAck` — the broker's subscription state
/// is session-scoped, so after any (re)connect we have to restate our
/// subscriptions. Failures are logged per-filter; one bad topic
/// mustn't block the rest.
async fn replay_subs(client_path: &NodePath) {
    let subs = registry().subs_for(client_path).await;
    if subs.is_empty() {
        return;
    }
    let Some(client) = registry().handle(client_path).await else {
        return;
    };
    for entry in subs {
        if let Err(e) = client.subscribe(&entry.filter, entry.qos).await {
            tracing::warn!(
                filter = %entry.filter, sub = %entry.sub_path.as_str(), error = %e,
                "mqtt replay subscribe failed",
            );
        }
    }
}

/// For each sub-node under this client whose filter matches the
/// incoming topic, build a Node-RED-style `Msg` and push it onto the
/// process-wide slot-event bus. The agent-side consumer (see
/// `blocks-host/src/host.rs::run_slot_event_consumer`) writes it to
/// each sub's `out` slot.
async fn dispatch_publish(client_path: &NodePath, p: &MqttPublish) {
    let subs = registry().subs_for(client_path).await;
    if subs.is_empty() {
        return;
    }
    let payload = decode_payload(&p.payload);
    for entry in subs {
        if !topic_matches(&p.topic, &entry.filter) {
            continue;
        }
        let msg = Msg::new(payload.clone()).with_topic(p.topic.clone());
        let Ok(msg_json) = serde_json::to_value(&msg) else {
            continue;
        };
        let delivered = publish_slot_event(&entry.sub_path, "out", &msg_json);
        if delivered == 0 {
            tracing::debug!(
                sub = %entry.sub_path.as_str(), topic = %p.topic,
                "slot-event bus has no agent-side subscriber yet — dropping",
            );
        }
    }
}

/// Payloads coming off the broker are `Vec<u8>`. Treat valid JSON as
/// structured data (Node-RED parity — `msg.payload` becomes an object);
/// fall back to UTF-8 string, then to a base64-less byte array.
fn decode_payload(bytes: &[u8]) -> serde_json::Value {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) {
        return v;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => json!(s),
        Err(_) => json!(bytes.to_vec()),
    }
}

fn publish_client_state(path: &NodePath, state: &str, detail: &str) {
    // `state` is scalar-per-slot (value_kind: string); `detail` same.
    // Matches the manifest in kinds/client.yaml exactly — keep in sync.
    let _ = publish_slot_event(path, "state", &json!(state));
    let _ = publish_slot_event(path, "detail", &json!(detail));
}

fn classify_error(e: &ConnectionError) -> (&'static str, String) {
    match e {
        ConnectionError::ConnectionRefused(_) | ConnectionError::NotConnAck(_) => {
            ("ERROR", e.to_string())
        }
        _ => ("WARNING", e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Publish stats emit
// ---------------------------------------------------------------------------

/// Build and emit the publish-stats envelope on the pub node's `out`
/// port. Called synchronously from inside `on_message` so the SDK's
/// `CapturingEmitSink` picks it up and returns it with the RPC response.
fn emit_stats(
    ctx: &NodeCtx,
    success: bool,
    topic: &str,
    qos: u8,
    retain: bool,
    bytes: u64,
    error: Option<&str>,
) {
    let mut payload = serde_json::Map::new();
    payload.insert("success".into(), json!(success));
    payload.insert("topic".into(), json!(topic));
    payload.insert("qos".into(), json!(qos));
    payload.insert("retain".into(), json!(retain));
    payload.insert("bytes".into(), json!(bytes));
    if let Some(e) = error {
        payload.insert("error".into(), json!(e));
    }
    let msg = Msg::new(serde_json::Value::Object(payload)).with_topic(topic.to_owned());
    if let Err(e) = ctx.emit("out", msg) {
        tracing::warn!(error = %e, "pub: stats emit failed");
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_qos(n: u8) -> QoS {
    match n {
        0 => QoS::AtMostOnce,
        1 => QoS::AtLeastOnce,
        _ => QoS::ExactlyOnce,
    }
}

/// Best-effort encoding of a `Msg` into a publishable payload. String
/// payloads go out as UTF-8; anything else is JSON-encoded.
fn payload_bytes(msg: &Msg) -> Vec<u8> {
    match &msg.payload {
        serde_json::Value::String(s) => s.as_bytes().to_vec(),
        other => serde_json::to_vec(other).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut identity = BlockIdentity::new("com.listo.mqtt-client", "0.1.0");
    identity.register_kind(Client);
    identity.register_kind(Publish);
    identity.register_kind(Subscribe);
    run_process_plugin(identity).await?;
    Ok(())
}
