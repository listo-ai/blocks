//! `com.acme.hello` — reference plugin showing the full shape.
//!
//! Two contributions in one plugin directory:
//! - **UI**: Module-Federation remote (see `ui/`) mounted in the
//!   sidebar.
//! - **Wasm**: one node kind `com.acme.hello.greeter` implemented
//!   here in Rust and compiled to `.wasm`.
//!
//! The greeter takes a name on port `in` and emits a greeting on
//! port `out`:
//!
//! ```text
//! in  → "World"
//! out → "Hello, World!"
//! ```
//!
//! Accepts either a bare JSON string (`"World"`) or an object
//! (`{"name": "World"}`) so it reads as an example regardless of how
//! upstream nodes shape their messages.

use extensions_sdk::wasm::{
    self, DescribeResponse, KindDecl, OnInputEnvelope, OutputMsg, WasmPlugin,
};
use serde::Deserialize;

const GREETER: &str = "com.acme.hello.greeter";

struct Hello;

impl WasmPlugin for Hello {
    fn describe() -> DescribeResponse {
        DescribeResponse {
            extension_id: "com.acme.hello".into(),
            version: "0.1.0".into(),
            kinds: vec![KindDecl {
                kind_id: GREETER.into(),
                display_name: Some("Greeter".into()),
                inputs: vec!["in".into()],
                outputs: vec!["out".into()],
            }],
        }
    }

    fn on_input(env: OnInputEnvelope<'_>) -> Result<Vec<OutputMsg>, String> {
        if env.kind_id != GREETER {
            return Err(format!("unknown kind_id `{}`", env.kind_id));
        }
        if env.port != "in" {
            return Err(format!("unknown input port `{}`", env.port));
        }

        let name = extract_name(env.msg_json)?;
        let greeting = format!("Hello, {name}!");
        Ok(vec![OutputMsg {
            port: "out".into(),
            msg_json: serde_json::to_string(&greeting).expect("string serialises"),
        }])
    }
}

/// Accept either `"World"` or `{"name": "World"}` — be forgiving
/// about the upstream msg shape so this stays demonstrative.
fn extract_name(msg_json: &str) -> Result<String, String> {
    // Try bare string first.
    if let Ok(s) = serde_json::from_str::<String>(msg_json) {
        return Ok(s);
    }
    // Fall back to an object with a `name` field.
    #[derive(Deserialize)]
    struct Named {
        name: String,
    }
    serde_json::from_str::<Named>(msg_json)
        .map(|n| n.name)
        .map_err(|e| format!("expected a string or {{\"name\": \"...\"}} — got: {e}"))
}

wasm::export_plugin!(Hello);
