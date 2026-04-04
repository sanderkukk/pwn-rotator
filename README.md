# pwn-rotator

Rust web API for an antenna rotator using the **GS-232A** protocol over a serial port.

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

| Variable         | Default         | Description                  |
|------------------|-----------------|------------------------------|
| `ROTATOR_DEVICE` | `/dev/ttyUSB0`  | Serial device path           |
| `ROTATOR_BAUD`   | `9600`          | Serial port baud rate        |
| `LISTEN_ADDR`    | `0.0.0.0:3000`  | HTTP listen address and port |

## Running

```sh
cargo build --release
ROTATOR_DEVICE=/dev/ttyUSB0 ./target/release/pwn-rotator
```
