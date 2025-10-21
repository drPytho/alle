use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc::Sender, RwLock};

use anyhow::{Context, Result};
use futures::StreamExt;
use tokio_postgres::{AsyncMessage, Client, NoTls};

use crate::{ChannelName, NotificationMessage};

type ChannelsMap = HashMap<ChannelName, HashMap<u64, Sender<NotificationMessage>>>;

/// PostgreSQL LISTEN/NOTIFY client
pub struct PostgresListener {
    client: Client,
    channels: Arc<RwLock<ChannelsMap>>,
}

impl PostgresListener {
    /// Create a new PostgreSQL listener
    pub async fn connect(postgres_url: &str) -> Result<Self> {
        let (client, mut connection) = tokio_postgres::connect(postgres_url, NoTls)
            .await
            .context("Failed to connect to PostgreSQL")?;

        tracing::info!("Connected to PostgreSQL");

        let channels: Arc<RwLock<ChannelsMap>> = Arc::new(RwLock::new(HashMap::new()));
        // Spawn a task to handle connection and notifications
        let channel_map = channels.clone();

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

                        let channel_map_guard = channel_map.read().await;

                        if let Some(set) = channel_map_guard.get(&channel) {
                            let mut dead_ids = Vec::new();

                            for (id, chan) in set.iter() {
                                if let Err(e) = chan.send(msg.clone()).await {
                                    tracing::warn!("Receiver {} disconnected: {}", id, e);
                                    dead_ids.push(*id);
                                }
                            }

                            drop(channel_map_guard); // Release read lock

                            if !dead_ids.is_empty() {
                                let mut channel_map_guard = channel_map.write().await;
                                if let Some(set) = channel_map_guard.get_mut(&channel) {
                                    for id in dead_ids {
                                        set.remove(&id);
                                    }
                                }
                            }
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

        Ok(Self { client, channels })
    }

    pub async fn listen(
        &self,
        client_id: u64,
        channel: ChannelName,
        chan: Sender<NotificationMessage>,
    ) -> Result<()> {
        let mut channels = self.channels.write().await;

        let client_ids = channels.entry(channel.clone()).or_insert_with(HashMap::new);
        if client_ids.is_empty() {
            self.execute_listen(&channel).await?;
        }
        client_ids.insert(client_id, chan);

        Ok(())
    }

    pub async fn unlisten(&self, client_id: u64, channel: ChannelName) -> Result<()> {
        let mut channels = self.channels.write().await;

        let client_ids = channels.entry(channel.clone()).or_insert_with(HashMap::new);
        client_ids.remove(&client_id);
        if client_ids.is_empty() {
            self.execute_unlisten(&channel).await?;
        }

        Ok(())
    }

    /// Subscribe to multiple channels at once (more efficient than calling listen multiple times)
    pub async fn listen_many(
        &self,
        client_id: u64,
        channels_to_subscribe: &[ChannelName],
        chan: Sender<NotificationMessage>,
    ) -> Result<()> {
        let mut channels = self.channels.write().await;

        for channel in channels_to_subscribe {
            let client_ids = channels.entry(channel.clone()).or_insert_with(HashMap::new);
            let needs_listen = client_ids.is_empty();
            client_ids.insert(client_id, chan.clone());

            if needs_listen {
                self.execute_listen(channel).await?;
            }
        }

        Ok(())
    }

    /// Unsubscribe from multiple channels at once (more efficient than calling unlisten multiple times)
    pub async fn unlisten_many(
        &self,
        client_id: u64,
        channels_to_unsubscribe: &[ChannelName],
    ) -> Result<()> {
        let mut channels = self.channels.write().await;

        for channel in channels_to_unsubscribe {
            let client_ids = channels.entry(channel.clone()).or_insert_with(HashMap::new);
            client_ids.remove(&client_id);

            if client_ids.is_empty() {
                self.execute_unlisten(channel).await?;
            }
        }

        Ok(())
    }

    /// Subscribe to a channel (LISTEN)
    async fn execute_listen(&self, channel: &ChannelName) -> Result<()> {
        self.client
            .execute(&format!("LISTEN {}", channel), &[])
            .await
            .context(format!("Failed to LISTEN on channel '{}'", channel))?;

        tracing::debug!("Listening on channel '{}'", channel);
        Ok(())
    }
    /// Unsubscribe from a channel (UNLISTEN)
    async fn execute_unlisten(&self, channel: &ChannelName) -> Result<()> {
        self.client
            .execute(&format!("UNLISTEN {}", channel), &[])
            .await
            .context(format!("Failed to UNLISTEN on channel '{}'", channel))?;

        tracing::debug!("Stopped listening on channel '{}'", channel);
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

    #[cfg(test)]
    pub(crate) fn channels(&self) -> Arc<RwLock<ChannelsMap>> {
        self.channels.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    // Macro to get test database URL from environment or skip the test
    macro_rules! get_test_db_url {
        () => {
            match std::env::var("TEST_DATABASE_URL") {
                Ok(url) => url,
                Err(_) => {
                    eprintln!("Skipping test: TEST_DATABASE_URL not set");
                    return;
                }
            }
        };
    }

    // Unit tests for internal state management
    mod unit_tests {
        use super::*;

        #[tokio::test]
        async fn test_listen_adds_channel_and_client() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channel = ChannelName::new("test_channel").unwrap();
            let (tx, _rx) = mpsc::channel(10);

            listener.listen(1, channel.clone(), tx).await.unwrap();

            let channels_arc = listener.channels();
            let channels = channels_arc.read().await;
            assert!(channels.contains_key(&channel));
            assert!(channels.get(&channel).unwrap().contains_key(&1));
        }

        #[tokio::test]
        async fn test_listen_multiple_clients_same_channel() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channel = ChannelName::new("test_channel").unwrap();
            let (tx1, _rx1) = mpsc::channel(10);
            let (tx2, _rx2) = mpsc::channel(10);

            listener.listen(1, channel.clone(), tx1).await.unwrap();
            listener.listen(2, channel.clone(), tx2).await.unwrap();

            let channels_arc = listener.channels();
            let channels = channels_arc.read().await;
            let clients = channels.get(&channel).unwrap();
            assert_eq!(clients.len(), 2);
            assert!(clients.contains_key(&1));
            assert!(clients.contains_key(&2));
        }

        #[tokio::test]
        async fn test_unlisten_removes_client() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channel = ChannelName::new("test_channel").unwrap();
            let (tx, _rx) = mpsc::channel(10);

            listener.listen(1, channel.clone(), tx).await.unwrap();
            listener.unlisten(1, channel.clone()).await.unwrap();

            let channels_arc = listener.channels();
            let channels = channels_arc.read().await;
            let clients = channels.get(&channel).unwrap();
            assert!(!clients.contains_key(&1));
        }

        #[tokio::test]
        async fn test_listen_many_adds_multiple_channels() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channels_list = vec![
                ChannelName::new("channel1").unwrap(),
                ChannelName::new("channel2").unwrap(),
                ChannelName::new("channel3").unwrap(),
            ];
            let (tx, _rx) = mpsc::channel(10);

            listener.listen_many(1, &channels_list, tx).await.unwrap();

            let channels_arc = listener.channels();
            let channels = channels_arc.read().await;
            for channel in &channels_list {
                assert!(channels.contains_key(channel));
                assert!(channels.get(channel).unwrap().contains_key(&1));
            }
        }

