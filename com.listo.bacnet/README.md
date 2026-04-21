# com.listo.bacnet

BACnet/IP connectivity block for the Listo agent.

## Kinds

| Kind | Description |
|------|-------------|
| `com.listo.bacnet.device` | Holds broker address + BACnet device config. Acts as a container for read/write nodes. |
| `com.listo.bacnet.read` | Reads a single BACnet property on trigger. |
| `com.listo.bacnet.write` | Writes a value to a BACnet property from an inbound message. |

## Quick start

```
make edge   # build + stage into edge-blocks + hot-reload
```

## Development

```
make ui       # build UI bundle only
make process  # build native process binary only
make reload   # hot-reload without rebuilding
make clean    # remove all build artefacts
```

## Architecture

- **process/src/main.rs** — Tokio-based process block. Uses [`bacnet-rs`](https://github.com/bacnet-rs/bacnet-rs) (0.3) for BACnet/IP.
- **ui-src/src/Panel.tsx** — React sidebar panel (Module Federation remote).
- **kinds/** — YAML manifests for each node kind.

## Notes

The `bacnet-rs` client API is at 0.3 and under active development. The
`read_property` / `write_property` implementations in `main.rs` are currently
stubs that log the request. Wire in the full `bacnet-rs` request/response
cycle once the upstream API stabilises.
