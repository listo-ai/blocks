# com.acme.wasm-demo

Reference Wasm block. Two nodes:

| Kind | Input | Output |
|---|---|---|
| `com.acme.wasm-demo.double` | `{"in": <number>}` | `{"out": 2 × number}` |
| `com.acme.wasm-demo.add`    | `{"in": {"a": <num>, "b": <num>}}` | `{"out": a + b}` |

## Build

**One-time toolchain:**

```bash
rustup target add wasm32-unknown-unknown
```

**Every build:**

```bash
cd blocks/com.acme.wasm-demo
cargo build --target wasm32-unknown-unknown --release
mkdir -p dist
cp target/wasm32-unknown-unknown/release/acme_wasm_demo.wasm dist/block.wasm
```

Or run `make wasm-demo` from the repo root once the Makefile target lands.

The host's [`WasmSupervisor::load`](../../crates/blocks-host/src/wasm.rs)
expects `dist/block.wasm` — matches the path declared in
[block.yaml](block.yaml).

## Why not in the main workspace?

It targets `wasm32-unknown-unknown`; making it a workspace member would
force the whole workspace onto that target or need per-member target
gating. Keeping the crate standalone with a path dep into
`../../crates/blocks-sdk` is the simpler option — you just
`cd` in and build.

## ABI cheat sheet

The block exports three symbols (via `export_plugin!`):

- `alloc(size: i32) -> i32` — host allocates scratch in guest memory
- `describe() -> i64` — returns packed `(ptr << 32) | len` of a JSON
  `DescribeResponse`
- `on_input(env_ptr: i32, env_len: i32) -> i64` — reads an envelope
  JSON, returns JSON of `Result<Vec<OutputMsg>, String>`

Authors never touch this — the macro wraps your `impl WasmPlugin`.
