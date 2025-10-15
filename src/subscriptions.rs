use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info};

/// Unique identifier for a WebSocket client
pub type ClientId = u64;

/// Commands to modify subscriptions
#[derive(Debug)]
pub enum SubscriptionCommand {
    /// Subscribe a client to a channel
    Subscribe {
        client_id: ClientId,
        channel: String,
    },
    /// Unsubscribe a client from a channel
    Unsubscribe {
        client_id: ClientId,
        channel: String,
    },
    /// Remove all subscriptions for a client (when they disconnect)
    RemoveClient { client_id: ClientId },
}

/// Events emitted when subscriptions change
#[derive(Debug, Clone)]
pub enum SubscriptionEvent {
    /// A channel needs to be listened to (first subscriber)
    ChannelNeeded { channel: String },
    /// A channel is no longer needed (last subscriber left)
    ChannelNotNeeded { channel: String },
}

/// Manages subscriptions across all WebSocket clients
///
/// This tracks:
/// - Which channels each client is subscribed to
/// - Which clients are subscribed to each channel
/// - When to LISTEN/UNLISTEN on Postgres (optimization)
pub struct SubscriptionManager {
    /// Map of client ID to set of channels they're subscribed to
    client_channels: Arc<RwLock<HashMap<ClientId, HashSet<String>>>>,

    /// Map of channel to set of client IDs subscribed to it
    channel_clients: Arc<RwLock<HashMap<String, HashSet<ClientId>>>>,

    /// Sender for subscription events
    event_tx: mpsc::UnboundedSender<SubscriptionEvent>,

    /// Next client ID
    next_client_id: Arc<RwLock<ClientId>>,
}

impl SubscriptionManager {
    /// Create a new subscription manager
    pub fn new() -> (Self, mpsc::UnboundedReceiver<SubscriptionEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        (
            Self {
                client_channels: Arc::new(RwLock::new(HashMap::new())),
                channel_clients: Arc::new(RwLock::new(HashMap::new())),
                event_tx,
                next_client_id: Arc::new(RwLock::new(0)),
            },
            event_rx,
        )
    }

    /// Register a new client and get their unique ID
    pub async fn register_client(&self) -> ClientId {
        let mut next_id = self.next_client_id.write().await;
        let client_id = *next_id;
        *next_id += 1;

        // Initialize empty subscription set for this client
        self.client_channels
            .write()
            .await
            .insert(client_id, HashSet::new());

        info!("Registered client {}", client_id);
        client_id
    }

    /// Subscribe a client to a channel
    pub async fn subscribe(&self, client_id: ClientId, channel: String) -> bool {
        let mut client_channels = self.client_channels.write().await;
        let mut channel_clients = self.channel_clients.write().await;

        // Add channel to client's subscriptions
        let client_subs = client_channels
            .entry(client_id)
            .or_insert_with(HashSet::new);
        let newly_subscribed = client_subs.insert(channel.clone());

        if !newly_subscribed {
            debug!(
                "Client {} already subscribed to channel '{}'",
                client_id, channel
            );
            return false;
        }

        // Add client to channel's subscribers
        let channel_subs = channel_clients
            .entry(channel.clone())
            .or_insert_with(HashSet::new);
        let first_subscriber = channel_subs.is_empty();
        channel_subs.insert(client_id);

        info!("Client {} subscribed to channel '{}'", client_id, channel);

        // If this is the first subscriber, emit event to LISTEN on Postgres
        if first_subscriber {
            info!(
                "Channel '{}' now has subscribers, sending ChannelNeeded event",
                channel
            );
            let _ = self.event_tx.send(SubscriptionEvent::ChannelNeeded {
                channel: channel.clone(),
            });
        }

        true
    }

    /// Unsubscribe a client from a channel
    pub async fn unsubscribe(&self, client_id: ClientId, channel: String) -> bool {
        let mut client_channels = self.client_channels.write().await;
        let mut channel_clients = self.channel_clients.write().await;

        // Remove channel from client's subscriptions
        let removed = if let Some(client_subs) = client_channels.get_mut(&client_id) {
            client_subs.remove(&channel)
        } else {
            false
        };

        if !removed {
            debug!(
                "Client {} was not subscribed to channel '{}'",
                client_id, channel
            );
            return false;
        }

        // Remove client from channel's subscribers
        if let Some(channel_subs) = channel_clients.get_mut(&channel) {
            channel_subs.remove(&client_id);

            info!(
                "Client {} unsubscribed from channel '{}'",
                client_id, channel
            );

            // If this was the last subscriber, emit event to UNLISTEN on Postgres
            if channel_subs.is_empty() {
                info!(
                    "Channel '{}' has no more subscribers, sending ChannelNotNeeded event",
                    channel
                );
                channel_clients.remove(&channel);
                let _ = self.event_tx.send(SubscriptionEvent::ChannelNotNeeded {
                    channel: channel.clone(),
                });
            }
        }

        true
    }

    /// Remove all subscriptions for a client (called on disconnect)
    pub async fn remove_client(&self, client_id: ClientId) {
        let mut client_channels = self.client_channels.write().await;
        let mut channel_clients = self.channel_clients.write().await;

        // Get all channels this client was subscribed to
        if let Some(channels) = client_channels.remove(&client_id) {
            info!(
                "Removing client {} from {} channels",
                client_id,
                channels.len()
            );

            // Remove client from each channel's subscriber list
            for channel in channels {
                if let Some(channel_subs) = channel_clients.get_mut(&channel) {
                    channel_subs.remove(&client_id);

                    // If this was the last subscriber, emit event to UNLISTEN
                    if channel_subs.is_empty() {
                        info!(
                            "Channel '{}' has no more subscribers, sending ChannelNotNeeded event",
                            channel
                        );
                        channel_clients.remove(&channel);
                        let _ = self.event_tx.send(SubscriptionEvent::ChannelNotNeeded {
                            channel: channel.clone(),
                        });
                    }
                }
            }
        }

        info!("Removed client {}", client_id);
    }

    /// Get all channels a client is subscribed to
    pub async fn get_client_channels(&self, client_id: ClientId) -> HashSet<String> {
        self.client_channels
            .read()
            .await
            .get(&client_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Check if a client is subscribed to a channel
    pub async fn is_subscribed(&self, client_id: ClientId, channel: &str) -> bool {
        self.client_channels
            .read()
            .await
            .get(&client_id)
            .map(|channels| channels.contains(channel))
            .unwrap_or(false)
    }

    /// Get all clients subscribed to a channel
    pub async fn get_channel_clients(&self, channel: &str) -> HashSet<ClientId> {
        self.channel_clients
            .read()
            .await
            .get(channel)
            .cloned()
            .unwrap_or_default()
    }

    /// Get the total number of active channels
    pub async fn active_channel_count(&self) -> usize {
        self.channel_clients.read().await.len()
    }

    /// Get all active channels
    pub async fn get_active_channels(&self) -> Vec<String> {
        self.channel_clients.read().await.keys().cloned().collect()
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new().0
    }
}
