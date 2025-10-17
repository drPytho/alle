use axum::{
    extract::{Query, State},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use std::{convert::Infallible, sync::atomic::AtomicU64};
use std::{sync::Arc, time::Duration};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;

use crate::{bridge::PgNotifyEvent, drop_stream::DropStream, ChannelName, NotificationMessage};

pub struct HttpPushServerState {
    pg_notify: UnboundedSender<PgNotifyEvent>,
    client_id_counter: Arc<AtomicU64>,
}

impl Clone for HttpPushServerState {
    fn clone(&self) -> Self {
        HttpPushServerState {
            pg_notify: self.pg_notify.clone(),
            client_id_counter: self.client_id_counter.clone(),
        }
    }
}

pub async fn server(bind_addr: String, pg_notify: UnboundedSender<PgNotifyEvent>) -> Result<()> {
    let listener = TcpListener::bind(bind_addr)
        .await
        .expect("We should be able to bind out server to addr");

    let state = HttpPushServerState {
        pg_notify,
        client_id_counter: Arc::new(AtomicU64::new(0)),
    };

    let app = Router::new()
        .route("/events", get(sse_handler))
        .route("/notify", post(notify_handler))
        .with_state(state);

    axum::serve(listener, app).await.unwrap();

    Ok(())
}
#[derive(Deserialize)]
struct Channels {
    channels: String,
}

#[derive(Deserialize)]
struct NotifyRequest {
    channel: ChannelName,
    payload: String,
}

#[derive(Serialize)]
struct EventMessage {
    channel: ChannelName,
    payload: String,
}

#[derive(Serialize)]
struct NotifyResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn sse_handler(
    channels: Query<Channels>,
    State(state): State<HttpPushServerState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(32);
    let client_id = state
        .client_id_counter
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Parse and validate channel names
    let filter_channels: Vec<ChannelName> = channels
        .channels
        .split(",")
        .filter_map(|ch| match ChannelName::new(ch) {
            Ok(channel_name) => Some(channel_name),
            Err(e) => {
                tracing::error!("Invalid channel name '{}': {}", ch, e);
                None
            }
        })
        .collect();

    // Subscribe to all valid channels
    for ch in filter_channels.iter() {
        state
            .pg_notify
            .send(PgNotifyEvent::ChannelSubscribe {
                client_id,
                channel: ch.clone(),
                chan: sender.clone(),
            })
            .expect("Should be able to send message");
    }

    let drop_channels = filter_channels.clone();

    let stream = ReceiverStream::new(receiver).filter_map(
        move |NotificationMessage { channel, payload }| {
            let message = EventMessage { channel, payload };
            let json_data =
                serde_json::to_string(&message).expect("Failed to serialize event message");
            let event = Event::default().data(json_data).event("message");

            Some(Ok(event))
        },
    );

    let stream = DropStream::new(stream, move || {
        for ch in drop_channels {
            state
                .pg_notify
                .send(PgNotifyEvent::ChannelUnsubscribe {
                    client_id,
                    channel: ch.clone(),
                })
                .expect("Should be able to send message");
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keep-alive"),
    )
}

async fn notify_handler(
    State(state): State<HttpPushServerState>,
    Json(request): Json<NotifyRequest>,
) -> Json<NotifyResponse> {
    match state.pg_notify.send(PgNotifyEvent::NotifyMessage {
        channel: request.channel,
        payload: request.payload,
    }) {
        Ok(_) => Json(NotifyResponse {
            success: true,
            error: None,
        }),
        Err(e) => {
            let error_msg = format!("Failed to push event: {}", e);
            tracing::error!("{}", error_msg);
            Json(NotifyResponse {
                success: false,
                error: Some(error_msg),
            })
        }
    }
}
