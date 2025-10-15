# Alle

A Rust library and deployable binary that bridges WebSockets and PostgreSQL NOTIFY/LISTEN with dynamic subscription management.

## Features

- **Dynamic Channel Subscriptions**: WebSocket clients can subscribe/unsubscribe to channels on-demand
- **Intelligent Connection Management**: Automatically LISTEN/UNLISTEN on Postgres as clients subscribe
- **Per-Client Message Routing**: Each client only receives messages from their subscribed channels
- **Bidirectional Communication**:
  - PostgreSQL notifications → WebSocket clients
  - WebSocket messages → PostgreSQL NOTIFY
- **Library and Binary**: Use as a library in your own projects or deploy the standalone binary
- **Async/Await**: Built on Tokio for high-performance async I/O
- **Multiple Clients**: Supports multiple WebSocket clients simultaneously with independent subscriptions

## Architecture

```
┌──────────────┐         ┌─────────────────┐         ┌──────────────────┐
│  PostgreSQL  │ NOTIFY  │  Bridge         │ WS      │  WebSocket       │
│  Database    │◄────────┤  + Subscription ├────────►│  Clients         │
│              │ LISTEN  │    Manager      │         │  (Per-channel)   │
└──────────────┘         └─────────────────┘         └──────────────────┘
```

### Dynamic Subscription System

The bridge uses intelligent subscription management:

1. **Client Connects**: Each WebSocket client gets a unique ID
2. **Client Subscribes**: Client sends `{"type": "subscribe", "channel": "orders"}`
3. **Automatic LISTEN**: If this is the first subscriber to "orders", the bridge executes `LISTEN orders` on Postgres
4. **Message Routing**: Only clients subscribed to "orders" receive notifications on that channel
5. **Client Unsubscribes**: Client sends `{"type": "unsubscribe", "channel": "orders"}`
6. **Automatic UNLISTEN**: If this was the last subscriber to "orders", the bridge executes `UNLISTEN orders` on Postgres

**Benefits:**
- **Performance**: Only maintain Postgres subscriptions for channels with active clients
- **Scalability**: Support thousands of channels without overhead
- **Flexibility**: Each client can have completely independent subscriptions

## Usage as a Binary

### Command-Line Interface

Alle provides three commands:

```
Usage: alle [OPTIONS] [COMMAND]

Commands:
  serve    Start the WebSocket bridge server (default)
  listen   Listen to PostgreSQL NOTIFY on specified channels
  publish  Publish a notification to a PostgreSQL channel
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

# With options
cargo run -- serve -w "0.0.0.0:8080" -c "orders,alerts"

# Using environment variables
POSTGRES_URL="postgresql://user:pass@localhost/mydb" \
WS_BIND_ADDR="0.0.0.0:8080" \
LISTEN_CHANNELS="orders,alerts" \
cargo run

# Run release binary
./target/release/alle serve -w "0.0.0.0:8080"
```

### 2. Listen Mode

Listen to channels via WebSocket connection with an interactive REPL:

```bash
# Listen to a single channel
cargo run -- listen orders

# Listen to multiple channels
cargo run -- listen orders,alerts,notifications
# or
cargo run -- listen orders alerts notifications

# With custom WebSocket URL
cargo run -- listen -w "ws://localhost:9000" orders

# Short form
./target/release/alle listen orders,alerts
```

Example output:
```
Connecting to WebSocket: ws://127.0.0.1:8080
Listening on channels: orders, alerts

Commands:
  sub <channel>    - Subscribe to a channel
  unsub <channel>  - Unsubscribe from a channel
  list             - List subscribed channels
  help             - Show this help
  quit / exit      - Exit the program

Press Ctrl+C to stop

✓ Subscribing to channel 'orders'
✓ Subscribing to channel 'alerts'

Waiting for notifications...

[orders] {"id": 123, "status": "pending"}
[alerts] System maintenance in 5 minutes
```

**Interactive commands:**
```
> sub notifications
✓ Subscribed to channel 'notifications'

> list
Subscribed channels:
  - alerts
  - notifications
  - orders

> unsub alerts
✓ Unsubscribed from channel 'alerts'

> help
Commands:
  sub <channel>    - Subscribe to a channel
  unsub <channel>  - Unsubscribe from a channel
  list             - List subscribed channels
  help             - Show this help
  quit / exit      - Exit the program

> quit
Exiting...
```

### 3. Publish Mode

Send a notification via WebSocket connection:

```bash
# Publish a message
cargo run -- publish orders "New order received"

# Publish JSON
cargo run -- publish events '{"type":"user_login","user_id":42}'

# With custom WebSocket URL
cargo run -- publish -w "ws://localhost:9000" alerts "Test message"

# Using release binary
./target/release/alle publish orders "Order #123"
```

Example output:
```
Connecting to WebSocket: ws://127.0.0.1:8080
Publishing to channel 'orders': New order received
✓ Notification sent successfully
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

### Advanced Usage

You can use the individual components directly:

```rust
use alle::{PostgresListener, WebSocketServer};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to PostgreSQL
    let (pg_listener, notification_rx) =
        PostgresListener::connect("postgresql://localhost/postgres", 1000).await?;

    // Listen to channels
    pg_listener.listen("my_channel").await?;

    // Send a notification
    pg_listener.notify("my_channel", "Hello from Rust!").await?;

    // Create WebSocket server
    let (ws_server, incoming_rx) = WebSocketServer::new("127.0.0.1:8080".to_string());

    // Start the server
    ws_server.start(notification_rx).await?;

    Ok(())
}
```

## Testing

### Quick Start with Docker Compose

The easiest way to test Alle is using the included Docker Compose setup:

1. Start PostgreSQL:
```bash
docker-compose up -d
```

2. Run the bridge:
```bash
cargo run
# Or with custom settings:
cargo run -- -p "postgresql://postgres:postgres@localhost/postgres"
```

3. Test with psql:
```bash
# Connect to the database
docker-compose exec postgres psql -U postgres

# Send a test notification
NOTIFY orders, 'Test order notification';

# Insert a new order (triggers automatic notification)
INSERT INTO orders (customer_name, total, status) VALUES ('Test User', 99.99, 'pending');

# View notification log
SELECT * FROM notification_log;
```

4. Stop PostgreSQL when done:
```bash
docker-compose down
# Or to remove volumes:
docker-compose down -v
```

### Testing with CLI Commands

The `listen` and `publish` commands make testing easy by connecting through WebSockets:

```bash
# Terminal 1: Start the bridge server
cargo run -- serve

# Terminal 2: Listen to channels
cargo run -- listen orders,alerts

# Terminal 3: Send notifications
cargo run -- publish orders "Test order notification"
cargo run -- publish alerts "System alert"

# Terminal 2 will show:
# [orders] Test order notification
# [alerts] System alert
```

This tests the complete flow: `publish` → WebSocket → Bridge → Postgres → Bridge → WebSocket → `listen`


## License

MIT
