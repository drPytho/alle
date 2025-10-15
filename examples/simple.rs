use alle::{Bridge, BridgeConfig};
use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Simple example demonstrating the bridge
///
/// This example starts the bridge with a single channel called "events".
///
/// To test:
/// 1. Run this example: `cargo run --example simple`
/// 2. In another terminal, connect with websocat: `websocat ws://127.0.0.1:8080`
/// 3. In a third terminal, send a Postgres notification:
///    `psql postgresql://localhost/postgres -c "NOTIFY events, 'Hello World!'"`
///
/// You should see the message in the websocat terminal.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "simple=debug,alle=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    println!("Starting WebSocket-Postgres bridge example");
    println!("PostgreSQL: postgresql://localhost/postgres");
    println!("WebSocket: ws://127.0.0.1:8080");
    println!("Channels: events");

    // Configure the bridge
    let config = BridgeConfig::new(
        "postgresql://localhost/postgres".to_string(),
        "127.0.0.1:8080".to_string(),
    )
    .with_channels(vec!["events".to_string()]);

    // Start the bridge
    let bridge = Bridge::new(config);
    bridge.run().await?;

    Ok(())
}
