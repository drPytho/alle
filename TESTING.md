# Testing Guide

This guide walks through testing Alle with the built-in CLI commands.

## Quick Test (No Database Required)

The fastest way to test the WebSocket functionality is using three terminals:

### Terminal 1: Start the Bridge Server

```bash
# Start Postgres
docker-compose up -d

# Start the bridge
cargo run -- serve -p "postgresql://postgres:postgres@localhost:15432/postgres"
```

Output:
```
 INFO  alle: Starting Alle WebSocket-Postgres bridge
 INFO  alle: PostgreSQL: postgresql://postgres:postgres@localhost:15432/postgres
 INFO  alle: WebSocket: 127.0.0.1:8080
 INFO  alle: No initial channels - clients will subscribe dynamically
 INFO  alle::postgres: Connected to PostgreSQL
 INFO  alle::websocket: WebSocket server listening on 127.0.0.1:8080
```

### Terminal 2: Listen to Channels

```bash
cargo run -- listen orders,alerts
```

Output:
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

```

**Interactive commands are available** - you can type commands while listening!

### Terminal 3: Send Notifications

```bash
# Send a simple message
cargo run -- publish orders "New order #123 received"

# Send JSON data
cargo run -- publish orders '{"order_id":123,"customer":"John Doe","total":99.99}'

# Send an alert
cargo run -- publish alerts "System check completed"
```

Output from each publish:
```
Connecting to WebSocket: ws://127.0.0.1:8080
Publishing to channel 'orders': New order #123 received
✓ Notification sent successfully
```

### Terminal 2 Output (Listener)

As you send notifications, Terminal 2 will show:
```
[orders] New order #123 received
[orders] {"order_id":123,"customer":"John Doe","total":99.99}
[alerts] System check completed
```

## Testing the Complete Flow

This demonstrates the full cycle: WebSocket → Postgres → WebSocket

### 1. Start the Server

```bash
docker-compose up -d
cargo run -- serve -p "postgresql://postgres:postgres@localhost:15432/postgres"
```

### 2. Start Two Listeners

Terminal 2:
```bash
cargo run -- listen orders
```

Terminal 3:
```bash
# Using websocat
websocat ws://127.0.0.1:8080
```

In websocat, type:
```json
{"type":"subscribe","channel":"orders"}
```

### 3. Publish a Message

Terminal 4:
```bash
cargo run -- publish orders "Test message"
```

### 4. Verify

- Terminal 2 (CLI listener) shows: `[orders] Test message`
- Terminal 3 (websocat) shows: `{"type":"notification","channel":"orders","payload":"Test message"}`

## Testing Postgres Triggers

The Docker setup includes automatic triggers on the `orders` table.

### Terminal 1: Start Server
```bash
docker-compose up -d
cargo run -- serve -p "postgresql://postgres:postgres@localhost:15432/postgres"
```

### Terminal 2: Listen
```bash
cargo run -- listen orders
```

### Terminal 3: Insert Data
```bash
docker-compose exec postgres psql -U postgres -c \
  "INSERT INTO orders (customer_name, total, status) VALUES ('Test User', 149.99, 'pending');"
```

### Terminal 2 Output
```
[orders] {"id": 4, "customer_name": "Test User", "total": 149.99, "status": "pending"}
```

## Testing Multiple Channels

### Terminal 1: Server
```bash
cargo run -- serve
```

### Terminal 2: Listen to Multiple Channels
```bash
cargo run -- listen orders,alerts,notifications
```

### Terminal 3: Send to Different Channels
```bash
cargo run -- publish orders "Order message"
cargo run -- publish alerts "Alert message"
cargo run -- publish notifications "Notification message"
```

### Terminal 2 Output
```
[orders] Order message
[alerts] Alert message
[notifications] Notification message
```

## Testing Interactive REPL Commands

The `listen` command includes an interactive REPL for managing subscriptions dynamically.

### Terminal 1: Start Server
```bash
cargo run -- serve
```

### Terminal 2: Start Listener (with no initial channels)
```bash
cargo run -- listen
```

Or with some initial channels:
```bash
cargo run -- listen orders
```

### Terminal 2: Interactive Session

While listening, type commands:

```
> sub alerts
✓ Subscribed to channel 'alerts'

