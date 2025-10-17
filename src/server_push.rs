use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use std::{convert::Infallible, sync::atomic::AtomicU64};
use std::{sync::Arc, time::Duration};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use anyhow::Result;
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;
use tokio::{net::TcpListener, sync::broadcast};

use crate::{bridge::PgNotifyEvent, drop_stream::DropStream, NotificationMessage};

pub struct HttpPushServerState {
    pg_notify: UnboundedSender<PgNotifyEvent>,
    notify: broadcast::Receiver<NotificationMessage>,
    client_id_counter: Arc<AtomicU64>,
}

impl Clone for HttpPushServerState {
    fn clone(&self) -> Self {
        HttpPushServerState {
            pg_notify: self.pg_notify.clone(),
            notify: self.notify.resubscribe(),
            client_id_counter: self.client_id_counter.clone(),
        }
    }
}

pub async fn server(
    bind_addr: String,
    pg_notify: UnboundedSender<PgNotifyEvent>,
    notify: broadcast::Receiver<NotificationMessage>,
) -> Result<()> {
    let listener = TcpListener::bind(bind_addr)
        .await
        .expect("We should be able to bind out server to addr");

    let state = HttpPushServerState {
        pg_notify,
        notify,
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
    channel: String,
    payload: String,
}

async fn sse_handler(
    channels: Query<Channels>,
    State(state): State<HttpPushServerState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let client_id = state
        .client_id_counter
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let channels: Vec<&str> = channels.channels.split(",").collect();
    for ch in channels.iter() {
        state
            .pg_notify
            .send(PgNotifyEvent::ChannelSubscribe {
                client_id,
                channel: ch.to_string(),
            })
            .expect("Should be able to send message");
    }

    // Create guard that will send unsubscribe on drop

    let filter_channels: Vec<String> = channels.into_iter().map(|s| s.to_string()).collect();
    let drop_channels = filter_channels.clone();
    let stream = BroadcastStream::new(state.notify).filter_map(move |v| {
        // Keep guard alive by cloning Arc into closure

        match v {
            Ok(NotificationMessage { channel, payload }) => {
                if filter_channels.contains(&channel) {
                    let event = Event::default()
                        .data(format!(
                            r#"{{"channel":"{}","payload":"{}"}}"#,
                            channel, payload
                        ))
                        .event("message");

                    Some(Ok(event))
                } else {
                    None
                }
            }

            Err(_e) => None,
        }
    });

    let stream = DropStream::new(stream, move || {
        for ch in drop_channels {
            state
                .pg_notify
                .send(PgNotifyEvent::ChannelUnsubscribe {
                    client_id,
                    channel: ch.to_string(),
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
) -> StatusCode {
    match state.pg_notify.send(PgNotifyEvent::NotifyMessage {
        channel: request.channel,
        payload: request.payload,
    }) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
