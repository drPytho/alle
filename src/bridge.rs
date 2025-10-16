use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::{server_push::server, BridgeConfig, Frontend, PostgresListener, WebSocketServer};

/// Main bridge that connects Postgres NOTIFY with WebSocket clients
pub struct Bridge {
    config: BridgeConfig,
}

#[derive(Debug, Clone)]
pub enum PgNotifyEvent {
    /// A channel needs to be listened to (first subscriber)
    ChannelSubscribe {
        client_id: u64,
        channel: String,
    },
    /// A channel is no longer needed (last subscriber left)
    ChannelUnsubscribe {
        client_id: u64,
        channel: String,
    },

    ClientDisconnect {
        client_id: u64,
    },

    NotifyMessage {
        channel: String,
        payload: String,
    },
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

        let (pg_event_tx, mut pg_event_rx) = mpsc::unbounded_channel::<PgNotifyEvent>();

        // Spawn task to handle subscription events (dynamic LISTEN/UNLISTEN)
        let pg_listener_for_events = Arc::clone(&pg_listener);
        tokio::spawn(async move {
            while let Some(event) = pg_event_rx.recv().await {
                // TODO: This contains a race condition
                match event {
                    PgNotifyEvent::ChannelSubscribe { client_id, channel } => {
                        info!("First subscriber to '{}', issuing LISTEN", channel);
                        if let Err(e) = pg_listener_for_events
                            .listen(client_id, channel.clone())
                            .await
                        {
                            tracing::error!("Failed to LISTEN on channel '{}': {}", channel, e);
                        }
                    }

                    PgNotifyEvent::ChannelUnsubscribe { client_id, channel } => {
                        info!("No more subscribers to '{}', issuing UNLISTEN", channel);
                        if let Err(e) = pg_listener_for_events
                            .unlisten(client_id, channel.clone())
                            .await
                        {
                            tracing::error!("Failed to UNLISTEN on channel '{}': {}", channel, e);
                        }
                    }

                    PgNotifyEvent::ClientDisconnect { client_id: _ } => {
                        todo!();
                    }

                    PgNotifyEvent::NotifyMessage { channel, payload } => {
                        if let Err(e) = pg_listener_for_events.notify(&channel, &payload).await {
                            tracing::error!("Failed to send NOTIFY to Postgres: {}", e);
                        }
                    }
                }
            }
        });

        match self.config.frontend {
            Frontend::WebSocket { bind_addr } => {
                info!("websocker path");
                let ws_server = WebSocketServer::new(bind_addr, pg_event_tx.clone());

                // Start WebSocket server (this forwards Postgres notifications to clients)
                ws_server.start(notification_rx).await?;
            }
            Frontend::ServerPush { bind_addr } => {
                info!("Server push path");
                if let Err(e) = server(bind_addr, pg_event_tx.clone(), notification_rx).await {
                    panic!("{}", e);
                }
            }
        }

        Ok(())
    }
}
