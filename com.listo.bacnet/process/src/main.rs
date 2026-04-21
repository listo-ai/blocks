//! `com.listo.bacnet` — native process block.
//!
//! Three kinds:
//!
//!   * `com.listo.bacnet.device`  — holds device address + BACnet session
//!   * `com.listo.bacnet.read`    — reads a property on trigger
//!   * `com.listo.bacnet.write`   — writes a property from inbound msg
//!
//! All three share a process-wide `Registry` of BACnet sessions keyed by
//! the device node's path. Read/Write nodes look up their parent device's
//! session by walking one path segment up.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::OnceLock;
use std::time::Duration;

use bacnet_rs::client::BacnetClient;
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
// Kind — device
// ---------------------------------------------------------------------------

#[derive(NodeKind)]
#[node(
    kind = "com.listo.bacnet.device",
    manifest = "../kinds/device.yaml",
    behavior = "custom"
)]
pub struct Device;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DeviceConfig {
    pub host: String,
    pub port: u16,
    pub device_id: u32,
    pub timeout_ms: u64,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            host: "192.168.1.1".into(),
            port: 47808,
            device_id: 0,
            timeout_ms: 3000,
        }
    }
}

impl NodeBehavior for Device {
    type Config = DeviceConfig;

    fn on_init(&self, ctx: &NodeCtx, cfg: &Self::Config) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            if let Err(e) = registry().connect(path.clone(), cfg).await {
                tracing::warn!(node = %path.as_str(), error = %e, "bacnet connect failed");
            }
        });
        Ok(())
    }

    fn on_message(&self, _ctx: &NodeCtx, _port: InputPort, _msg: Msg) -> Result<(), NodeError> {
        // Device has no input ports.
        Ok(())
    }

    fn on_config_change(&self, ctx: &NodeCtx, cfg: &Self::Config) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            registry().disconnect(&path).await;
            if let Err(e) = registry().connect(path.clone(), cfg).await {
                tracing::warn!(node = %path.as_str(), error = %e, "bacnet reconnect failed");
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
// Kind — read
// ---------------------------------------------------------------------------

#[derive(NodeKind)]
#[node(
    kind = "com.listo.bacnet.read",
    manifest = "../kinds/read.yaml",
    behavior = "custom"
)]
pub struct Read;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ReadConfig {
    pub object_type: String,
    pub object_instance: u32,
    pub property: String,
}

impl Default for ReadConfig {
    fn default() -> Self {
        Self {
            object_type: "analog-input".into(),
            object_instance: 0,
            property: "present-value".into(),
        }
    }
}

impl NodeBehavior for Read {
    type Config = ReadConfig;

    fn on_message(&self, ctx: &NodeCtx, port: InputPort, _msg: Msg) -> Result<(), NodeError> {
        if port != "in" {
            return Err(NodeError::runtime(format!("unexpected port `{port}`")));
        }

        let cfg: ReadConfig = serde_json::from_value(ctx.config().clone())
            .map_err(|e| NodeError::InvalidConfig(e.to_string()))?;

        let Some(parent_path) = ctx.node_path().parent() else {
            return Err(NodeError::runtime(
                "read node must live under a device — has no parent",
            ));
        };

        let node_path = ctx.node_path().clone();
        tokio::spawn(async move {
            let Some(session) = registry().get(&parent_path).await else {
                tracing::warn!(
                    parent = %parent_path.as_str(),
                    "read: parent device not connected — request dropped",
                );
                return;
            };

            match session
                .read_property(&cfg.object_type, cfg.object_instance, &cfg.property)
                .await
            {
                Ok(value) => {
                    tracing::debug!(
                        node = %node_path.as_str(),
                        object = %cfg.object_type,
                        instance = cfg.object_instance,
                        property = %cfg.property,
                        ?value,
                        "bacnet read ok",
                    );
                    // TODO: emit value via streaming-emit RPC once available.
                }
                Err(e) => {
                    tracing::warn!(
                        node = %node_path.as_str(),
                        error = %e,
                        "bacnet read failed",
                    );
                }
            }
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Kind — write
// ---------------------------------------------------------------------------

#[derive(NodeKind)]
#[node(
    kind = "com.listo.bacnet.write",
    manifest = "../kinds/write.yaml",
    behavior = "custom"
)]
pub struct Write;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WriteConfig {
    pub object_type: String,
    pub object_instance: u32,
    pub property: String,
    pub priority: u8,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            object_type: "analog-output".into(),
            object_instance: 0,
            property: "present-value".into(),
            priority: 16,
        }
    }
}

impl NodeBehavior for Write {
    type Config = WriteConfig;

