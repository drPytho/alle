use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{
    BridgeConfig, ChannelName, NotificationMessage, PostgresListener, WebSocketServer,
    auth::Authenticator, server_push::server,
};

/// Main bridge that connects Postgres NOTIFY with WebSocket clients
pub struct Bridge {
    config: BridgeConfig,
    cancellation_token: CancellationToken,
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
        Self {
            config,
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Get a handle to the cancellation token for graceful shutdown
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Initiate graceful shutdown of the bridge
    /// This will stop accepting new connections and signal all servers to terminate
    pub fn terminate(&self) {
        info!("Bridge termination requested");
        self.cancellation_token.cancel();
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

        // Create authenticator if auth config is provided
        let authenticator = if let Some(auth_config) = self.config.auth_config {
            info!(
                "Authentication enabled with function: {}",
                auth_config.pg_function
            );
            let auth = Authenticator::new(&self.config.postgres_url, auth_config).await?;
            Some(Arc::new(auth))
        } else {
            info!("Authentication disabled");
            None
        };

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
                let auth_ws = authenticator.clone();
                let auth_se = authenticator.clone();
                let cancel_token_ws = self.cancellation_token.clone();
                let cancel_token_se = self.cancellation_token.clone();

                // Run both servers concurrently
                tokio::try_join!(
                    async move {
                        info!("WebSocket server starting");
                        let ws_server = WebSocketServer::new(ws_addr, pg_listener_ws, auth_ws);
                        ws_server.start(cancel_token_ws).await
                    },
                    async move {
                        info!("Server-Side Events server starting");
                        server(se_addr, pg_listener_se, auth_se, cancel_token_se).await
                    }
                )?;
            }
            (Some(ws_addr), None) => {
                info!("WebSocket server path");
                let ws_server =
                    WebSocketServer::new(ws_addr.clone(), Arc::clone(&pg_listener), authenticator);
                ws_server.start(self.cancellation_token.clone()).await?;
            }
            (None, Some(se_addr)) => {
                info!("Server-Side Events server path");
                server(
                    se_addr.clone(),
                    Arc::clone(&pg_listener),
                    authenticator,
                    self.cancellation_token.clone(),
                )
                .await?;
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
