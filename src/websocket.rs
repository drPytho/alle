use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};
use tracing::{debug, error, info, warn};

use crate::{
    subscriptions::ClientId, subscriptions::SubscriptionManager, ClientMessage,
    NotificationMessage, NotifyMessage, ServerMessage,
};

/// WebSocket server that handles client connections
pub struct WebSocketServer {
    bind_addr: String,
    incoming_tx: mpsc::UnboundedSender<NotifyMessage>,
    subscription_manager: Arc<SubscriptionManager>,
}

impl WebSocketServer {
    /// Create a new WebSocket server
    pub fn new(
        bind_addr: String,
        subscription_manager: Arc<SubscriptionManager>,
    ) -> (Self, mpsc::UnboundedReceiver<NotifyMessage>) {
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();

        (
            Self {
                bind_addr,
                incoming_tx,
                subscription_manager,
            },
            incoming_rx,
        )
    }

    /// Start the WebSocket server
    pub async fn start(
        self,
        notification_rx: broadcast::Receiver<NotificationMessage>,
    ) -> Result<()> {
        let listener = TcpListener::bind(&self.bind_addr)
            .await
            .context(format!("Failed to bind to {}", self.bind_addr))?;

        info!("WebSocket server listening on {}", self.bind_addr);

        let incoming_tx = Arc::new(self.incoming_tx);
        let subscription_manager = self.subscription_manager;

        loop {
            // Accept new WebSocket connections
            let (stream, addr) = listener.accept().await?;
            info!("New WebSocket connection from {}", addr);

            let incoming_tx = Arc::clone(&incoming_tx);
            let subscription_manager = Arc::clone(&subscription_manager);
            let notification_rx = notification_rx.resubscribe();

            tokio::spawn(async move {
                let mut client = match WebSocketClient::new(stream, subscription_manager).await {
                    Ok(client) => client,
                    Err(err) => {
                        error!("Error instanciating the connection to {}: {}", addr, err);
                        return;
                    }
                };

                if let Err(e) = client.handle_connection(notification_rx, incoming_tx).await {
                    error!("Error handling connection from {}: {}", addr, e);
                }
            });
        }
    }
}

struct WebSocketClient {
    channels: HashSet<String>,
    ws_stream: WebSocketStream<TcpStream>,
    client_id: ClientId,
    subscription_manager: Arc<SubscriptionManager>,
}

impl WebSocketClient {
    async fn new(
        stream: TcpStream,
        subscription_manager: Arc<SubscriptionManager>,
    ) -> Result<Self> {
        let ws_stream = accept_async(stream)
            .await
            .context("Failed to accept WebSocket connection")?;

        let client_id = subscription_manager.register_client().await;

        Ok(Self {
            channels: HashSet::new(),
            ws_stream,
            client_id,
            subscription_manager,
        })
    }
    async fn handle_connection(
        &mut self,
        mut notification_rx: broadcast::Receiver<NotificationMessage>,
        incoming_tx: Arc<mpsc::UnboundedSender<NotifyMessage>>,
    ) -> Result<()> {
        // Register this client with the subscription manager
        info!("Client {} connected", self.client_id);

        let result = self
            .handle_client_loop(&mut notification_rx, &incoming_tx)
            .await;

        // Clean up client subscriptions on disconnect
        self.subscription_manager
            .remove_client(self.client_id)
            .await;
        info!("Client {} disconnected", self.client_id);

        result
    }

    /// Main client loop handling messages
    async fn handle_client_loop(
        &mut self,
        notification_rx: &mut broadcast::Receiver<NotificationMessage>,
        incoming_tx: &Arc<mpsc::UnboundedSender<NotifyMessage>>,
    ) -> Result<()> {
        loop {
            tokio::select! {
                // Receive messages from Postgres and send to WebSocket client (if subscribed)
                msg = notification_rx.recv() => {
                    match msg {
                        Ok(notif) => {
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
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("Client {} lagged behind by {} messages", self.client_id, n);
                            self.send(ServerMessage::Error {
                                message: format!("Lagged behind by {} messages", n),
                            }).await?;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            debug!("Notification channel closed");
                            break;
                        }
                    }
                }

                // Receive messages from WebSocket client
                msg = self.ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Err(e) = self.handle_client_message(
                                &text,
                                incoming_tx,
                            )
                            .await
                            {
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
        incoming_tx: &Arc<mpsc::UnboundedSender<NotifyMessage>>,
    ) -> Result<()> {
        let client_msg: ClientMessage =
            serde_json::from_str(text).context("Failed to parse client message")?;

        match client_msg {
            ClientMessage::Subscribe { channel } => {
                self.channels.insert(channel.clone());
                self.subscription_manager
                    .subscribe(self.client_id, channel.clone())
                    .await;

                self.send(ServerMessage::Subscribed { channel }).await?;
            }

            ClientMessage::Unsubscribe { channel } => {
                self.channels.remove(&channel);
                self.subscription_manager
                    .unsubscribe(self.client_id, channel.clone())
                    .await;

                self.send(ServerMessage::Unsubscribed { channel }).await?;
            }

            ClientMessage::Notify { channel, payload } => {
                let notify_msg = NotifyMessage { channel, payload };
                incoming_tx
                    .send(notify_msg)
                    .context("Failed to forward message to Postgres")?;
            }
        }

        Ok(())
    }

    async fn send(&mut self, response: ServerMessage) -> Result<()> {
        let json = serde_json::to_string(&response)?;
        self.ws_stream.send(Message::Text(json)).await?;
        Ok(())
    }
}
