use axum::{
    extract::{Path, State},
    response::sse::{Event, Sse},
    routing::get,
    Router,
};
use futures::stream::Stream;
use std::{convert::Infallible, sync::atomic::AtomicU64};
use std::{sync::Arc, time::Duration};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use anyhow::Result;
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
        .route("/events/{channel}", get(sse_handler))
        .with_state(state);

    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn sse_handler(
    Path(channel): Path<String>,
    State(state): State<HttpPushServerState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let client_id = state
        .client_id_counter
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state
        .pg_notify
        .send(PgNotifyEvent::ChannelSubscribe {
            client_id,
            channel: channel.clone(),
        })
        .expect("Should be able to send message");

    // Create guard that will send unsubscribe on drop

    let filter_channel = channel.clone();
    let stream = BroadcastStream::new(state.notify).filter_map(move |v| {
        // Keep guard alive by cloning Arc into closure

        match v {
            Ok(NotificationMessage { channel, payload }) => {
                if channel == filter_channel {
                    let event = Event::default().data(payload).event("message");

                    Some(Ok(event))
                } else {
                    None
                }
            }

            Err(_e) => None,
        }
    });

    let stream = DropStream::new(stream, move || {
        state
            .pg_notify
            .send(PgNotifyEvent::ChannelUnsubscribe { client_id, channel })
            .expect("should not fail to send here");
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keep-alive"),
    )
}