> sub notifications
✓ Subscribed to channel 'notifications'

> list
Subscribed channels:
  - alerts
  - notifications
  - orders
```

### Terminal 3: Send Messages
```bash
cargo run -- publish orders "Order message"
cargo run -- publish alerts "Alert message"
cargo run -- publish notifications "Info message"
```

### Terminal 2: See Messages Arrive
```
[orders] Order message
[alerts] Alert message
[notifications] Info message
```

### Terminal 2: Unsubscribe
```
> unsub orders
✓ Unsubscribed from channel 'orders'

> list
Subscribed channels:
  - alerts
  - notifications
```

Now messages to `orders` will not be received, but `alerts` and `notifications` will still come through.

### Terminal 2: Exit
```
> quit
Exiting...
```

Or press Ctrl+C.

## Testing with Different Ports

### Start Server on Custom Port

Terminal 1:
```bash
cargo run -- serve -w "0.0.0.0:9000"
```

### Connect Clients to Custom Port

Terminal 2:
```bash
cargo run -- listen -w "ws://localhost:9000" orders
```

Terminal 3:
```bash
cargo run -- publish -w "ws://localhost:9000" orders "Custom port message"
```

## Testing Error Handling

### Server Not Running

```bash
cargo run -- listen orders
```

Output:
```
Connecting to WebSocket: ws://127.0.0.1:8080
Error: Failed to connect to WebSocket server

Caused by:
    Connection refused (os error 111)
```

### Invalid JSON

Using websocat:
```bash
websocat ws://127.0.0.1:8080
```

Type invalid JSON:
```
{invalid json}
```

Server logs will show:
```
WARN  alle::websocket: Failed to parse message from WebSocket client: expected value at line 1 column 2
```

## Performance Testing

### Load Test with Multiple Publishers

```bash
# Terminal 1: Start server
cargo run --release -- serve

# Terminal 2: Listen
cargo run --release -- listen orders

# Terminal 3: Send many messages
for i in {1..100}; do
  cargo run --release -- publish orders "Message $i"
done
```

### Concurrent Listeners

Start multiple listeners in different terminals:

```bash
# Terminal 2-5: All listening to the same channel
cargo run -- listen orders

# Terminal 6: Publish
cargo run -- publish orders "Broadcast message"
```

All listeners should receive the message simultaneously.

## Debugging

### Enable Debug Logging

```bash
# Server with debug logging
cargo run -- serve -l "alle=debug,debug"

# Listener with debug logging
cargo run -- listen -l "alle=debug,debug" orders
```

### Monitor Raw WebSocket Traffic

Using websocat in verbose mode:
```bash
websocat -v ws://127.0.0.1:8080
```

## Clean Up

Stop the PostgreSQL container:
```bash
docker-compose down

# Remove volumes to start fresh
docker-compose down -v
```

## Troubleshooting

### Connection Refused

Make sure the server is running:
```bash
# Check if server is listening
netstat -tlnp | grep 8080
# or
lsof -i :8080
```

### Postgres Connection Failed

Check the connection string:
```bash
# Test connection with psql
psql "postgresql://postgres:postgres@localhost:15432/postgres" -c "SELECT 1;"
```

### No Messages Received

1. Verify the server is running
2. Check that you're listening to the correct channel
3. Verify the channel name matches exactly (case-sensitive)
4. Check server logs for errors

### Messages Not Persisted in Postgres

The `publish` command sends to Postgres NOTIFY, which is **not** persistent. NOTIFY is for real-time notifications, not data storage. If you need persistence, insert into a table that has a trigger.
