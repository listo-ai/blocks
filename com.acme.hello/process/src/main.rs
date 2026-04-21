//! `com.acme.hello` — native process-block binary.
//!
//! Mirrors the Wasm sibling (`src/lib.rs`) but runs as a **native
//! Linux binary** supervised by the engine over a Unix-domain socket.
//!
//! The SDK's `process` feature is mutually exclusive with `wasm`, so
//! this lives in its own crate (`process/`) next to the Wasm root.
//!
//! # What it does
//!
//! Implements the `com.acme.hello.greeter` kind:
//!
//! ```text
//! in  → "World"
//! out → "Hello, World!"
//! ```
//!
//! Accepts either a bare JSON string (`"World"`) or an object
//! (`{"name": "World"}`) for the same behaviour as the Wasm twin.
//!
//! # Build
//!
//! ```bash
//! cargo build --release
//! mkdir -p dist
//! cp target/release/acme-hello-process ../dist/process
//! ```
//!
//! # Runtime
//!
//! The supervisor launches this binary and injects the UDS path via
//! `US_PLUGIN_SOCKET`. The block responds to `Describe` and `Health`
//! RPCs immediately; `Invoke` is wired in Stage 3c when
//! `NodeBehavior` gets its process adapter.

use serde::Deserialize;

use blocks_sdk::{
    ctx::NodeCtx,
    error::NodeError,
    node::{InputPort, NodeBehavior},
    process::{run_process_plugin, BlockIdentity},
    Msg, NodeKind,
};

// ---------------------------------------------------------------------------
// Kind declaration
// ---------------------------------------------------------------------------

/// Declarative half — kind id + manifest read from YAML at compile time.
///
/// `behavior = "custom"` tells the derive that this struct also implements
/// [`NodeBehavior`] below, so the registry can register both halves.
#[derive(NodeKind)]
#[node(
    kind = "com.acme.hello.greeter",
    // Single source of truth — the block scanner reads this same file at
    // runtime via `block.yaml:contributes.kinds`.
    manifest = "../kinds/greeter.yaml",
    behavior = "custom"
)]
pub struct Greeter;

// ---------------------------------------------------------------------------
// Config — no user-configurable settings for this demo kind
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub struct GreeterConfig {}

// ---------------------------------------------------------------------------
// Behaviour — imperative half
// ---------------------------------------------------------------------------

impl NodeBehavior for Greeter {
    type Config = GreeterConfig;

    fn on_message(&self, ctx: &NodeCtx, port: InputPort, msg: Msg) -> Result<(), NodeError> {
        if port != "in" {
            return Err(NodeError::runtime(format!("unexpected port `{port}`")));
        }
        let name = extract_name(&msg.payload)?;
        let greeting = format!("Hello, {name}!");
        ctx.emit("out", Msg::new(serde_json::Value::String(greeting)))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Accept either `"World"` (bare string) or `{"name":"World"}` (object).
fn extract_name(payload: &serde_json::Value) -> Result<String, NodeError> {
    if let Some(s) = payload.as_str() {
        return Ok(s.to_owned());
    }

    #[derive(Deserialize)]
    struct Named {
        name: String,
    }
    serde_json::from_value::<Named>(payload.clone())
        .map(|n| n.name)
        .map_err(|e| {
            NodeError::runtime(format!(
                "expected a string or {{\"name\": \"...\"}} — got: {e}"
            ))
        })
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut identity = BlockIdentity::new("com.acme.hello", "0.1.0");
    identity.register_kind(Greeter);
    run_process_plugin(identity).await?;
    Ok(())
}
