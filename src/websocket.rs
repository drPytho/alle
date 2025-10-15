use anyhow::{Context, Result};
use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};
use tracing::{debug, error, info, warn};

use crate::{
    subscriptions::ClientId, ClientMessage, NotificationMessage, NotifyMessage, ServerMessage,
    SubscriptionManager,
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
                if let Err(e) =
                    handle_connection(stream, notification_rx, incoming_tx, subscription_manager)
                        .await
                {
                    error!("Error handling connection from {}: {}", addr, e);
                }
            });
        }
    }
}

/// Handle a single WebSocket connection
async fn handle_connection(
    stream: TcpStream,
    mut notification_rx: broadcast::Receiver<NotificationMessage>,
    incoming_tx: Arc<mpsc::UnboundedSender<NotifyMessage>>,
    subscription_manager: Arc<SubscriptionManager>,
) -> Result<()> {
    let ws_stream = accept_async(stream)
        .await
        .context("Failed to accept WebSocket connection")?;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Register this client with the subscription manager
    let client_id = subscription_manager.register_client().await;
    info!("Client {} connected", client_id);

    let result = handle_client_loop(
        client_id,
        &mut ws_sender,
        &mut ws_receiver,
        &mut notification_rx,
        &incoming_tx,
        &subscription_manager,
    )
    .await;

    // Clean up client subscriptions on disconnect
    subscription_manager.remove_client(client_id).await;
    info!("Client {} disconnected", client_id);

    result
}

/// Main client loop handling messages
async fn handle_client_loop(
    client_id: ClientId,
    ws_sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    ws_receiver: &mut SplitStream<WebSocketStream<TcpStream>>,
    notification_rx: &mut broadcast::Receiver<NotificationMessage>,
    incoming_tx: &Arc<mpsc::UnboundedSender<NotifyMessage>>,
    subscription_manager: &Arc<SubscriptionManager>,
) -> Result<()> {
    loop {
        tokio::select! {
            // Receive messages from Postgres and send to WebSocket client (if subscribed)
            msg = notification_rx.recv() => {
                match msg {
                    Ok(notif) => {
                        // Only send if client is subscribed to this channel
                        if subscription_manager.is_subscribed(client_id, &notif.channel).await {
                            let server_msg = ServerMessage::Notification {
                                channel: notif.channel.clone(),
                                payload: notif.payload.clone(),
                            };
                            let json = serde_json::to_string(&server_msg)?;

                            if let Err(e) = ws_sender.send(Message::Text(json)).await {
                                warn!("Failed to send message to client {}: {}", client_id, e);
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Client {} lagged behind by {} messages", client_id, n);
                        let error_msg = ServerMessage::Error {
                            message: format!("Lagged behind by {} messages", n),
                        };
                        let json = serde_json::to_string(&error_msg)?;
                        let _ = ws_sender.send(Message::Text(json)).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("Notification channel closed");
                        break;
                    }
                }
            }

            // Receive messages from WebSocket client
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(e) = handle_client_message(
                            client_id,
                            &text,
                            ws_sender,
                            incoming_tx,
                            subscription_manager,
                        )
                        .await
                        {
                            error!("Error handling client {} message: {}", client_id, e);
                            let error_msg = ServerMessage::Error {
                                message: format!("Error: {}", e),
                            };
                            let json = serde_json::to_string(&error_msg)?;
                            let _ = ws_sender.send(Message::Text(json)).await;
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        debug!("Client {} closed connection", client_id);
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if let Err(e) = ws_sender.send(Message::Pong(data)).await {
                            warn!("Failed to send pong to client {}: {}", client_id, e);
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        warn!("WebSocket error for client {}: {}", client_id, e);
                        break;
                    }
                    None => {
                        debug!("Client {} stream ended", client_id);
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
    client_id: ClientId,
    text: &str,
    ws_sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    incoming_tx: &Arc<mpsc::UnboundedSender<NotifyMessage>>,
    subscription_manager: &Arc<SubscriptionManager>,
) -> Result<()> {
    let client_msg: ClientMessage =
        serde_json::from_str(text).context("Failed to parse client message")?;

    match client_msg {
        ClientMessage::Subscribe { channel } => {
            subscription_manager
                .subscribe(client_id, channel.clone())
                .await;

            let response = ServerMessage::Subscribed { channel };
            let json = serde_json::to_string(&response)?;
            ws_sender.send(Message::Text(json)).await?;
        }

        ClientMessage::Unsubscribe { channel } => {
            subscription_manager
                .unsubscribe(client_id, channel.clone())
                .await;

            let response = ServerMessage::Unsubscribed { channel };
            let json = serde_json::to_string(&response)?;
            ws_sender.send(Message::Text(json)).await?;
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
