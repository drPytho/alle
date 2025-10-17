use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::sync::broadcast;
use tokio_postgres::{AsyncMessage, Client, NoTls};

use crate::{ChannelName, NotificationMessage};

/// PostgreSQL LISTEN/NOTIFY client
pub struct PostgresListener {
    client: Client,
    notification_tx: broadcast::Sender<NotificationMessage>,

    channels: Arc<RwLock<HashMap<ChannelName, HashSet<u64>>>>,
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
                        // Parse channel name - this should always succeed since Postgres validated it
                        let channel = match ChannelName::new(notif.channel()) {
                            Ok(ch) => ch,
                            Err(e) => {
                                tracing::error!("Invalid channel name from Postgres: {}", e);
                                continue;
                            }
                        };

                        let msg = NotificationMessage {
                            channel: channel.clone(),
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

    pub async fn listen(&self, client_id: u64, channel: ChannelName) -> Result<()> {
        let mut channels = self.channels.write().await;

        let client_ids = channels.entry(channel.clone()).or_insert_with(HashSet::new);
        if client_ids.is_empty() {
            self.execute_listen(&channel).await?;
        }
        client_ids.insert(client_id);

        tracing::debug!("Listening on channel '{}'", channel);
        Ok(())
    }

    pub async fn unlisten(&self, client_id: u64, channel: ChannelName) -> Result<()> {
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
            .collect::<Vec<ChannelName>>();

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
    async fn execute_listen(&self, channel: &ChannelName) -> Result<()> {
        self.client
            .execute(&format!("LISTEN {}", channel), &[])
            .await
            .context(format!("Failed to LISTEN on channel '{}'", channel))?;

        Ok(())
    }
    /// Unsubscribe from a channel (UNLISTEN)
    async fn execute_unlisten(&self, channel: &ChannelName) -> Result<()> {
        self.client
            .execute(&format!("UNLISTEN {}", channel), &[])
            .await
            .context(format!("Failed to UNLISTEN on channel '{}'", channel))?;

        Ok(())
    }

    /// Send a notification to a channel (NOTIFY)
    pub async fn notify(&self, channel: &ChannelName, payload: &str) -> Result<()> {
        tracing::debug!("Channer: {}, Payload: {}", channel, payload);
        let channel_str = channel.as_str();
        if let Err(e) = self
            .client
            .execute("SELECT pg_notify($1, $2)", &[&channel_str, &payload])
            .await
        {
            tracing::error!("could not notify postgres {:?}", e.to_string());
            return Err(e.into());
        }

        tracing::debug!("Sent notification to channel '{}': {}", channel, payload);
        Ok(())
    }

    /// Get a new receiver for notifications
    pub fn subscribe(&self) -> broadcast::Receiver<NotificationMessage> {
        self.notification_tx.subscribe()
    }
}
