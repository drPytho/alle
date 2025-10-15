use alle::{PostgresListener, SubscriptionManager, WebSocketServer};
use anyhow::Result;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Advanced example showing direct usage of PostgresListener and WebSocketServer
///
/// This example demonstrates:
/// - Connecting to PostgreSQL directly
/// - Using a custom subscription manager
/// - Handling incoming WebSocket messages
/// - Sending custom notifications
/// - Dynamic subscription management
///
/// To test:
/// 1. Run this example: `cargo run --example advanced`
/// 2. Connect with websocat: `websocat ws://127.0.0.1:9000`
/// 3. Subscribe to a channel:
///    {"type": "subscribe", "channel": "chat"}
/// 4. Send a notification:
///    {"type": "notify", "channel": "chat", "payload": "Hello!"}
/// 5. Or trigger from Postgres:
///    `psql postgresql://localhost/postgres -c "NOTIFY chat, 'System message'"`
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "advanced=debug,alle=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    println!("Starting advanced WebSocket-Postgres bridge example");
    println!("PostgreSQL: postgresql://localhost/postgres");
    println!("WebSocket: ws://127.0.0.1:9000");
    println!("Using dynamic subscriptions - clients control which channels to subscribe to");

    // Connect to PostgreSQL
    let (pg_listener, notification_rx) =
        PostgresListener::connect("postgresql://localhost/postgres", 1000).await?;
    let pg_listener = Arc::new(pg_listener);

    // Create subscription manager
    let (subscription_manager, mut subscription_events) = SubscriptionManager::new();
    let subscription_manager = Arc::new(subscription_manager);

    // Spawn task to handle subscription events
    let pg_listener_for_events = Arc::clone(&pg_listener);
    tokio::spawn(async move {
        use alle::subscriptions::SubscriptionEvent;

        while let Some(event) = subscription_events.recv().await {
            match event {
                SubscriptionEvent::ChannelNeeded { channel } => {
                    tracing::info!("First subscriber to '{}', issuing LISTEN", channel);
                    if let Err(e) = pg_listener_for_events.listen(&channel).await {
                        tracing::error!("Failed to LISTEN on channel '{}': {}", channel, e);
                    }
                }
                SubscriptionEvent::ChannelNotNeeded { channel } => {
                    tracing::info!("No more subscribers to '{}', issuing UNLISTEN", channel);
                    if let Err(e) = pg_listener_for_events.unlisten(&channel).await {
                        tracing::error!("Failed to UNLISTEN on channel '{}': {}", channel, e);
                    }
                }
            }
        }
    });

    // Create WebSocket server
    let (ws_server, mut incoming_rx) = WebSocketServer::new(
        "127.0.0.1:9000".to_string(),
        Arc::clone(&subscription_manager),
    );

    // Spawn a task to handle incoming messages from WebSocket clients
    let pg_listener_for_notify = Arc::clone(&pg_listener);
    tokio::spawn(async move {
        while let Some(notify_msg) = incoming_rx.recv().await {
            tracing::info!(
                "Received WebSocket message for channel '{}': {}",
                notify_msg.channel,
                notify_msg.payload
            );

            // Send the notification to Postgres
            if let Err(e) = pg_listener_for_notify
                .notify(&notify_msg.channel, &notify_msg.payload)
                .await
            {
                tracing::error!("Failed to send NOTIFY to Postgres: {}", e);
            }
        }
    });

    // Start the WebSocket server
    ws_server.start(notification_rx).await?;

    Ok(())
}
