//! `com.listo.bacnet` — native process block.
//!
//! Three kinds:
//!
//!   * `com.listo.bacnet.driver`  — local BACnet/IP interface config
//!   * `com.listo.bacnet.device`  — remote BACnet device (lives under driver)
//!   * `com.listo.bacnet.point`   — single object property, read or write
//!                                  (lives under device)
//!
//! The process-wide `Registry` stores driver sessions keyed by driver node
//! path. Point nodes resolve their parent device config and grandparent
//! driver session when issuing read/write requests.

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
// Kind — driver
// ---------------------------------------------------------------------------

#[derive(NodeKind)]
#[node(
    kind = "com.listo.bacnet.driver",
    manifest = "../kinds/driver.yaml",
    behavior = "custom"
)]
pub struct Driver;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DriverConfig {
    pub local_addr: String,
    pub local_port: u16,
    pub broadcast_addr: String,
    pub timeout_ms: u64,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            local_addr: "0.0.0.0".into(),
            local_port: 47808,
            broadcast_addr: "255.255.255.255".into(),
            timeout_ms: 3000,
        }
    }
}

impl NodeBehavior for Driver {
    type Config = DriverConfig;

    fn on_init(&self, ctx: &NodeCtx, cfg: &Self::Config) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            if let Err(e) = registry().start(path.clone(), cfg).await {
                tracing::warn!(node = %path.as_str(), error = %e, "bacnet driver start failed");
            }
        });
        Ok(())
    }

    fn on_message(&self, _ctx: &NodeCtx, _port: InputPort, _msg: Msg) -> Result<(), NodeError> {
        Ok(())
    }

    fn on_config_change(&self, ctx: &NodeCtx, cfg: &Self::Config) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            registry().stop(&path).await;
            if let Err(e) = registry().start(path.clone(), cfg).await {
                tracing::warn!(node = %path.as_str(), error = %e, "bacnet driver restart failed");
            }
        });
        Ok(())
    }

    fn on_shutdown(&self, ctx: &NodeCtx) -> Result<(), NodeError> {
        let path = ctx.node_path().clone();
        tokio::spawn(async move { registry().stop(&path).await });
        Ok(())
    }
}

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
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            host: "192.168.1.1".into(),
            port: 47808,
            device_id: 0,
        }
    }
}

impl NodeBehavior for Device {
    type Config = DeviceConfig;

    fn on_message(&self, _ctx: &NodeCtx, _port: InputPort, _msg: Msg) -> Result<(), NodeError> {
        // Device is a config container — no input ports.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Kind — point
// ---------------------------------------------------------------------------

#[derive(NodeKind)]
#[node(
    kind = "com.listo.bacnet.point",
    manifest = "../kinds/point.yaml",
    behavior = "custom"
)]
pub struct Point;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PointDirection {
    Read,
    Write,
}

