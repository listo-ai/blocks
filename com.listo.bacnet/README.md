# com.listo.bacnet

BACnet/IP connectivity block for the Listo agent.

## Kinds

| Kind | Description |
|------|-------------|
| `com.listo.bacnet.driver` | Manages the local BACnet/IP interface. Top-level container for devices. |
| `com.listo.bacnet.device` | A remote BACnet device (lives under a driver). Container for points. |
| `com.listo.bacnet.point` | A single BACnet object property — `direction: read` or `write` (lives under a device). |

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

- **process/src/main.rs** — Tokio-based process block (`Driver` / `Device` / `Point`). Uses [`bacnet-rs`](https://github.com/bacnet-rs/bacnet-rs) (0.3) for BACnet/IP.
- **ui-src/src/Panel.tsx** — React sidebar panel (Module Federation remote). Lists driver nodes.
- **kinds/** — YAML manifests: `driver.yaml`, `device.yaml`, `point.yaml`.

## Notes

The `bacnet-rs` client API is at 0.3 and under active development. The
`read_property` / `write_property` implementations in `main.rs` are currently
stubs that log the request. Wire in the full `bacnet-rs` request/response
cycle once the upstream API stabilises.
