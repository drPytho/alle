pub mod bridge;
mod drop_stream;
pub mod postgres;
pub mod server_push;
pub mod websocket;

pub use bridge::Bridge;
pub use postgres::PostgresListener;
pub use websocket::WebSocketServer;

use serde::{Deserialize, Serialize};

type ClientId = u64;

/// Message sent from Postgres NOTIFY to WebSocket clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationMessage {
    pub channel: String,
    pub payload: String,
}

/// Message sent from WebSocket clients to Postgres NOTIFY
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyMessage {
    pub channel: String,
    pub payload: String,
}

/// Messages from WebSocket clients to the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Subscribe to a channel
    Subscribe { channel: String },
    /// Unsubscribe from a channel
    Unsubscribe { channel: String },
    /// Send a NOTIFY to Postgres
    Notify { channel: String, payload: String },
}

/// Messages from server to WebSocket clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Notification from Postgres
    Notification { channel: String, payload: String },
    /// Subscription confirmed
    Subscribed { channel: String },
    /// Unsubscription confirmed
    Unsubscribed { channel: String },
    /// Error message
    Error { message: String },
}

#[derive(Debug, Clone)]
pub enum Frontend {
    WebSocket { bind_addr: String },
    ServerPush { bind_addr: String },
}

/// Configuration for the bridge
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// PostgreSQL connection string
    pub postgres_url: String,

    /// WebSocket server bind address
    pub frontend: Frontend,

    /// Channels to listen to on Postgres
    pub listen_channels: Vec<String>,
}

impl BridgeConfig {
    pub fn new(postgres_url: String, frontend: Frontend) -> Self {
        Self {
            postgres_url,
            frontend,
            listen_channels: Vec::new(),
        }
    }

    pub fn with_channels(mut self, channels: Vec<String>) -> Self {
        self.listen_channels = channels;
        self
    }
}