    fn on_message(&self, ctx: &NodeCtx, port: InputPort, msg: Msg) -> Result<(), NodeError> {
        if port != "in" {
            return Err(NodeError::runtime(format!("unexpected port `{port}`")));
        }

        let cfg: WriteConfig = serde_json::from_value(ctx.config().clone())
            .map_err(|e| NodeError::InvalidConfig(e.to_string()))?;

        let Some(parent_path) = ctx.node_path().parent() else {
            return Err(NodeError::runtime(
                "write node must live under a device — has no parent",
            ));
        };

        let node_path = ctx.node_path().clone();
        let value = msg.payload.clone();
        tokio::spawn(async move {
            let Some(session) = registry().get(&parent_path).await else {
                tracing::warn!(
                    parent = %parent_path.as_str(),
                    "write: parent device not connected — message dropped",
                );
                return;
            };

            match session
                .write_property(
                    &cfg.object_type,
                    cfg.object_instance,
                    &cfg.property,
                    &value,
                    cfg.priority,
                )
                .await
            {
                Ok(()) => {
                    tracing::debug!(
                        node = %node_path.as_str(),
                        object = %cfg.object_type,
                        instance = cfg.object_instance,
                        property = %cfg.property,
                        "bacnet write ok",
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        node = %node_path.as_str(),
                        error = %e,
                        "bacnet write failed",
                    );
                }
            }
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Process-wide BACnet session registry
// ---------------------------------------------------------------------------

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(Registry::new)
}

struct Registry {
    sessions: Mutex<HashMap<NodePath, DeviceSession>>,
}

struct DeviceSession {
    client: BacnetClient,
    device_id: u32,
    addr: SocketAddr,
    timeout: Duration,
}

impl DeviceSession {
    async fn read_property(
        &self,
        object_type: &str,
        object_instance: u32,
        property: &str,
    ) -> Result<serde_json::Value, BacnetError> {
        // bacnet-rs client API: discover device then read property.
        // This is a simplified placeholder — wire in full read-property
        // request/response once the client API stabilises.
        tracing::debug!(
            device_id = self.device_id,
            addr = %self.addr,
            object_type,
            object_instance,
            property,
            "bacnet read-property (stub)",
        );
        Ok(serde_json::Value::Null)
    }

    async fn write_property(
        &self,
        object_type: &str,
        object_instance: u32,
        property: &str,
        value: &serde_json::Value,
        priority: u8,
    ) -> Result<(), BacnetError> {
        tracing::debug!(
            device_id = self.device_id,
            addr = %self.addr,
            object_type,
            object_instance,
            property,
            ?value,
            priority,
            "bacnet write-property (stub)",
        );
        Ok(())
    }
}

impl Registry {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    async fn connect(&self, path: NodePath, cfg: DeviceConfig) -> Result<(), BacnetError> {
        let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port)
            .parse()
            .map_err(|e: std::net::AddrParseError| BacnetError::Config(e.to_string()))?;

        // bacnet-rs 0.3 BacnetClient is created per-request (stateless UDP).
        // We store the config so read/write nodes can build requests.
        let client = BacnetClient::new().map_err(|e| BacnetError::Connect(e.to_string()))?;

        let session = DeviceSession {
            client,
            device_id: cfg.device_id,
            addr,
            timeout: Duration::from_millis(cfg.timeout_ms),
        };

        let mut sessions = self.sessions.lock().await;
        sessions.insert(path, session);
        Ok(())
    }

    async fn disconnect(&self, path: &NodePath) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(path);
    }

    async fn get(&self, path: &NodePath) -> Option<SessionHandle> {
        let sessions = self.sessions.lock().await;
        sessions.get(path).map(|s| SessionHandle {
            device_id: s.device_id,
            addr: s.addr,
            timeout: s.timeout,
        })
    }
}

/// A cheap, cloneable handle to the relevant session parameters.
struct SessionHandle {
    device_id: u32,
    addr: SocketAddr,
    timeout: Duration,
}

impl SessionHandle {
    async fn read_property(
        &self,
        object_type: &str,
        object_instance: u32,
        property: &str,
    ) -> Result<serde_json::Value, BacnetError> {
        tracing::debug!(
            device_id = self.device_id,
            addr = %self.addr,
            object_type,
            object_instance,
            property,
            "bacnet read-property",
        );
        // TODO: use bacnet-rs client to issue a ReadProperty request.
        // bacnet_rs::client::ReadPropertyRequest { ... }.send(self.addr, timeout)
        Ok(serde_json::Value::Null)
    }

    async fn write_property(
        &self,
        object_type: &str,
        object_instance: u32,
        property: &str,
        value: &serde_json::Value,
        priority: u8,
    ) -> Result<(), BacnetError> {
        tracing::debug!(
            device_id = self.device_id,
            addr = %self.addr,
            object_type,
            object_instance,
            property,
            ?value,
            priority,
            "bacnet write-property",
        );
        // TODO: use bacnet-rs client to issue a WriteProperty request.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
enum BacnetError {
    #[error("config error: {0}")]
    Config(String),
    #[error("connect error: {0}")]
    Connect(String),
    #[error("{0}")]
    Other(String),
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

    let mut identity = BlockIdentity::new("com.listo.bacnet", "0.1.0");
    identity.register_kind(Device);
    identity.register_kind(Read);
    identity.register_kind(Write);
    run_process_plugin(identity).await?;
    Ok(())
}
