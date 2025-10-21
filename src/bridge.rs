use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::info;

use crate::{
    server_push::server, BridgeConfig, ChannelName, Frontend, NotificationMessage,
    PostgresListener, WebSocketServer,
};

/// Main bridge that connects Postgres NOTIFY with WebSocket clients
pub struct Bridge {
    config: BridgeConfig,
}

#[derive(Debug, Clone)]
pub enum PgNotifyEvent {
    /// A channel needs to be listened to (first subscriber)
    ChannelSubscribe {
        client_id: u64,
        channel: ChannelName,
        chan: Sender<NotificationMessage>,
    },
    /// A channel is no longer needed (last subscriber left)
    ChannelUnsubscribe {
        client_id: u64,
        channel: ChannelName,
    },

    ClientDisconnect {
        client_id: u64,
    },

    NotifyMessage {
        channel: ChannelName,
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
        let pg_listener = PostgresListener::connect(&self.config.postgres_url).await?;
        let pg_listener = Arc::new(pg_listener);

        // Create subscription manager

        // Spawn task to handle subscription events (dynamic LISTEN/UNLISTEN)

        match self.config.frontend {
            Frontend::WebSocket { bind_addr } => {
                info!("websocker path");
                let ws_server = WebSocketServer::new(bind_addr, Arc::clone(&pg_listener));

                // Start WebSocket server (this forwards Postgres notifications to clients)
                ws_server.start().await?;
            }
            Frontend::ServerPush { bind_addr } => {
                info!("Server push path");
                if let Err(e) = server(bind_addr, Arc::clone(&pg_listener)).await {
                    panic!("{}", e);
                }
            }
        }

        Ok(())
    }
}
