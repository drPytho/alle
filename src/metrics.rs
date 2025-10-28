#[cfg(feature = "metrics-export")]
mod export {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
    use std::collections::HashMap;
    use std::sync::OnceLock;

    static SNAPSHOTTER: OnceLock<Snapshotter> = OnceLock::new();

    /// Install the debugging recorder for capturing metrics
    pub fn install_recorder() {
        // Check if already initialized
        if SNAPSHOTTER.get().is_some() {
            return;
        }

        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();

        // Install the recorder as the global metrics recorder
        // This can only succeed once, which is fine
        if metrics::set_global_recorder(recorder).is_ok() {
            // Store the snapshotter for later querying
            SNAPSHOTTER.set(snapshotter).ok();
        }
    }

    /// Get a snapshot of all current metric values
    pub fn get_snapshot() -> HashMap<String, f64> {
        let Some(snapshotter) = SNAPSHOTTER.get() else {
            tracing::warn!("Metrics snapshotter not initialized, returning empty metrics");
            return HashMap::new();
        };

        let snapshot = snapshotter.snapshot();
        let mut result = HashMap::new();

        // The snapshot returns a HashMap where each entry is:
        // (CompositeKey, (Option<Unit>, Option<Description>, DebugValue))
        for (composite_key, (_unit, _desc, debug_value)) in snapshot.into_hashmap() {
            let key = composite_key.key();
            let metric_name = format_metric_name(key);

            let value = match debug_value {
                DebugValue::Counter(v) => v as f64,
                DebugValue::Gauge(v) => v.into_inner(),
                DebugValue::Histogram(values) => {
                    // For histograms, return the count of samples
                    values.len() as f64
                }
            };
            result.insert(metric_name, value);
        }

        result
    }

    /// Format a metric key into a string with labels
    fn format_metric_name(key: &metrics::Key) -> String {
        let name = key.name();
        let labels: Vec<_> = key.labels().collect();

        if labels.is_empty() {
            name.to_string()
        } else {
            let label_parts: Vec<String> = labels
                .iter()
                .map(|label| format!("{}={}", label.key(), label.value()))
                .collect();
            format!("{}{{{}}}", name, label_parts.join(","))
        }
    }
}

/// Install the debugging recorder for capturing metrics
/// This should be called once at application startup
/// Safe to call multiple times - only the first call takes effect
///
/// Note: This function is only available when the `metrics-export` feature is enabled
pub fn install_recorder() {
    #[cfg(feature = "metrics-export")]
    export::install_recorder();
}

/// Get a snapshot of all current metric values
/// Returns a HashMap with metric names as keys and their current values as f64
///
/// Note: This function is only available when the `metrics-export` feature is enabled
#[cfg(feature = "metrics-export")]
pub fn get_snapshot() -> std::collections::HashMap<String, f64> {
    export::get_snapshot()
}

/// Connection metrics
pub mod connections {
    use metrics::{counter, gauge};

    /// Increment when a new WebSocket connection is established
    pub fn ws_connected() {
        counter!("alle_websocket_connections_total").increment(1);
        gauge!("alle_websocket_connections_active").increment(1.0);
    }

    /// Increment when a WebSocket connection is closed
    pub fn ws_disconnected() {
        gauge!("alle_websocket_connections_active").decrement(1.0);
    }

    /// Increment when a new SSE connection is established
    pub fn sse_connected() {
        counter!("alle_sse_connections_total").increment(1);
        gauge!("alle_sse_connections_active").increment(1.0);
    }

    /// Increment when an SSE connection is closed
    pub fn sse_disconnected() {
        gauge!("alle_sse_connections_active").decrement(1.0);
    }
}

/// Channel subscription metrics
pub mod channels {
    use metrics::{counter, gauge};

    /// Called when a client subscribes to a channel
    pub fn subscribed(channel: &str) {
        counter!("alle_channel_subscriptions_total", "channel" => channel.to_string()).increment(1);
        gauge!("alle_channel_subscribers", "channel" => channel.to_string()).increment(1.0);
    }

    /// Called when a client unsubscribes from a channel
    pub fn unsubscribed(channel: &str) {
        counter!("alle_channel_unsubscriptions_total", "channel" => channel.to_string())
            .increment(1);
        gauge!("alle_channel_subscribers", "channel" => channel.to_string()).decrement(1.0);
    }

    /// Called when Postgres LISTEN is executed for a channel
    pub fn postgres_listen(channel: &str) {
        counter!("alle_postgres_listen_total", "channel" => channel.to_string()).increment(1);
    }

    /// Called when Postgres UNLISTEN is executed for a channel
    pub fn postgres_unlisten(channel: &str) {
        counter!("alle_postgres_unlisten_total", "channel" => channel.to_string()).increment(1);
    }
}

/// Message flow metrics
pub mod messages {
    use metrics::{counter, histogram};

    /// Called when a notification is received from Postgres
    pub fn received_from_postgres(channel: &str) {
        counter!("alle_messages_received_from_postgres_total", "channel" => channel.to_string())
            .increment(1);
    }

    /// Called when a notification is sent to a WebSocket client
    pub fn sent_to_ws_client(channel: &str) {
        counter!("alle_messages_sent_to_ws_clients_total", "channel" => channel.to_string())
            .increment(1);
    }

    /// Called when a notification is sent to an SSE client
    pub fn sent_to_sse_client(channel: &str) {
        counter!("alle_messages_sent_to_sse_clients_total", "channel" => channel.to_string())
            .increment(1);
    }

    /// Called when a client sends a NOTIFY to Postgres
    pub fn sent_to_postgres(channel: &str) {
        counter!("alle_messages_sent_to_postgres_total", "channel" => channel.to_string())
            .increment(1);
    }

    /// Track payload size in bytes
    pub fn payload_size(bytes: usize, direction: &str) {
        histogram!("alle_message_payload_bytes", "direction" => direction.to_string())
            .record(bytes as f64);
    }

    /// Called when a message fails to send to a client (dead connection)
    pub fn send_failed(reason: &str) {
        counter!("alle_message_send_failures_total", "reason" => reason.to_string()).increment(1);
    }
}

/// Authentication metrics
pub mod auth {
    use metrics::counter;

    /// Called when authentication succeeds
    pub fn success() {
        counter!("alle_auth_attempts_total", "result" => "success").increment(1);
    }

    /// Called when authentication fails
    pub fn failure() {
        counter!("alle_auth_attempts_total", "result" => "failure").increment(1);
    }

    /// Called when authentication encounters an error
    pub fn error() {
        counter!("alle_auth_attempts_total", "result" => "error").increment(1);
    }

    /// Called when an unauthenticated request is rejected
    pub fn rejected() {
        counter!("alle_auth_rejections_total").increment(1);
    }
}

/// Error metrics
pub mod errors {
    use metrics::counter;

    /// Generic error counter with error type label
    pub fn occurred(error_type: &str, operation: &str) {
        counter!("alle_errors_total",
            "error_type" => error_type.to_string(),
            "operation" => operation.to_string()
        )
        .increment(1);
    }

    /// Postgres connection errors
    pub fn postgres_connection() {
        occurred("postgres_connection", "connect");
    }

    /// Channel name validation errors
    pub fn invalid_channel_name() {
        occurred("invalid_channel_name", "validate");
    }

    /// JSON parsing errors
    pub fn json_parse() {
        occurred("json_parse", "deserialize");
    }
}
