use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{atomic::AtomicU64, Arc};
use tokio::sync::{mpsc, RwLock};

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
    /// Map of channel to set of client IDs subscribed to it
    channel_clients: Arc<RwLock<HashMap<String, HashSet<ClientId>>>>,

    /// Sender for subscription events
    event_tx: mpsc::UnboundedSender<SubscriptionEvent>,

    /// Next client ID
    next_client_id: AtomicU64,
}

impl SubscriptionManager {
    /// Create a new subscription manager
    pub(crate) fn new() -> (Self, mpsc::UnboundedReceiver<SubscriptionEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        (
            Self {
                channel_clients: Arc::new(RwLock::new(HashMap::new())),
                event_tx,
                next_client_id: AtomicU64::new(0),
            },
            event_rx,
        )
    }

    /// Register a new client and get their unique ID
    pub(crate) async fn register_client(&self) -> ClientId {
        // NOTE: this should be able to be relaxed
        let next_id = self.next_client_id.fetch_add(1, Ordering::SeqCst);

        tracing::info!("Registered client {}", next_id);
        next_id
    }

    /// Subscribe a client to a channel
    pub(crate) async fn subscribe(&self, client_id: ClientId, channel: String) -> bool {
        let mut channel_clients = self.channel_clients.write().await;

        // Add client to channel's subscribers
        let channel_subs = channel_clients
            .entry(channel.clone())
            .or_insert_with(HashSet::new);
        let first_subscriber = channel_subs.is_empty();
        channel_subs.insert(client_id);

        tracing::info!("Client {} subscribed to channel '{}'", client_id, channel);

        // If this is the first subscriber, emit event to LISTEN on Postgres
        if first_subscriber {
            tracing::info!(
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
    pub(crate) async fn unsubscribe(&self, client_id: ClientId, channel: String) -> bool {
        let mut channel_clients = self.channel_clients.write().await;

        // Remove client from channel's subscribers
        if let Some(channel_subs) = channel_clients.get_mut(&channel) {
            channel_subs.remove(&client_id);

            tracing::info!(
                "Client {} unsubscribed from channel '{}'",
                client_id,
                channel
            );

            // If this was the last subscriber, emit event to UNLISTEN on Postgres
            if channel_subs.is_empty() {
                tracing::info!(
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
    pub(crate) async fn remove_client(&self, client_id: ClientId) {
        let mut channel_clients = self.channel_clients.write().await;

        // Remove client from each channel's subscriber list
        channel_clients.retain(|channel, channel_subs| {
            channel_subs.remove(&client_id);

            if channel_subs.is_empty() {
                tracing::info!(
                    "Channel '{}' has no more subscribers, sending ChannelNotNeeded event",
                    channel
                );
                let _ = self.event_tx.send(SubscriptionEvent::ChannelNotNeeded {
                    channel: channel.clone(),
                });
                false // Remove this entry
            } else {
                true // Keep this entry
            }
        });

        tracing::info!("Removed client {}", client_id);
    }
}
