use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use tokio::sync::broadcast;
use tokio_postgres::{AsyncMessage, Client, NoTls};

use crate::NotificationMessage;

/// Validate channel name to prevent SQL injection
/// Only allows alphanumeric characters and underscores, must start with letter or underscore
fn validate_channel_name(channel: &str) -> Result<()> {
    if channel.is_empty() {
        return Err(anyhow!("Channel name cannot be empty"));
    }

    if channel.len() > 63 {
        return Err(anyhow!("Channel name too long (max 63 characters)"));
    }

    let first_char = channel.chars().next().unwrap();
    if !first_char.is_ascii_alphabetic() && first_char != '_' {
        return Err(anyhow!(
            "Channel name must start with a letter or underscore"
        ));
    }

    for ch in channel.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            return Err(anyhow!(
                "Channel name can only contain letters, numbers, and underscores"
            ));
        }
    }

    Ok(())
}

/// PostgreSQL LISTEN/NOTIFY client
pub struct PostgresListener {
    client: Client,
    notification_tx: broadcast::Sender<NotificationMessage>,

    channels: Arc<RwLock<HashMap<String, HashSet<u64>>>>,
}

impl PostgresListener {
    /// Create a new PostgreSQL listener
    pub async fn connect(
        postgres_url: &str,
        buffer_size: usize,
    ) -> Result<(Self, broadcast::Receiver<NotificationMessage>)> {
        let (client, mut connection) = tokio_postgres::connect(postgres_url, NoTls)
            .await
            .context("Failed to connect to PostgreSQL")?;

        tracing::info!("Connected to PostgreSQL");

        let (notification_tx, notification_rx) = broadcast::channel(buffer_size);
        let notification_tx_clone = notification_tx.clone();

        // Spawn a task to handle connection and notifications
        tokio::spawn(async move {
            let mut stream = futures::stream::poll_fn(move |cx| connection.poll_message(cx));

            while let Some(message) = stream.next().await {
                match message {
                    Ok(AsyncMessage::Notification(notif)) => {
                        let msg = NotificationMessage {
                            channel: notif.channel().to_string(),
                            payload: notif.payload().to_string(),
                        };
                        tracing::debug!(
                            "Received notification on channel '{}': {}",
                            msg.channel,
                            msg.payload
                        );

                        if let Err(e) = notification_tx_clone.send(msg) {
                            tracing::error!("Failed to broadcast notification: {}", e);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("PostgreSQL connection error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok((
            Self {
                client,
                notification_tx,
                channels: Arc::new(RwLock::new(HashMap::new())),
            },
            notification_rx,
        ))
    }

    pub async fn listen(&self, client_id: u64, channel: String) -> Result<()> {
        validate_channel_name(&channel)?;

        let mut channels = self.channels.write().await;

        let client_ids = channels.entry(channel.clone()).or_insert_with(HashSet::new);
        if client_ids.is_empty() {
            self.execute_listen(&channel).await?;
        }
        client_ids.insert(client_id);

        tracing::debug!("Listening on channel '{}'", channel);
        Ok(())
    }

    pub async fn unlisten(&self, client_id: u64, channel: String) -> Result<()> {
        validate_channel_name(&channel)?;

        let mut channels = self.channels.write().await;

        let client_ids = channels.entry(channel.clone()).or_insert_with(HashSet::new);
        client_ids.remove(&client_id);
        if client_ids.is_empty() {
            self.execute_unlisten(&channel).await?;
        }

        tracing::debug!("Stopped listening on channel '{}'", channel);
        Ok(())
    }

    pub async fn client_disconnect(&self, client_id: u64) -> Result<()> {
        let mut channels = self.channels.write().await;

        let vals = channels
            .iter()
            .filter_map(|(chan, set)| {
                if set.contains(&client_id) {
                    Some(chan.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<String>>();

        // Remove client from each channel's subscriber list
        for val in vals.into_iter() {
            let client_ids = channels.entry(val.clone()).or_insert_with(HashSet::new);
            client_ids.remove(&client_id);
            if client_ids.is_empty() {
                self.execute_unlisten(&val).await?;
            }
        }

        tracing::info!("Removed client {}", client_id);

        Ok(())
    }

    /// Subscribe to a channel (LISTEN)
    async fn execute_listen(&self, channel: &str) -> Result<()> {
        self.client
            .execute(&format!("LISTEN {}", channel), &[])
            .await
            .context(format!("Failed to LISTEN on channel '{}'", channel))?;

        Ok(())
    }
    /// Unsubscribe from a channel (UNLISTEN)
    async fn execute_unlisten(&self, channel: &str) -> Result<()> {
        validate_channel_name(channel)?;

        self.client
            .execute(&format!("UNLISTEN {}", channel), &[])
            .await
            .context(format!("Failed to UNLISTEN on channel '{}'", channel))?;

        Ok(())
    }

    /// Send a notification to a channel (NOTIFY)
    pub async fn notify(&self, channel: &str, payload: &str) -> Result<()> {
        validate_channel_name(channel)?;

        let query = format!("NOTIFY {}, '$1'", channel);
        self.client
            .execute(&query, &[&payload])
            .await
            .context(format!("Failed to NOTIFY on channel '{}'", channel))?;

        tracing::debug!("Sent notification to channel '{}': {}", channel, payload);
        Ok(())
    }

    /// Get a new receiver for notifications
    pub fn subscribe(&self) -> broadcast::Receiver<NotificationMessage> {
        self.notification_tx.subscribe()
    }
}
