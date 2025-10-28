# Alle

A Rust library and deployable binary that bridges WebSockets or Server Side events with and PostgreSQL NOTIFY/LISTEN with dynamic subscription management.

This allows you to have an unlimited amount of remote listeners in the postgreSQL NOTIFY/LISTEN interface.

## Features

- **Dynamic Channel Subscriptions**: Clients can subscribe/unsubscribe to channels on-demand
- **Intelligent Connection Management**: Automatically LISTEN/UNLISTEN on Postgres as clients subscribe on a single connection to postgres to minimize load on postgres
- **Library and Binary**: Use as a library in your own projects or deploy the standalone binary
- **Async/Await**: Built on Tokio for high-performance async I/O

## Architecture

```
┌──────────────┐         ┌─────────────────┐           ┌──────────────────┐
│  PostgreSQL  │ NOTIFY  │                 │ WebSocket │                  |
│  Database    │◄────────┤  Bridge         ├──--──────►│  Clients         │
│              │ LISTEN  │    Manager      │           │  (Per-channel)   │
└──────────────┘         └─────────────────┘           └──────────────────┘
```


## Usage as a Binary

### Command-Line Interface

Alle provides three commands:

```
Usage: alle [OPTIONS] [COMMAND]

Commands:
  serve    Start the WebSocket bridge server (default)
  help     Print this message or the help of the given subcommand(s)

Global Options:
  -p, --postgres-url <POSTGRES_URL>  PostgreSQL connection string [default: postgresql://localhost/postgres]
  -l, --log-level <LOG_LEVEL>        Log level filter [default: alle=info,info]
  -h, --help                         Print help
  -V, --version                      Print version
```

### 1. Server Mode (default)

Start the WebSocket bridge server:

```bash
# Start server with defaults
cargo run
# or
cargo run -- serve

# Using environment variables
POSTGRES_URL="postgresql://user:pass@localhost/mydb" \
WS_BIND_ADDR="0.0.0.0:8080" \
LISTEN_CHANNELS="orders,alerts" \
cargo run

# Run release binary
./target/release/alle serve -w "0.0.0.0:8080"
```

### WebSocket Protocol

All messages use JSON format.

**Client → Server Messages:**

Subscribe to a channel:
```json
{
  "type": "subscribe",
  "channel": "orders"
}
```

Unsubscribe from a channel:
```json
{
  "type": "unsubscribe",
  "channel": "orders"
}
```

Send a notification to Postgres:
```json
{
  "type": "notify",
  "channel": "orders",
  "payload": "New order #123"
}
```

**Server → Client Messages:**

Notification from Postgres:
```json
{
  "type": "notification",
  "channel": "orders",
  "payload": "New order #123"
}
```

Subscription confirmed:
```json
{
  "type": "subscribed",
  "channel": "orders"
}
```

Unsubscription confirmed:
```json
{
  "type": "unsubscribed",
  "channel": "orders"
}
```

Error message:
```json
{
  "type": "error",
  "message": "Error description"
}
```

## Usage as a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
alle = { path = "../alle" }
```

### Example

```rust
use alle::{Bridge, BridgeConfig};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let config = BridgeConfig::new(
        "postgresql://localhost/postgres".to_string(),
        "127.0.0.1:8080".to_string(),
    )
    .with_channels(vec!["channel1".to_string(), "channel2".to_string()]);

    let bridge = Bridge::new(config);
    bridge.run().await?;

    Ok(())
}
```

## Helper client



## License

MIT