        #[tokio::test]
        async fn test_unlisten_many_removes_multiple_channels() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channels_list = vec![
                ChannelName::new("channel1").unwrap(),
                ChannelName::new("channel2").unwrap(),
                ChannelName::new("channel3").unwrap(),
            ];
            let (tx, _rx) = mpsc::channel(10);

            listener.listen_many(1, &channels_list, tx).await.unwrap();
            listener.unlisten_many(1, &channels_list).await.unwrap();

            let channels_arc = listener.channels();
            let channels = channels_arc.read().await;
            for channel in &channels_list {
                let clients = channels.get(channel).unwrap();
                assert!(!clients.contains_key(&1));
            }
        }

        #[tokio::test]
        async fn test_multiple_clients_one_unlistens() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channel = ChannelName::new("test_channel").unwrap();
            let (tx1, _rx1) = mpsc::channel(10);
            let (tx2, _rx2) = mpsc::channel(10);

            // Both clients listen
            listener.listen(1, channel.clone(), tx1).await.unwrap();
            listener.listen(2, channel.clone(), tx2).await.unwrap();

            // Client 1 unlistens
            listener.unlisten(1, channel.clone()).await.unwrap();

            // Client 2 should still be subscribed
            let channels_arc = listener.channels();
            let channels = channels_arc.read().await;
            let clients = channels.get(&channel).unwrap();
            assert!(!clients.contains_key(&1));
            assert!(clients.contains_key(&2));
        }
    }

    // Integration tests for notification flow
    mod integration_tests {
        use super::*;
        use std::time::Duration;
        use tokio::time::timeout;

        #[tokio::test]
        async fn test_notify_and_receive() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channel = ChannelName::new("test_notify").unwrap();
            let (tx, mut rx) = mpsc::channel(10);

            listener.listen(1, channel.clone(), tx).await.unwrap();

            // Give some time for subscription to be established
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Send notification
            listener.notify(&channel, "test payload").await.unwrap();

            // Receive notification
            let result = timeout(Duration::from_secs(2), rx.recv()).await;
            assert!(result.is_ok());
            let msg = result.unwrap();
            assert!(msg.is_some());
            let msg = msg.unwrap();
            assert_eq!(msg.channel, channel);
            assert_eq!(msg.payload, "test payload");
        }

        #[tokio::test]
        async fn test_multiple_clients_receive_same_notification() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channel = ChannelName::new("test_multi").unwrap();
            let (tx1, mut rx1) = mpsc::channel(10);
            let (tx2, mut rx2) = mpsc::channel(10);

            listener.listen(1, channel.clone(), tx1).await.unwrap();
            listener.listen(2, channel.clone(), tx2).await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Send notification
            listener
                .notify(&channel, "broadcast message")
                .await
                .unwrap();

            // Both clients should receive it
            let result1 = timeout(Duration::from_secs(2), rx1.recv()).await;
            let result2 = timeout(Duration::from_secs(2), rx2.recv()).await;

            assert!(result1.is_ok());
            assert!(result2.is_ok());

            let msg1 = result1.unwrap().unwrap();
            let msg2 = result2.unwrap().unwrap();

            assert_eq!(msg1.payload, "broadcast message");
            assert_eq!(msg2.payload, "broadcast message");
        }

        #[tokio::test]
        async fn test_unlistened_client_does_not_receive() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channel = ChannelName::new("test_unlisten").unwrap();
            let (tx, mut rx) = mpsc::channel(10);

            listener
                .listen(1, channel.clone(), tx.clone())
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;

            listener.unlisten(1, channel.clone()).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Send notification after unlistening
            listener
                .notify(&channel, "should not receive")
                .await
                .unwrap();

            // Should not receive anything
            let result = timeout(Duration::from_millis(500), rx.recv()).await;
            assert!(result.is_err(), "got {:?}", result); // Timeout should occur
        }

        #[tokio::test]
        async fn test_different_channels_isolated() {
            let db_url = get_test_db_url!();
            let listener = PostgresListener::connect(&db_url).await.unwrap();
            let channel1 = ChannelName::new("channel_a").unwrap();
            let channel2 = ChannelName::new("channel_b").unwrap();
            let (tx1, mut rx1) = mpsc::channel(10);
            let (tx2, mut rx2) = mpsc::channel(10);

            listener.listen(1, channel1.clone(), tx1).await.unwrap();
            listener.listen(2, channel2.clone(), tx2).await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Notify only channel1
            listener
                .notify(&channel1, "only for channel1")
                .await
                .unwrap();

            // Client 1 should receive, client 2 should not
            let result1 = timeout(Duration::from_secs(2), rx1.recv()).await;
            assert!(result1.is_ok());
            let msg1 = result1.unwrap().unwrap();
            assert_eq!(msg1.payload, "only for channel1");

            let result2 = timeout(Duration::from_millis(500), rx2.recv()).await;
            assert!(result2.is_err()); // Should timeout
        }
    }
}
