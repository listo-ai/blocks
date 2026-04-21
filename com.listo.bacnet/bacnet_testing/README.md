# bacnet_testing

Standalone BACnet/IP test tool for one real device on your network.

This is separate from the block runtime crate so you can experiment here without
being blocked by the current `com.listo.bacnet/process` compile issues.

## Setup

```bash
cd /home/user/code/workspace/blocks/com.listo.bacnet/bacnet_testing
cp config/bacnet-device.example.json config/bacnet-device.json
```

Edit `config/bacnet-device.json`:

- `network.local_bind`: local socket bind address
- `network.broadcast_addr`: subnet broadcast BACnet/IP address
- `network.interface`: optional note for the NIC you expect to use
- `devices`: one or more named BACnet devices
- `devices[].address`: target BACnet device IP
- `devices[].device_id`: target BACnet device instance
- `devices[].reads`: object/property list to query

## Commands

```bash
cargo run -- whois
cargo run -- read-props device-a
cargo run -- read-props device-b
```

Optional custom config path:

```bash
BACNET_TEST_CONFIG=/path/to/device.json cargo run -- whois
BACNET_TEST_CONFIG=/path/to/device.json cargo run -- read-props device-a
```

## Notes

- `whois` sends both broadcast and direct unicast to the configured target.
- `whois` sends broadcast plus direct unicast to every configured device.
- `read-props` uses `ReadPropertyMultiple` with the object/property list for the
  selected device.
- The `interface` field is stored in config for convenience, but the current
  implementation selects the network path via `local_bind` and
  `broadcast_addr`.
