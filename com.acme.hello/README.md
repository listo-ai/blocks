# com.acme.hello — reference block

The canonical end-to-end example of a block in this repo. One
directory, three contributions, one `block.yaml`:

| Contribution | What | Source |
|---|---|---|
| **UI** (sidebar panel) | Module-Federation remote, loaded into Studio | [`ui-src/`](ui-src/) → [`ui/remoteEntry.js`](ui/remoteEntry.js) |
| **Wasm kind** (`com.acme.hello.greeter`) | In-process node — takes a name, emits a greeting | [`src/lib.rs`](src/lib.rs) → `dist/block.wasm` |
| **Process kind** (`com.acme.hello.greeter`) | Same kind, native binary supervised over gRPC/UDS | [`process/`](process/) → `dist/process` |

The three contributions are independent — ship any subset. The
`block.yaml` manifest shape doesn't change.

> **Wasm vs Process** — both implement the same `com.acme.hello.greeter`
> kind. The Wasm variant runs in-process inside the engine (lower
> latency, smaller footprint). The process variant runs as a supervised
> native binary (native deps, async I/O, full Tokio ecosystem). Pick the
> one that matches your block's needs; block consumers see the same kind
> either way.


## The greeter node

```text
com.acme.hello.greeter
┌──────────┐
│ in       │  JSON string or {"name": "..."}
│    out   │  → "Hello, <name>!"
└──────────┘
```

Wire it into a flow like any other node — input a string, get a
greeting out.

## Build

### UI bundle

The UI source lives in [`ui-src/`](ui-src/). It is a standard Rsbuild +
Module Federation remote that uses `@listo/block-ui-sdk` for all
agent connectivity — never `@listo/ui-core` directly.

```bash
# from workspace root — install workspace deps (first time only)
pnpm install

# dev server with HMR (standalone harness, no Studio needed)
cd blocks/com.acme.hello/ui-src
pnpm dev

# production build → outputs into ../ui/ (what block.yaml references)
pnpm build
```

The build emits `ui/remoteEntry.js` which is what `block.yaml` already
points at:

```yaml
contributes:
  ui:
    entry: ui/remoteEntry.js
```

### Wasm kind

One-time toolchain:

```bash
rustup target add wasm32-unknown-unknown
```

Every build:

```bash
cd blocks/com.acme.hello
cargo build --target wasm32-unknown-unknown --release
mkdir -p dist
cp target/wasm32-unknown-unknown/release/acme_hello.wasm dist/block.wasm
```

The host reads `dist/block.wasm` as declared in
[`block.yaml`](block.yaml).

### Process kind (native binary)

The process block is a separate crate at [`process/`](process/) because
the `wasm` and `process` features of `blocks-sdk` are mutually
exclusive.

```bash
cd blocks/com.acme.hello/process
cargo build --release
mkdir -p ../dist
cp target/release/acme-hello-process ../dist/process
```

The supervisor reads `dist/process` from
[`block.yaml`](block.yaml) and launches it as a child process, injecting
the Unix-domain socket path via `US_PLUGIN_SOCKET`.

## Why both crates are standalone

Both crates import `blocks-sdk` from git (`listo-ai/agent-sdk`) rather
than as a workspace member arrangement:

- The **Wasm** crate targets `wasm32-unknown-unknown` — workspace
  membership would force that target onto every other crate.
- The **process** crate compiles to a native binary with the
  mutually-exclusive `process` feature of `blocks-sdk`.

A git path dep gives each what it needs without infecting the workspace.

## Stage 3c note

The process block currently responds to `Describe` and `Health` RPCs
(kind registration + readiness probes). The `on_message` / `Invoke` RPC
wiring (*NodeBehavior* → gRPC adapter) lands in Stage 3c and will light
up the `Greeter::on_message` impl in `process/src/main.rs` without any
author-side changes.

## Related reading

- [PLUGINS.md](../../docs/design/PLUGINS.md) — block layout + lifecycle
- [`com.acme.wasm-demo`](../com.acme.wasm-demo/) — a pure-Wasm block
  (no UI) with two numeric nodes; smaller diff to read
- [`agent-sdk/blocks-sdk/src/wasm.rs`](../../agent-sdk/blocks-sdk/src/wasm.rs)
  — the Wasm SDK this block uses (`WasmPlugin` trait)
- [`agent-sdk/blocks-sdk/src/process.rs`](../../agent-sdk/blocks-sdk/src/process.rs)
  — the process SDK this block uses (`run_process_plugin`, `BlockIdentity`)
- [`agent-sdk/blocks-sdk/src/node.rs`](../../agent-sdk/blocks-sdk/src/node.rs)
  — `NodeBehavior` + `NodeKind` traits authored in `process/src/main.rs`
