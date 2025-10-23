use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use futures::stream::Stream;
use std::{convert::Infallible, sync::atomic::AtomicU64};
use std::{sync::Arc, time::Duration};
use tokio_stream::{once, wrappers::ReceiverStream, StreamExt};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::{
    auth::Authenticator, drop_stream::DropStream, ChannelName, NotificationMessage,
    PostgresListener,
};

pub struct HttpPushServerState {
    pg_notify: Arc<PostgresListener>,
    client_id_counter: Arc<AtomicU64>,
    auth: Option<Arc<Authenticator>>,
}

impl Clone for HttpPushServerState {
    fn clone(&self) -> Self {
        HttpPushServerState {
            pg_notify: self.pg_notify.clone(),
            client_id_counter: self.client_id_counter.clone(),
            auth: self.auth.clone(),
        }
    }
}

pub async fn server(
    bind_addr: String,
    pg_notify: Arc<PostgresListener>,
    auth: Option<Arc<Authenticator>>,
) -> Result<()> {
    let listener = TcpListener::bind(bind_addr)
        .await
        .expect("We should be able to bind out server to addr");

    let state = HttpPushServerState {
        pg_notify,
        client_id_counter: Arc::new(AtomicU64::new(0)),
        auth,
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

#[derive(Serialize)]
struct EventMessage {
    channel: ChannelName,
    payload: String,
}

async fn sse_handler(
    channels: Query<Channels>,
    auth_token: Option<TypedHeader<Authorization<Bearer>>>,
    State(state): State<HttpPushServerState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, impl IntoResponse> {
    // If authenticator is configured, verify the token
    if let Some(auth) = &state.auth {
        match auth_token {
            Some(TypedHeader(auth_header)) => match auth.authenticate(auth_header.token()).await {
                Ok(result) => {
                    if !result.authenticated {
                        tracing::warn!("SSE authentication failed");
                        return Err((StatusCode::UNAUTHORIZED, "Authentication failed"));
                    } else {
                        tracing::debug!(
                            "SSE authentication successful for user: {}",
                            result.user_id
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("SSE authentication error: {}", e);
                    return Err((StatusCode::INTERNAL_SERVER_ERROR, "Authentication error"));
                }
            },
            None => {
                tracing::warn!("SSE request missing authentication token");
                return Err((StatusCode::UNAUTHORIZED, "Authentication required"));
            }
        }
    }

    let (sender, receiver) = tokio::sync::mpsc::channel(32);

    tracing::debug!("Request received for channels: {}", &channels.channels);
    let client_id = state
        .client_id_counter
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Parse and validate channel names
    let channels: Vec<ChannelName> = channels
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

    tracing::debug!("Channels extracted: {:?}", &channels);

    // Subscribe to all valid channels
    state
        .pg_notify
        .listen_many(client_id, &channels, sender.clone())
        .await
        .expect("Should be able to send message");

    tracing::debug!("Channels listened to: {:?}", &channels);

    let drop_channels = channels.clone();

    let stream = ReceiverStream::new(receiver).filter_map(
        move |NotificationMessage { channel, payload }| {
            let message = EventMessage { channel, payload };
            let json_data =
                serde_json::to_string(&message).expect("Failed to serialize event message");
            let event = Event::default().data(json_data).event("message");

            Some(Ok(event))
        },
    );

    let ping = once(Ok(Event::DEFAULT_KEEP_ALIVE));

    let stream = ping.chain(stream);

    let stream = DropStream::new(stream, move || async move {
        state
            .pg_notify
            .unlisten_many(client_id, &drop_channels)
            .await
            .expect("Should be able to send message");
    });

    tracing::debug!("Stream generated listened to: {:?}", &channels);
    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keep-alive"),
    ))
}

#[derive(Deserialize)]
struct NotifyRequest {
    channel: ChannelName,
    payload: String,
}

#[derive(Serialize)]
struct NotifyResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn notify_handler(
    State(state): State<HttpPushServerState>,
    Json(request): Json<NotifyRequest>,
) -> Json<NotifyResponse> {
    let channel = match ChannelName::new(request.channel) {
        Ok(val) => val,
        Err(e) => {
            let error_msg = format!("Bad channel name: {}", e);
            tracing::error!("{}", error_msg);
            return Json(NotifyResponse {
                success: false,
                error: Some(error_msg),
            });
        }
    };
    let res = state.pg_notify.notify(&channel, &request.payload).await;
    match res {
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