impl Default for PointDirection {
    fn default() -> Self {
        Self::Read
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PointConfig {
    pub direction: PointDirection,
    pub object_type: String,
    pub object_instance: u32,
    pub property: String,
    pub priority: u8,
}

impl Default for PointConfig {
    fn default() -> Self {
        Self {
            direction: PointDirection::Read,
            object_type: "analog-input".into(),
            object_instance: 0,
            property: "present-value".into(),
            priority: 16,
        }
    }
}

impl NodeBehavior for Point {
    type Config = PointConfig;

    fn on_message(&self, ctx: &NodeCtx, port: InputPort, msg: Msg) -> Result<(), NodeError> {
        if port != "in" {
            return Err(NodeError::runtime(format!("unexpected port `{port}`")));
        }

        let cfg: PointConfig = serde_json::from_value(ctx.config().clone())
            .map_err(|e| NodeError::InvalidConfig(e.to_string()))?;

        // point → device → driver (two levels up)
        let Some(device_path) = ctx.node_path().parent() else {
            return Err(NodeError::runtime("point must live under a device"));
        };
        let Some(driver_path) = device_path.parent() else {
            return Err(NodeError::runtime("device must live under a driver"));
        };

        let device_cfg: DeviceConfig =
            serde_json::from_value(ctx.peer_config(&device_path).unwrap_or_default())
                .unwrap_or_default();

        let node_path = ctx.node_path().clone();
        let payload = msg.payload.clone();
        tokio::spawn(async move {
            let Some(drv) = registry().get(&driver_path).await else {
                tracing::warn!(
                    driver = %driver_path.as_str(),
                    "point: driver not started — request dropped",
                );
                return;
            };

            let addr: SocketAddr =
                match format!("{}:{}", device_cfg.host, device_cfg.port).parse() {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::warn!(node = %node_path.as_str(), error = %e, "invalid device address");
                        return;
                    }
                };

            match cfg.direction {
                PointDirection::Read => {
                    match drv
                        .read_property(
                            addr,
                            device_cfg.device_id,
                            &cfg.object_type,
                            cfg.object_instance,
                            &cfg.property,
                        )
                        .await
                    {
                        Ok(value) => {
                            tracing::debug!(node = %node_path.as_str(), ?value, "bacnet read ok");
                            // TODO: emit via streaming-emit RPC.
                        }
                        Err(e) => {
                            tracing::warn!(node = %node_path.as_str(), error = %e, "bacnet read failed");
                        }
                    }
                }
                PointDirection::Write => {
                    match drv
                        .write_property(
                            addr,
                            device_cfg.device_id,
                            &cfg.object_type,
                            cfg.object_instance,
                            &cfg.property,
                            &payload,
                            cfg.priority,
                        )
                        .await
                    {
                        Ok(()) => {
                            tracing::debug!(node = %node_path.as_str(), "bacnet write ok");
                        }
                        Err(e) => {
                            tracing::warn!(node = %node_path.as_str(), error = %e, "bacnet write failed");
                        }
                    }
                }
            }
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Process-wide driver registry
// ---------------------------------------------------------------------------

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(Registry::new)
}

struct Registry {
    drivers: Mutex<HashMap<NodePath, DriverSession>>,
}

struct DriverSession {
    cfg: DriverConfig,
    #[allow(dead_code)]
    client: BacnetClient,
}

impl Registry {
    fn new() -> Self {
        Self {
            drivers: Mutex::new(HashMap::new()),
        }
    }

    async fn start(&self, path: NodePath, cfg: DriverConfig) -> Result<(), BacnetError> {
        let client = BacnetClient::new().map_err(|e| BacnetError::Start(e.to_string()))?;
        let mut drivers = self.drivers.lock().await;
        drivers.insert(path, DriverSession { cfg, client });
        Ok(())
    }

    async fn stop(&self, path: &NodePath) {
        let mut drivers = self.drivers.lock().await;
        drivers.remove(path);
    }

    async fn get(&self, path: &NodePath) -> Option<DriverHandle> {
        let drivers = self.drivers.lock().await;
        drivers.get(path).map(|s| DriverHandle {
            timeout: Duration::from_millis(s.cfg.timeout_ms),
        })
    }
}

struct DriverHandle {
    timeout: Duration,
}

impl DriverHandle {
    async fn read_property(
        &self,
        addr: SocketAddr,
        device_id: u32,
        object_type: &str,
        object_instance: u32,
        property: &str,
    ) -> Result<serde_json::Value, BacnetError> {
        tracing::debug!(
            %addr, device_id, object_type, object_instance, property,
            "bacnet read-property (stub)",
        );
        // TODO: bacnet_rs ReadProperty request — self.timeout available.
        let _ = self.timeout;
        Ok(serde_json::Value::Null)
    }

    async fn write_property(
        &self,
        addr: SocketAddr,
        device_id: u32,
        object_type: &str,
        object_instance: u32,
        property: &str,
        value: &serde_json::Value,
        priority: u8,
    ) -> Result<(), BacnetError> {
        tracing::debug!(
            %addr, device_id, object_type, object_instance, property, ?value, priority,
            "bacnet write-property (stub)",
        );
        // TODO: bacnet_rs WriteProperty request — self.timeout available.
        let _ = self.timeout;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
enum BacnetError {
    #[error("driver start error: {0}")]
    Start(String),
    #[allow(dead_code)]
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
    identity.register_kind(Driver);
    identity.register_kind(Device);
    identity.register_kind(Point);
    run_process_plugin(identity).await?;
    Ok(())
}
