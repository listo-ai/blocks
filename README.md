# listo-ai/blocks

Reference and example blocks for the listo agent platform.

## Structure

Each block is a self-contained directory:

```
com.acme.hello/           # Minimal UI-only block example
com.acme.wasm-demo/       # Wasm-only block example
```

Each block contains:
- `block.yaml` — block manifest
- `Cargo.toml` — Rust crate (standalone, not a workspace member)
- `src/` — Rust handlers (compiled to wasm)
- `ui/` — Optional MF bundle (`@listo/block-ui-sdk` consumers)

## Dependencies

Blocks depend on published crates, not path deps into the monorepo:
- `listo-blocks-sdk` on crates.io
- `@listo/block-ui-sdk` on npm

See [listo-ai/agent-sdk](https://github.com/listo-ai/agent-sdk) for the SDK.
