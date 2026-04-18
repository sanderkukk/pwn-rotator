# pwn-rotator

Rust web API for an antenna rotator using the **GS-232B** protocol over a serial port or a remote RFC 2217 serial server.

## Endpoints

| Method | Path      | Description                               |
|--------|-----------|-------------------------------------------|
| GET    | `/status` | Current azimuth and elevation             |
| POST   | `/rotate` | Rotate to azimuth (and optional elevation)|
| POST   | `/stop`   | Stop all movement                         |

### `POST /rotate` body

```json
{ "azimuth": 180 }
{ "azimuth": 180, "elevation": 45 }
```

Azimuth range: 0–359°. Elevation range: 0–180°.

## Configuration

All settings are read from environment variables at startup:

| Variable         | Default         | Description                                      |
|------------------|-----------------|--------------------------------------------------|
| `ROTATOR_DEVICE` | `/dev/ttyUSB0`  | Serial device path **or** `rfc2217://host:port`  |
| `ROTATOR_BAUD`   | `9600`          | Baud rate (local serial only)                    |
| `LISTEN_ADDR`    | `0.0.0.0:3000`  | HTTP listen address and port                     |

### Local serial port

```sh
ROTATOR_DEVICE=/dev/ttyUSB0 ./target/release/pwn-rotator
```

### Remote RFC 2217 serial server

Set `ROTATOR_DEVICE` to an `rfc2217://` URL pointing at any RFC 2217–compatible
server (e.g. MikroTik IP/Serial, ser2net, …):

```sh
ROTATOR_DEVICE=rfc2217://192.168.1.1:2217 ./target/release/pwn-rotator
```

The driver handles the Telnet/COM-PORT-OPTION negotiation automatically and
**reconnects with exponential back-off** (1 s → 30 s) whenever the TCP
connection drops.

## Running

```sh
cargo build --release
ROTATOR_DEVICE=/dev/ttyUSB0 ./target/release/pwn-rotator
```
