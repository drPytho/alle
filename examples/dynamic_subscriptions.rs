use alle::{Bridge, BridgeConfig};
use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Example demonstrating dynamic channel subscriptions
///
/// This example shows how WebSocket clients can dynamically subscribe and unsubscribe
/// to different channels, and how the bridge automatically manages Postgres LISTEN/UNLISTEN.
///
/// To test:
/// 1. Run this example: `cargo run --example dynamic_subscriptions`
/// 2. In another terminal, connect with websocat: `websocat ws://127.0.0.1:8080`
/// 3. Subscribe to a channel:
///    {"type": "subscribe", "channel": "orders"}
/// 4. Send a Postgres notification:
///    `psql postgresql://localhost/postgres -c "NOTIFY orders, 'New order #123'"`
/// 5. You should receive: {"type":"notification","channel":"orders","payload":"New order #123"}
/// 6. Subscribe to another channel:
///    {"type": "subscribe", "channel": "alerts"}
/// 7. Unsubscribe from the first:
///    {"type": "unsubscribe", "channel": "orders"}
/// 8. Send a notification to a channel you can send notifications back to Postgres:
///    {"type": "notify", "channel": "alerts", "payload": "Test alert"}
///
/// Watch the logs to see automatic LISTEN/UNLISTEN happening!
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dynamic_subscriptions=debug,alle=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    println!("=== Dynamic Subscription Example ===");
    println!();
    println!("Starting WebSocket-Postgres bridge with dynamic subscriptions");
    println!("PostgreSQL: postgresql://localhost/postgres");
    println!("WebSocket: ws://127.0.0.1:8080");
    println!();
    println!("No channels are initially subscribed.");
    println!("Channels will be LISTEN'd to on Postgres when the first client subscribes,");
    println!("and UNLISTEN'd when the last client unsubscribes.");
    println!();
    println!("Message Protocol:");
    println!("  Subscribe:   {{\"type\": \"subscribe\", \"channel\": \"your_channel\"}}");
    println!("  Unsubscribe: {{\"type\": \"unsubscribe\", \"channel\": \"your_channel\"}}");
    println!("  Notify:      {{\"type\": \"notify\", \"channel\": \"your_channel\", \"payload\": \"your_message\"}}");
    println!();

    // Configure the bridge with NO initial channels
    // All channels will be added dynamically
    let config = BridgeConfig::new(
        "postgresql://localhost/postgres".to_string(),
        "127.0.0.1:8080".to_string(),
    )
    .with_channels(vec![]); // Start with no channels!

    // Start the bridge
    let bridge = Bridge::new(config);
    bridge.run().await?;

    Ok(())
}
