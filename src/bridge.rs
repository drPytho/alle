use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::info;

use crate::{
    server_push::server, BridgeConfig, ChannelName, NotificationMessage, PostgresListener,
    WebSocketServer,
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
    /// 3. Start the WebSocket server and/or Server-Side Events server
    /// 4. Dynamically LISTEN/UNLISTEN on Postgres based on client subscriptions
    /// 5. Forward messages bidirectionally between Postgres and WebSocket clients
    pub async fn run(self) -> Result<()> {
        info!("Starting bridge with dynamic subscription management");

        // Connect to PostgreSQL
        let pg_listener = PostgresListener::connect(&self.config.postgres_url).await?;
        let pg_listener = Arc::new(pg_listener);

        // Create subscription manager

        // Spawn task to handle subscription events (dynamic LISTEN/UNLISTEN)

        // Spawn both servers if both are configured, or just one if only one is configured
        match (
            &self.config.frontend.websocket,
            &self.config.frontend.server_push,
        ) {
            (Some(ws_addr), Some(se_addr)) => {
                info!("Starting both WebSocket and Server-Side Events servers");
                let ws_addr = ws_addr.clone();
                let se_addr = se_addr.clone();
                let pg_listener_ws = Arc::clone(&pg_listener);
                let pg_listener_se = Arc::clone(&pg_listener);

                // Run both servers concurrently
                tokio::try_join!(
                    async move {
                        info!("WebSocket server starting");
                        let ws_server = WebSocketServer::new(ws_addr, pg_listener_ws);
                        ws_server.start().await
                    },
                    async move {
                        info!("Server-Side Events server starting");
                        server(se_addr, pg_listener_se, None).await
                    }
                )?;
            }
            (Some(ws_addr), None) => {
                info!("WebSocket server path");
                let ws_server = WebSocketServer::new(ws_addr.clone(), Arc::clone(&pg_listener));
                ws_server.start().await?;
            }
            (None, Some(se_addr)) => {
                info!("Server-Side Events server path");
                server(se_addr.clone(), Arc::clone(&pg_listener), None).await?;
            }
            (None, None) => {
                return Err(anyhow::anyhow!(
                    "At least one frontend (WebSocket or Server-Side Events) must be configured"
                ));
            }
        }

        Ok(())
    }
}
