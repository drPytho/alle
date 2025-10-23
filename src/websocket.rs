use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::slice::from_ref;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};
use tracing::{debug, error, info, warn};

use crate::{
    ChannelName, ClientId, ClientMessage, NotificationMessage, PostgresListener, ServerMessage,
};

/// WebSocket server that handles client connections
pub struct WebSocketServer {
    bind_addr: String,
    pg_listener: Arc<PostgresListener>,

    next_client_id: u64,
}

impl WebSocketServer {
    /// Create a new WebSocket server
    pub fn new(bind_addr: String, pg_listener: Arc<PostgresListener>) -> Self {
        Self {
            bind_addr,
            pg_listener,
            next_client_id: 0,
        }
    }

    /// Start the WebSocket server
    pub async fn start(mut self) -> Result<()> {
        let listener = TcpListener::bind(&self.bind_addr)
            .await
            .context(format!("Failed to bind to {}", self.bind_addr))?;

        info!("WebSocket server listening on {}", self.bind_addr);

        loop {
            // Accept new WebSocket connections
            let (stream, addr) = listener.accept().await?;
            info!("New WebSocket connection from {}", addr);

            let client_id = self.next_client_id;
            self.next_client_id += 1;
            let pg_listener = Arc::clone(&self.pg_listener);

            tokio::spawn(async move {
                let mut client = match WebSocketClient::new(client_id, stream, pg_listener).await {
                    Ok(client) => client,
                    Err(err) => {
                        error!("Error instanciating the connection to {}: {}", addr, err);
                        return;
                    }
                };

                if let Err(e) = client.handle_connection().await {
                    error!("Error handling connection from {}: {}", addr, e);
                }
            });
        }
    }
}

struct WebSocketClient {
    channels: HashSet<ChannelName>,
    ws_stream: WebSocketStream<TcpStream>,
    client_id: ClientId,
    pg_listener: Arc<PostgresListener>,
}

impl WebSocketClient {
    async fn new(
        client_id: ClientId,
        stream: TcpStream,
        pg_listener: Arc<PostgresListener>,
    ) -> Result<Self> {
        let ws_stream = accept_async(stream)
            .await
            .context("Failed to accept WebSocket connection")?;

        Ok(Self {
            channels: HashSet::new(),
            ws_stream,
            client_id,
            pg_listener,
        })
    }
    async fn handle_connection(&mut self) -> Result<()> {
        // Register this client with the subscription manager
        info!("Client {} connected", self.client_id);

        let result = self.handle_client_loop().await;

        // Clean up client subscriptions on disconnect
        let channels_to_cleanup: Vec<ChannelName> = self.channels.iter().cloned().collect();
        if !channels_to_cleanup.is_empty() {
            if let Err(e) = self
                .pg_listener
                .unlisten_many(self.client_id, &channels_to_cleanup)
                .await
            {
                warn!(
                    "Failed to cleanup subscriptions for client {}: {}",
                    self.client_id, e
                );
            }
        }
        info!("Client {} disconnected", self.client_id);

        result
    }

    /// Main client loop handling messages
    async fn handle_client_loop(&mut self) -> Result<()> {
        let (sender, receiver) = mpsc::channel::<NotificationMessage>(32);
        let mut rece = ReceiverStream::new(receiver);
        loop {
            tokio::select! {
                // Receive messages from Postgres and send to WebSocket client (if subscribed)
                msg = rece.next() => {
                    match msg {
                        Some(notif) => {

                            // Only send if client is subscribed to this channel
                            if !self.channels.contains(&notif.channel) {
                                continue;
                            }

                            if let Err(e) = self.send(ServerMessage::Notification {
                                channel: notif.channel.clone(),
                                payload: notif.payload.clone(),
                            }).await {
                                warn!("Failed to send message to client {}: {}", self.client_id, e);
                                break;
                            }
                        }
                        None => {
                                warn!("We got an empty message");
                        }

                    }
                }

                // Receive messages from WebSocket client
                msg = self.ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Err(e) = self.handle_client_message(&text, &sender).await {
                                error!("Error handling client {} message: {}", self.client_id, e);
                                self.send(ServerMessage::Error {
                                    message: format!("Error: {}", e),
                                }).await?;
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            debug!("Client {} closed connection", self.client_id);
                            break;
                        }
                        Some(Ok(Message::Ping(data))) => {
                            if let Err(e) = self.ws_stream.send(Message::Pong(data)).await {
                                warn!("Failed to send pong to client {}: {}", self.client_id, e);
                                break;
                            }
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            warn!("WebSocket error for client {}: {}", self.client_id, e);
                            break;
                        }
                        None => {
                            debug!("Client {} stream ended", self.client_id);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle a message from a WebSocket client
    async fn handle_client_message(
        &mut self,
        text: &str,
        chan: &Sender<NotificationMessage>,
    ) -> Result<()> {
        let client_msg: ClientMessage =
            serde_json::from_str(text).context("Failed to parse client message")?;

        match client_msg {
            ClientMessage::Subscribe { channel } => {
                self.channels.insert(channel.clone());
                self.pg_listener
                    .listen_many(self.client_id, from_ref(&channel), chan.clone())
                    .await
                    .context("Failed to subscribe to channel")?;

                self.send(ServerMessage::Subscribed { channel }).await?;
            }

            ClientMessage::Unsubscribe { channel } => {
                self.channels.remove(&channel);
                self.pg_listener
                    .unlisten_many(self.client_id, from_ref(&channel))
                    .await
                    .context("Failed to unsubscribe from channel")?;

                self.send(ServerMessage::Unsubscribed { channel }).await?;
            }

            ClientMessage::Notify { channel, payload } => {
                self.pg_listener
                    .notify(&channel, &payload)
                    .await
                    .context("Failed to forward message to Postgres")?;
            }
        }

        Ok(())
    }

    async fn send(&mut self, response: ServerMessage) -> Result<()> {
        let json = serde_json::to_string(&response)?;
        self.ws_stream.send(Message::Text(json.into())).await?;
        Ok(())
    }
}
