pub mod auth;
pub mod bridge;
pub mod channel_name;
mod drop_stream;
pub(crate) mod metrics;
pub mod postgres;
pub mod server_push;
pub mod websocket;
pub use bridge::Bridge;
pub use channel_name::ChannelName;
pub use postgres::PostgresListener;
pub use websocket::WebSocketServer;

use serde::{Deserialize, Serialize};

use crate::auth::AuthConfig;

type ClientId = u64;

/// Message sent from Postgres NOTIFY to WebSocket clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub channel: ChannelName,
    pub payload: String,
}

/// Message sent from WebSocket clients to Postgres NOTIFY
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyMessage {
    pub channel: ChannelName,
    pub payload: String,
}

/// Messages from WebSocket clients to the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Authenticate with a token
    Authenticate { token: String },
    /// Subscribe to a channel
    Subscribe { channel: ChannelName },
    /// Unsubscribe from a channel
    Unsubscribe { channel: ChannelName },
    /// Send a NOTIFY to Postgres
    Notify {
        channel: ChannelName,
        payload: String,
    },
}

/// Messages from server to WebSocket clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Notification from Postgres
    Notification {
        channel: ChannelName,
        payload: String,
    },
    /// Subscription confirmed
    Subscribed { channel: ChannelName },
    /// Unsubscription confirmed
    Unsubscribed { channel: ChannelName },
    /// Authentication successful
    Authenticated { user_id: String },
    /// Error message
    Error { message: String },
}

#[derive(Debug, Clone)]
pub struct Frontend {
    pub websocket: Option<String>,
    pub server_push: Option<String>,
    pub sse_path: Option<String>,
}

impl Default for Frontend {
    fn default() -> Self {
        Self::new()
    }
}

impl Frontend {
    pub fn new() -> Self {
        Self {
            websocket: None,
            server_push: None,
            sse_path: None,
        }
    }

    pub fn with_websocket(mut self, bind_addr: String) -> Self {
        self.websocket = Some(bind_addr);
        self
    }

    pub fn with_server_push(mut self, bind_addr: String) -> Self {
        self.server_push = Some(bind_addr);
        self
    }

    pub fn with_sse_path(mut self, path: String) -> Self {
        self.sse_path = Some(path);
        self
    }
}

/// Configuration for the bridge
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// PostgreSQL connection string
    pub postgres_url: String,

    /// WebSocket server bind address
    pub frontend: Frontend,

    /// Channels to listen to on Postgres
    pub listen_channels: Vec<ChannelName>,

    pub auth_config: Option<AuthConfig>,
}

impl BridgeConfig {
    pub fn new(postgres_url: String, frontend: Frontend) -> Self {
        Self {
            postgres_url,
            frontend,
            listen_channels: Vec::new(),
            auth_config: None,
        }
    }

    pub fn with_auth(mut self, auth_config: AuthConfig) -> Self {
        self.auth_config = Some(auth_config);
        self
    }

    pub fn with_channels(mut self, channels: Vec<ChannelName>) -> Self {
        self.listen_channels = channels;
        self
    }
}
