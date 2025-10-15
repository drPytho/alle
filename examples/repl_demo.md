# Interactive REPL Demo

This document shows how to use the interactive REPL in the `alle listen` command.

## Starting with No Channels

You can start the listener with no initial subscriptions:

```bash
cargo run -- listen
```

Output:
```
Connecting to WebSocket: ws://127.0.0.1:8080
Starting with no initial subscriptions.

Commands:
  sub <channel>    - Subscribe to a channel
  unsub <channel>  - Unsubscribe from a channel
  list             - List subscribed channels
  help             - Show this help
  quit / exit      - Exit the program

Press Ctrl+C to stop

Type 'sub <channel>' to subscribe to channels

```

## Interactive Session

Now you can dynamically subscribe to channels:

```
> sub orders
✓ Subscribed to channel 'orders'

> sub alerts
✓ Subscribed to channel 'alerts'

> list
Subscribed channels:
  - alerts
  - orders
```

## Receiving Notifications

When notifications arrive, they appear in the output:

```
[orders] New order #123
[alerts] System check completed
```

## Managing Subscriptions

You can unsubscribe from channels you're no longer interested in:

```
> unsub alerts
✓ Unsubscribed from channel 'alerts'

> list
Subscribed channels:
  - orders
```

Now only messages on the `orders` channel will be received.

## Command Aliases

Some commands have shorter aliases:

- `sub` is short for `subscribe`
- `unsub` is short for `unsubscribe`

Both work the same:

```
> subscribe notifications
✓ Subscribed to channel 'notifications'

> unsubscribe notifications
✓ Unsubscribed from channel 'notifications'
```

## Getting Help

Type `help` at any time to see available commands:

```
> help

Commands:
  sub <channel>    - Subscribe to a channel
  unsub <channel>  - Unsubscribe from a channel
  list             - List subscribed channels
  help             - Show this help
  quit / exit      - Exit the program

```

## Exiting

Exit the program with `quit` or `exit`:

```
> quit
Exiting...
```

Or press Ctrl+C at any time.

## Full Example Session

Here's a complete example of using the REPL:

### Terminal 1: Start Server
```bash
cargo run -- serve
```

### Terminal 2: Start Interactive Listener
```bash
cargo run -- listen
```

Interactive commands:
```
> sub orders
✓ Subscribed to channel 'orders'

> sub alerts
✓ Subscribed to channel 'alerts'

> list
Subscribed channels:
  - alerts
  - orders
```

### Terminal 3: Send Some Notifications
```bash
cargo run -- publish orders "First order"
cargo run -- publish alerts "System alert"
cargo run -- publish orders "Second order"
```

### Terminal 2: See Messages
```
[orders] First order
[alerts] System alert
[orders] Second order
```

### Terminal 2: Continue Interactive Session
```
> unsub orders
✓ Unsubscribed from channel 'orders'
```

### Terminal 3: Send More Notifications
```bash
cargo run -- publish orders "Third order"
cargo run -- publish alerts "Another alert"
```

### Terminal 2: Only See Subscribed Channel
```
[alerts] Another alert
```

Note: The "Third order" message is not received because we unsubscribed from `orders`.

### Terminal 2: Clean Exit
```
> quit
Exiting...
```

## Starting with Initial Channels

You can also start with initial channels and add more interactively:

```bash
cargo run -- listen orders
```

Then interactively add more:
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

## Use Cases

The interactive REPL is useful for:

1. **Debugging** - Quickly subscribe/unsubscribe to channels to see what's happening
2. **Monitoring** - Start monitoring and add channels as issues arise
3. **Exploration** - Discover what channels are available by subscribing to them
4. **Testing** - Test channel subscription logic without restarting the client
5. **Production Monitoring** - Monitor production channels and adjust subscriptions on the fly

## Tips

- Use `list` frequently to see what you're subscribed to
- Channel names are case-sensitive: `Orders` ≠ `orders`
- You can subscribe to channels that don't exist yet - you'll receive messages when they start
- The REPL works simultaneously with notifications - you can type commands while messages are arriving
- Empty lines are ignored - just press Enter to get a clean output
