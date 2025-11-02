use alle_pg::{
    Bridge, BridgeConfig, ChannelName, Frontend, PostgresListener,
    auth::{AuthConfig, Authenticator},
};
use anyhow::{Context, Result};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Test database URL - must have the auth functions from init.sql
fn test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:15432/postgres".to_string())
}

/// Helper to start a test server with SSE and optional auth
/// Returns the server URL and the Bridge instance for shutdown control
async fn start_test_server(with_auth: bool, port: u16) -> Result<(String, Bridge)> {
    let db_url = test_db_url();
    let sse_bind_addr = format!("127.0.0.1:{}", port);

    let frontend = Frontend::new().with_server_push(sse_bind_addr);

    let mut config = BridgeConfig::new(db_url, frontend);
    if with_auth {
        config = config.with_auth(AuthConfig::new("authenticate_user"));
    }

    let bridge = Bridge::new(config);
    let bridge_clone = bridge.clone();

    // Start the bridge in a background task
    tokio::spawn(async move {
        let _ = bridge_clone.run().await;
    });

    // Give the server a moment to start
    sleep(Duration::from_millis(500)).await;

    let server_url = format!("http://127.0.0.1:{}", port);

    Ok((server_url, bridge))
}

/// Helper to create PostgreSQL listener for sending test notifications
async fn create_test_listener() -> Result<Arc<PostgresListener>> {
    let db_url = test_db_url();
    let listener = PostgresListener::connect(&db_url)
        .await
        .with_context(|| format!("failed to connect with {}", db_url))?;
    Ok(Arc::new(listener))
}

#[tokio::test]
#[ignore] // Requires PostgreSQL with auth functions
async fn test_sse_with_valid_token() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(true, 18081).await?;

    // Create HTTP client with valid token
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/events?channels=test_channel", server_url))
        .header(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer valid_token"),
        )
        .send()
        .await?;

    // Should get 200 OK with valid token
    assert_eq!(response.status(), 200);

    // Try to receive some events
    let mut stream = response.bytes_stream().eventsource();

    // Send a test notification
    let listener = create_test_listener().await?;
    let channel = ChannelName::new("test_channel")?;
    listener.notify(&channel, "test_payload").await?;

    // Wait for event (with timeout)
    let timeout_result = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("test_payload") {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Stream ended without receiving message"))
    })
    .await;

    bridge.terminate();
    assert!(
        timeout_result.is_ok(),
        "Should receive notification through SSE"
    );

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL with auth functions
async fn test_sse_with_invalid_token() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(true, 18082).await?;

    // Create HTTP client with invalid token
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/events?channels=test_channel", server_url))
        .header(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer invalid_token"),
        )
        .send()
        .await?;

    // Should get 401 Unauthorized with invalid token
    assert_eq!(response.status(), 401);

    bridge.terminate();

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL with auth functions
async fn test_sse_without_token() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(true, 18083).await?;

    // Create HTTP client without auth header
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/events?channels=test_channel", server_url))
        .send()
        .await?;

    // Should get 401 Unauthorized when auth is required but no token provided
    assert_eq!(response.status(), 401);

    let body = response.text().await?;
    assert!(body.contains("Authentication required") || body.contains("Unauthorized"));

    bridge.terminate();

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_sse_without_auth_enabled() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(false, 18084).await?;

    // Create HTTP client without auth header (auth not enabled on server)
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/events?channels=test_channel", server_url))
        .send()
        .await?;

    // Should get 200 OK when auth is not enabled
    assert_eq!(response.status(), 200);

    bridge.terminate();

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL with auth functions
async fn test_sse_multiple_channels_with_auth() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(true, 18085).await?;

    // Create HTTP client with valid token, subscribing to multiple channels
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "{}/events?channels=channel1,channel2,channel3",
            server_url
        ))
        .header(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer valid_token"),
        )
        .send()
        .await?;

    // Should get 200 OK with valid token
    assert_eq!(response.status(), 200);

    let mut stream = response.bytes_stream().eventsource();

    // Send notifications to different channels
    let listener = create_test_listener().await?;

    let channel1 = ChannelName::new("channel1")?;
    let channel2 = ChannelName::new("channel2")?;

    listener.notify(&channel1, "payload1").await?;
    listener.notify(&channel2, "payload2").await?;

    // Collect events with timeout
    let mut received_payloads = Vec::new();
    let timeout_result = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("payload1") {
                        received_payloads.push("payload1");
                    }
                    if event.data.contains("payload2") {
                        received_payloads.push("payload2");
                    }
                    if received_payloads.len() >= 2 {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Did not receive all expected messages"))
    })
    .await;

    bridge.terminate();
    assert!(
        timeout_result.is_ok(),
        "Should receive notifications from multiple channels"
    );
    assert_eq!(received_payloads.len(), 2, "Should receive both payloads");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL with auth functions
async fn test_authenticator_directly() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let db_url = test_db_url();
    let config = AuthConfig::new("authenticate_user");
    let authenticator = Authenticator::new(&db_url, config)
        .await
        .with_context(|| format!("Failed to create authenticator with {}", db_url))?;

    // Test with valid token
    let result = authenticator.authenticate("valid_token").await?;
    assert!(result.authenticated, "Should authenticate with valid token");
    assert_eq!(result.user_id, "user_123", "Should return correct user_id");

    // Test with invalid token
    let result = authenticator.authenticate("invalid_token").await?;
    assert!(
        !result.authenticated,
        "Should not authenticate with invalid token"
    );

    // Test verify_token method
    let is_valid = authenticator.verify_token("valid_token").await?;
    assert!(is_valid, "verify_token should return true for valid token");

    let is_invalid = authenticator.verify_token("invalid_token").await?;
    assert!(
        !is_invalid,
        "verify_token should return false for invalid token"
    );

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL with auth functions
async fn test_boolean_auth_function() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let db_url = test_db_url();
    let config = AuthConfig::new("verify_token");
    let authenticator = Authenticator::new(&db_url, config).await?;

    // Test with valid token using boolean-only function
    let is_valid = authenticator.verify_token("valid_token").await?;
    assert!(is_valid, "Should authenticate with valid token");

    let is_invalid = authenticator.verify_token("invalid_token").await?;
    assert!(!is_invalid, "Should not authenticate with invalid token");

    Ok(())
}
