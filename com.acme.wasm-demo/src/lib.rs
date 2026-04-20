//! Reference Wasm plugin: two compute nodes.
//!
//! - `com.acme.wasm-demo.double` — takes a number on port `in`,
//!   emits `2 × n` on port `out`.
//! - `com.acme.wasm-demo.add`    — takes `{"a": n, "b": m}` on port
//!   `in`, emits `n + m` on port `out`.
//!
//! Kept deliberately minimal so the shape of a real plugin is easy
//! to see: one `impl WasmPlugin` block, one `export_plugin!`.
//!
//! Build:
//!
//! ```bash
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use extensions_sdk::wasm::{
    self, DescribeResponse, KindDecl, OnInputEnvelope, OutputMsg, WasmPlugin,
};

struct Demo;

const DOUBLE: &str = "com.acme.wasm-demo.double";
const ADD: &str = "com.acme.wasm-demo.add";

impl WasmPlugin for Demo {
    fn describe() -> DescribeResponse {
        DescribeResponse {
            extension_id: "com.acme.wasm-demo".into(),
            version: "0.1.0".into(),
            kinds: vec![
                KindDecl {
                    kind_id: DOUBLE.into(),
                    display_name: Some("Double".into()),
                    inputs: vec!["in".into()],
                    outputs: vec!["out".into()],
                },
                KindDecl {
                    kind_id: ADD.into(),
                    display_name: Some("Add".into()),
                    inputs: vec!["in".into()],
                    outputs: vec!["out".into()],
                },
            ],
        }
    }

    fn on_input(env: OnInputEnvelope<'_>) -> Result<Vec<OutputMsg>, String> {
        match env.kind_id {
            DOUBLE => {
                let n: f64 = serde_json::from_str(env.msg_json)
                    .map_err(|e| format!("double: expected a number, got {e}"))?;
                Ok(vec![OutputMsg {
                    port: "out".into(),
                    msg_json: serde_json::to_string(&(n * 2.0))
                        .expect("f64 serialises"),
                }])
            }
            ADD => {
                #[derive(serde::Deserialize)]
                struct Pair {
                    a: f64,
                    b: f64,
                }
                let p: Pair = serde_json::from_str(env.msg_json)
                    .map_err(|e| format!("add: expected {{a, b}}, got {e}"))?;
                Ok(vec![OutputMsg {
                    port: "out".into(),
                    msg_json: serde_json::to_string(&(p.a + p.b))
                        .expect("f64 serialises"),
                }])
            }
            other => Err(format!("unknown kind_id `{other}`")),
        }
    }
}

wasm::export_plugin!(Demo);
