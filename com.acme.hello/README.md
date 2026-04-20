# com.acme.hello — reference block

The canonical end-to-end example of a block in this repo. One
directory, two contributions, one `block.yaml`:

| Contribution | What | Source |
|---|---|---|
| **UI** (sidebar panel) | Module-Federation remote, loaded into Studio | [`ui/`](ui/) + [`ui/remoteEntry.js`](ui/remoteEntry.js) |
| **Wasm kind** (`com.acme.hello.greeter`) | In-process node — takes a name, emits a greeting | [`src/lib.rs`](src/lib.rs) → `dist/block.wasm` |

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
cd blocks/com.acme.hello
cargo build --target wasm32-unknown-unknown --release
mkdir -p dist
cp target/wasm32-unknown-unknown/release/acme_hello.wasm dist/block.wasm
```

The host reads `dist/block.wasm` as declared in
[`block.yaml`](block.yaml).

## Why the crate is standalone

It targets `wasm32-unknown-unknown` — making it a workspace member
would force the whole workspace onto that target. A path dep into
`../../crates/blocks-sdk` gives it everything it needs.

## Related reading

- [PLUGINS.md](../../docs/design/PLUGINS.md) — block layout + lifecycle
- [`com.acme.wasm-demo`](../com.acme.wasm-demo/) — a pure-Wasm block
  (no UI) with two numeric nodes; smaller diff to read
- [`crates/blocks-sdk/src/wasm.rs`](../../crates/blocks-sdk/src/wasm.rs)
  — the author-facing SDK this block uses
- [`crates/blocks-host/src/wasm.rs`](../../crates/blocks-host/src/wasm.rs)
  — the host-side supervisor that loads and runs `block.wasm`
