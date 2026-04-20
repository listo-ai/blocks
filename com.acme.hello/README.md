# com.acme.hello — reference plugin

The canonical end-to-end example of a plugin in this repo. One
directory, two contributions, one `plugin.yaml`:

| Contribution | What | Source |
|---|---|---|
| **UI** (sidebar panel) | Module-Federation remote, loaded into Studio | [`ui/`](ui/) + [`ui/remoteEntry.js`](ui/remoteEntry.js) |
| **Wasm kind** (`com.acme.hello.greeter`) | In-process node — takes a name, emits a greeting | [`src/lib.rs`](src/lib.rs) → `dist/plugin.wasm` |

The two contributions are independent — you can ship just the UI, or
just the Wasm, or both. The manifest shape doesn't change.

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

The existing `ui/` directory already contains a prebuilt MF remote
entry. If you're iterating on it, the build step lives in the repo's
frontend tooling; see the root build docs.

### Wasm kind

One-time toolchain:

```bash
rustup target add wasm32-unknown-unknown
```

Every build:

```bash
cd plugins/com.acme.hello
cargo build --target wasm32-unknown-unknown --release
mkdir -p dist
cp target/wasm32-unknown-unknown/release/acme_hello.wasm dist/plugin.wasm
```

The host reads `dist/plugin.wasm` as declared in
[`plugin.yaml`](plugin.yaml).

## Why the crate is standalone

It targets `wasm32-unknown-unknown` — making it a workspace member
would force the whole workspace onto that target. A path dep into
`../../crates/extensions-sdk` gives it everything it needs.

## Related reading

- [PLUGINS.md](../../docs/design/PLUGINS.md) — plugin layout + lifecycle
- [`com.acme.wasm-demo`](../com.acme.wasm-demo/) — a pure-Wasm plugin
  (no UI) with two numeric nodes; smaller diff to read
- [`crates/extensions-sdk/src/wasm.rs`](../../crates/extensions-sdk/src/wasm.rs)
  — the author-facing SDK this plugin uses
- [`crates/extensions-host/src/wasm.rs`](../../crates/extensions-host/src/wasm.rs)
  — the host-side supervisor that loads and runs `plugin.wasm`
