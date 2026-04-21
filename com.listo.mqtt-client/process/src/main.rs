//! `com.listo.mqtt-client` — native process block.
//!
//! Three kinds:
//!
//!   * `com.listo.mqtt-client.client`  — holds broker settings + session
//!   * `com.listo.mqtt-client.pub`     — publishes inbound msgs to a topic
//!   * `com.listo.mqtt-client.sub`     — subscribes to a topic (output wiring
//!                                       pending streaming-emit RPC)
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

use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use serde::Deserialize;

use blocks_sdk::{
    ctx::NodeCtx,
    error::NodeError,
    node::{InputPort, NodeBehavior},
    process::{run_process_plugin, BlockIdentity},
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
        tokio::spawn(async move {
            if let Err(e) = registry().connect(path.clone(), cfg).await {
                tracing::warn!(node = %path.as_str(), error = %e, "mqtt connect failed");
            }
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
        tokio::spawn(async move {
            registry().disconnect(&path).await;
            if let Err(e) = registry().connect(path.clone(), cfg).await {
                tracing::warn!(node = %path.as_str(), error = %e, "mqtt reconnect failed");
            }
        });
        Ok(())
    }

    fn on_shutdown(&self, ctx: &NodeCtx) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
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

        // Decode the typed PublishConfig out of the node's settings
        // blob.
        let cfg: PublishConfig = serde_json::from_value(ctx.config().clone())
            .map_err(|e| NodeError::InvalidConfig(e.to_string()))?;
        if cfg.topic.is_empty() {
            return Err(NodeError::runtime("pub node has no topic configured"));
        }

        // Pub/Sub share the parent client's MQTT session. The parent
        // path is the immediate ancestor in the graph tree.
        let Some(parent_path) = ctx.node_path().parent() else {
            return Err(NodeError::runtime(
                "pub node must live under a client — has no parent",
            ));
        };

        // Fire-and-forget publish. Returning an error from on_message
        // surfaces as a behaviour error on the engine side; we keep
        // this path happy and log async failures via tracing.
        let qos = parse_qos(cfg.qos);
        let payload = payload_bytes(&msg);
        tokio::spawn(async move {
            let Some(client) = registry().handle(&parent_path).await else {
                tracing::warn!(
                    parent = %parent_path.as_str(),
                    "pub: parent client not connected — message dropped",
                );
                return;
            };
            if let Err(e) = client
                .publish(&cfg.topic, qos, cfg.retain, payload)
                .await
            {
                tracing::warn!(topic = %cfg.topic, error = %e, "mqtt publish failed");
            }
        });
        Ok(())
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
        let topic = cfg.topic.clone();
        let qos = parse_qos(cfg.qos);
        tokio::spawn(async move {
            let Some(client) = registry().handle(&parent_path).await else {
                tracing::warn!(
                    parent = %parent_path.as_str(),
                    "sub: parent client not connected — subscribe skipped",
                );
                return;
            };
            if let Err(e) = client.subscribe(&topic, qos).await {
                tracing::warn!(%topic, error = %e, "mqtt subscribe failed");
            }
        });
        Ok(())
    }

    fn on_message(&self, _ctx: &NodeCtx, _port: InputPort, _msg: Msg) -> Result<(), NodeError> {
        // `sub` has no inputs. Inbound MQTT messages need to push up
        // into the engine via a streaming-emit RPC that isn't wired
        // yet (see blocks-sdk/src/process.rs). For now, this kind
        // registers the subscription and logs received messages.
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
}

struct Session {
    client: AsyncClient,
    /// The event-loop task. Dropped on disconnect, stopping the loop.
    _task: tokio::task::JoinHandle<()>,
}

impl Registry {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Open a connection for a client node. If a session already exists
    /// under this path it is torn down first (on_config_change case).
    async fn connect(&self, path: NodePath, cfg: ClientConfig) -> Result<(), MqttError> {
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

        let path_log = path.clone();
        let task = tokio::spawn(async move {
            drive_event_loop(path_log, &mut event_loop).await;
        });

        let mut sessions = self.sessions.lock().await;
        // Replace previous session, dropping its task (and thus its
        // event loop).
        sessions.insert(
            path,
            Session {
                client,
                _task: task,
            },
        );
        Ok(())
    }

    async fn disconnect(&self, path: &NodePath) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(path);
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

async fn drive_event_loop(path: NodePath, el: &mut EventLoop) {
    loop {
        match el.poll().await {
            Ok(event) => {
                tracing::debug!(node = %path.as_str(), ?event, "mqtt event");
            }
            Err(e) => {
                tracing::warn!(node = %path.as_str(), error = %e, "mqtt connection error");
                // Back off a moment before the event loop reconnects
                // — rumqttc handles reconnection internally.
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
enum MqttError {
    // Reserved for future connect-side errors that we want to surface.
    // Connect itself is fire-and-forget via rumqttc today.
    #[allow(dead_code)]
    #[error("{0}")]
    Other(String),
}

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
