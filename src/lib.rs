pub mod bridge;
mod drop_stream;
pub mod postgres;
pub mod server_push;
pub mod websocket;

pub use bridge::Bridge;
pub use postgres::PostgresListener;
pub use websocket::WebSocketServer;

use serde::{Deserialize, Serialize};
use std::fmt;

type ClientId = u64;

/// A validated, lowercase PostgreSQL channel name
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelName(String);

impl ChannelName {
    /// Create a new ChannelName, validating and converting to lowercase
    pub fn new(name: impl AsRef<str>) -> Result<Self, ChannelNameError> {
        let name = name.as_ref().to_lowercase();
        Self::validate(&name)?;
        Ok(Self(name))
    }

    /// Validate channel name rules
    fn validate(name: &str) -> Result<(), ChannelNameError> {
        if name.is_empty() {
            return Err(ChannelNameError::Empty);
        }

        if name.len() > 63 {
            return Err(ChannelNameError::TooLong);
        }

        let first_char = name.chars().next().unwrap();
        if !first_char.is_ascii_alphabetic() && first_char != '_' {
            return Err(ChannelNameError::InvalidStart);
        }

        for ch in name.chars() {
            if !ch.is_ascii_alphanumeric() && ch != '_' {
                return Err(ChannelNameError::InvalidChar);
            }
        }

        Ok(())
    }

    /// Get the channel name as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ChannelName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChannelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for ChannelName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChannelName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ChannelName::new(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone)]
pub enum ChannelNameError {
    Empty,
    TooLong,
    InvalidStart,
    InvalidChar,
}

impl fmt::Display for ChannelNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelNameError::Empty => write!(f, "Channel name cannot be empty"),
            ChannelNameError::TooLong => write!(f, "Channel name too long (max 63 characters)"),
            ChannelNameError::InvalidStart => {
                write!(f, "Channel name must start with a letter or underscore")
            }
            ChannelNameError::InvalidChar => write!(
                f,
                "Channel name can only contain letters, numbers, and underscores"
            ),
        }
    }
}

impl std::error::Error for ChannelNameError {}

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
    /// Subscribe to a channel
    Subscribe { channel: ChannelName },
    /// Unsubscribe from a channel
    Unsubscribe { channel: ChannelName },
    /// Send a NOTIFY to Postgres
    Notify { channel: ChannelName, payload: String },
}

/// Messages from server to WebSocket clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Notification from Postgres
    Notification { channel: ChannelName, payload: String },
    /// Subscription confirmed
    Subscribed { channel: ChannelName },
    /// Unsubscription confirmed
    Unsubscribed { channel: ChannelName },
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
    pub listen_channels: Vec<ChannelName>,
}

impl BridgeConfig {
    pub fn new(postgres_url: String, frontend: Frontend) -> Self {
        Self {
            postgres_url,
            frontend,
            listen_channels: Vec::new(),
        }
    }

    pub fn with_channels(mut self, channels: Vec<ChannelName>) -> Self {
        self.listen_channels = channels;
        self
    }
}
