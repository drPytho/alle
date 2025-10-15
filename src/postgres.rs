use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::sync::broadcast;
use tokio_postgres::{AsyncMessage, Client, NoTls};
use tracing::{debug, error, info};

use crate::NotificationMessage;

/// PostgreSQL LISTEN/NOTIFY client
pub struct PostgresListener {
    client: Client,
    notification_tx: broadcast::Sender<NotificationMessage>,
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

        info!("Connected to PostgreSQL");

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
                        debug!(
                            "Received notification on channel '{}': {}",
                            msg.channel, msg.payload
                        );

                        if let Err(e) = notification_tx_clone.send(msg) {
                            error!("Failed to broadcast notification: {}", e);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("PostgreSQL connection error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok((
            Self {
                client,
                notification_tx,
            },
            notification_rx,
        ))
    }

    /// Subscribe to a channel (LISTEN)
    pub async fn listen(&self, channel: &str) -> Result<()> {
        self.client
            .execute(&format!("LISTEN {}", channel), &[])
            .await
            .context(format!("Failed to LISTEN on channel '{}'", channel))?;

        info!("Listening on channel '{}'", channel);
        Ok(())
    }

    /// Unsubscribe from a channel (UNLISTEN)
    pub async fn unlisten(&self, channel: &str) -> Result<()> {
        self.client
            .execute(&format!("UNLISTEN {}", channel), &[])
            .await
            .context(format!("Failed to UNLISTEN on channel '{}'", channel))?;

        info!("Stopped listening on channel '{}'", channel);
        Ok(())
    }

    /// Send a notification to a channel (NOTIFY)
    pub async fn notify(&self, channel: &str, payload: &str) -> Result<()> {
        let query = format!("NOTIFY {}, '{}'", channel, payload.replace('\'', "''"));
        self.client
            .execute(&query, &[])
            .await
            .context(format!("Failed to NOTIFY on channel '{}'", channel))?;

        debug!("Sent notification to channel '{}': {}", channel, payload);
        Ok(())
    }

    /// Get a new receiver for notifications
    pub fn subscribe(&self) -> broadcast::Receiver<NotificationMessage> {
        self.notification_tx.subscribe()
    }
}
