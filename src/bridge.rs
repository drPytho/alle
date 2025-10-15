use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::{
    subscriptions::{SubscriptionEvent, SubscriptionManager},
    BridgeConfig, PostgresListener, WebSocketServer,
};

/// Main bridge that connects Postgres NOTIFY with WebSocket clients
pub struct Bridge {
    config: BridgeConfig,
}

impl Bridge {
    /// Create a new bridge with the given configuration
    pub fn new(config: BridgeConfig) -> Self {
        Self { config }
    }

    /// Start the bridge
    /// This will:
    /// 1. Connect to PostgreSQL
    /// 2. Create subscription manager for dynamic channel management
    /// 3. Start the WebSocket server
    /// 4. Dynamically LISTEN/UNLISTEN on Postgres based on client subscriptions
    /// 5. Forward messages bidirectionally between Postgres and WebSocket clients
    pub async fn run(self) -> Result<()> {
        info!("Starting bridge with dynamic subscription management");

        // Connect to PostgreSQL
        let (pg_listener, notification_rx) =
            PostgresListener::connect(&self.config.postgres_url, 1000).await?;
        let pg_listener = Arc::new(pg_listener);

        // Create subscription manager
        let (subscription_manager, mut subscription_events) = SubscriptionManager::new();
        let subscription_manager = Arc::new(subscription_manager);

        // Subscribe to initial channels if configured
        for channel in &self.config.listen_channels {
            pg_listener.listen(channel).await?;
            info!("Initially listening on channel '{}'", channel);
        }

        // Spawn task to handle subscription events (dynamic LISTEN/UNLISTEN)
        let pg_listener_for_events = Arc::clone(&pg_listener);
        tokio::spawn(async move {
            while let Some(event) = subscription_events.recv().await {
                // TODO: This contains a race condition
                match event {
                    SubscriptionEvent::ChannelNeeded { channel } => {
                        info!("First subscriber to '{}', issuing LISTEN", channel);
                        if let Err(e) = pg_listener_for_events.listen(&channel).await {
                            tracing::error!("Failed to LISTEN on channel '{}': {}", channel, e);
                        }
                    }

                    SubscriptionEvent::ChannelNotNeeded { channel } => {
                        info!("No more subscribers to '{}', issuing UNLISTEN", channel);
                        if let Err(e) = pg_listener_for_events.unlisten(&channel).await {
                            tracing::error!("Failed to UNLISTEN on channel '{}': {}", channel, e);
                        }
                    }
                }
            }
        });

        // Create WebSocket server
        let (ws_server, mut incoming_rx) = WebSocketServer::new(
            self.config.ws_bind_addr.clone(),
            Arc::clone(&subscription_manager),
        );

        // Spawn task to handle incoming messages from WebSocket clients and send to Postgres
        let pg_listener_for_notify = Arc::clone(&pg_listener);
        tokio::spawn(async move {
            while let Some(notify_msg) = incoming_rx.recv().await {
                if let Err(e) = pg_listener_for_notify
                    .notify(&notify_msg.channel, &notify_msg.payload)
                    .await
                {
                    tracing::error!("Failed to send NOTIFY to Postgres: {}", e);
                }
            }
        });

        // Start WebSocket server (this forwards Postgres notifications to clients)
        ws_server.start(notification_rx).await?;

        Ok(())
    }
}
