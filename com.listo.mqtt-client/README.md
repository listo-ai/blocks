# com.listo.mqtt-client

Minimal scaffold for an MQTT client block.

Current placeholder kinds:

- `com.listo.mqtt-client.client`
- `com.listo.mqtt-client.pub-sub`

Current shape:

- `block.yaml` declares the block manifest
- `Cargo.toml` defines a standalone Wasm crate
- `src/lib.rs` exports an empty placeholder plugin

## Build

```bash
rustup target add wasm32-unknown-unknown
cd blocks/com.listo.mqtt-client
cargo build --target wasm32-unknown-unknown --release
mkdir -p dist
cp target/wasm32-unknown-unknown/release/listo_mqtt_client.wasm dist/block.wasm
```

This is only a starter scaffold for now. The node kinds are declared,
but there is no MQTT logic behind them yet.
