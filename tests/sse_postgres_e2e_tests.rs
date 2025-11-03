use alle_pg::{Bridge, BridgeConfig, ChannelName, Frontend};
use anyhow::{Context, Result};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use tokio_postgres::NoTls;

/// Test database URL
fn test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/postgres".to_string())
}

/// Test server host - can be overridden via TEST_SERVER_HOST env var
fn test_server_host() -> String {
    std::env::var("TEST_SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Helper to construct server URL from port
fn test_server_url(port: u16) -> String {
    format!("http://{}:{}", test_server_host(), port)
}

/// Helper to start a test server with SSE
async fn start_test_server(port: u16) -> Result<(String, Bridge)> {
    let db_url = test_db_url();
    let sse_bind_addr = format!("{}:{}", test_server_host(), port);

    let frontend = Frontend::new().with_server_push(sse_bind_addr);
    let config = BridgeConfig::new(db_url, frontend);

    let bridge = Bridge::new(config);
    let bridge_clone = bridge.clone();

    // Start the bridge in a background task
    tokio::spawn(async move {
        let _ = bridge_clone.run().await;
    });

    // Give the server a moment to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    let server_url = test_server_url(port);

    Ok((server_url, bridge))
}

/// Helper to create a direct PostgreSQL client for sending notifications
async fn create_postgres_client() -> Result<tokio_postgres::Client> {
    let db_url = test_db_url();
    let (client, connection) = tokio_postgres::connect(&db_url, NoTls)
        .await
        .context("Failed to connect to PostgreSQL")?;

    // Spawn connection handler
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("PostgreSQL connection error: {}", e);
        }
    });

    Ok(client)
}

/// Helper to send a notification using raw postgres client
async fn send_notification(
    client: &tokio_postgres::Client,
    channel: &ChannelName,
    payload: &str,
) -> Result<()> {
    client
        .execute(
            "SELECT pg_notify($1, $2)",
            &[&channel.quote_identifier(), &payload],
        )
        .await
        .context("Failed to send notification")?;
    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_basic_sse_notification() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19001).await?;
    let pg_client = create_postgres_client().await?;

    // Connect to SSE endpoint
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/sse?channels=basic_test", server_url))
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let mut stream = response.bytes_stream().eventsource();

    // Send notification via postgres crate
    let channel = ChannelName::new("basic_test")?;
    send_notification(&pg_client, &channel, "hello_world").await?;

    // Wait for event
    let result = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("hello_world") {
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
    assert!(result.is_ok(), "Should receive notification through SSE");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_multiple_channels() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19002).await?;
    let pg_client = create_postgres_client().await?;

    // Subscribe to multiple channels
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "{}/sse?channels=channel_a,channel_b,channel_c",
            server_url
        ))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream().eventsource();

    // Send notifications to different channels
    let channel_a = ChannelName::new("channel_a")?;
    let channel_b = ChannelName::new("channel_b")?;
    let channel_c = ChannelName::new("channel_c")?;

    send_notification(&pg_client, &channel_a, "payload_a").await?;
    send_notification(&pg_client, &channel_b, "payload_b").await?;
    send_notification(&pg_client, &channel_c, "payload_c").await?;

    // Collect all three payloads
    let mut received = Vec::new();
    let result = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("payload_a") {
                        received.push("a");
                    }
                    if event.data.contains("payload_b") {
                        received.push("b");
                    }
                    if event.data.contains("payload_c") {
                        received.push("c");
                    }
                    if received.len() >= 3 {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Did not receive all messages"))
    })
    .await;

    bridge.terminate();
    assert!(result.is_ok(), "Should receive all notifications");
    assert_eq!(received.len(), 3);

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_special_characters_in_channel_name() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19003).await?;
    let pg_client = create_postgres_client().await?;

    // Test channel names with uppercase (will be lowercased)
    let channel = ChannelName::new("Test_Channel_123")?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/sse?channels={}", server_url, channel.as_str()))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream().eventsource();

    send_notification(&pg_client, &channel, "test_payload").await?;

    let result = timeout(Duration::from_secs(5), async {
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
        Err(anyhow::anyhow!("Did not receive message"))
    })
    .await;

    bridge.terminate();
    assert!(
        result.is_ok(),
        "Should handle special characters in channel names"
    );

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_unicode_payload() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19004).await?;
    let pg_client = create_postgres_client().await?;

    let channel = ChannelName::new("unicode_test")?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/sse?channels=unicode_test", server_url))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream().eventsource();

    // Send notification with unicode characters
    let unicode_payload = "Hello 世界 🌍 émojis and accénts";
    send_notification(&pg_client, &channel, unicode_payload).await?;

    let result = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("世界") && event.data.contains("🌍") {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Did not receive unicode message"))
    })
    .await;

    bridge.terminate();
    assert!(result.is_ok(), "Should handle unicode in payloads");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_empty_payload() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19005).await?;
    let pg_client = create_postgres_client().await?;

    let channel = ChannelName::new("empty_test")?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/sse?channels=empty_test", server_url))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream().eventsource();

    // Send notification with empty payload
    send_notification(&pg_client, &channel, "").await?;

    let result = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    // Empty payload should still trigger an event
                    if event.data.contains(channel.as_str()) {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Did not receive empty message"))
    })
    .await;

    bridge.terminate();
    assert!(result.is_ok(), "Should handle empty payloads");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_large_payload() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19006).await?;
    let pg_client = create_postgres_client().await?;

    let channel = ChannelName::new("large_test")?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/sse?channels=large_test", server_url))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream().eventsource();

    // PostgreSQL NOTIFY has an 8000 byte limit, so test with something reasonable
    let large_payload = "x".repeat(7000);
    send_notification(&pg_client, &channel, &large_payload).await?;

    let result = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    if event.data.len() > 7000 {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Did not receive large message"))
    })
    .await;

    bridge.terminate();
    assert!(result.is_ok(), "Should handle large payloads");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_json_payload() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19007).await?;
    let pg_client = create_postgres_client().await?;

    let channel = ChannelName::new("json_test")?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/sse?channels=json_test", server_url))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream().eventsource();

    // Send JSON payload
    let json_payload = r#"{"type":"update","data":{"id":123123123,"status":"active"}}"#;
    send_notification(&pg_client, &channel, json_payload).await?;

    let result = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    // This is maybe not the best test, but simply check we get something back.
                    if event.data.contains("123123123") {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Did not receive JSON message"))
    })
    .await;

    bridge.terminate();
    assert!(
        result.is_ok(),
        "Should handle JSON payloads {:?}",
        result.err().unwrap()
    );

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_rapid_notifications() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19008).await?;
    let pg_client = create_postgres_client().await?;

    let channel = ChannelName::new("rapid_test")?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/sse?channels=rapid_test", server_url))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream().eventsource();

    // Send multiple rapid notifications
    let num_messages = 10;
    for i in 0..num_messages {
        send_notification(&pg_client, &channel, &format!("msg_{}", i)).await?;
    }

    // Collect all messages
    let mut received_count = 0;
    let result = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("msg_") {
                        received_count += 1;
                        if received_count >= num_messages {
                            return Ok::<_, anyhow::Error>(());
                        }
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!(
            "Did not receive all messages (got {})",
            received_count
        ))
    })
    .await;

    bridge.terminate();
    assert!(
        result.is_ok(),
        "Should handle rapid notifications: {:?}",
        result
    );

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_multiple_sse_clients_same_channel() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19009).await?;
    let pg_client = create_postgres_client().await?;

    let channel = ChannelName::new("broadcast_test")?;

    // Connect two SSE clients to the same channel
    let client = reqwest::Client::new();
    let response1 = client
        .get(format!("{}/sse?channels=broadcast_test", server_url))
        .send()
        .await?;
    let response2 = client
        .get(format!("{}/sse?channels=broadcast_test", server_url))
        .send()
        .await?;

    assert_eq!(response1.status(), 200);
    assert_eq!(response2.status(), 200);

    let mut stream1 = response1.bytes_stream().eventsource();
    let mut stream2 = response2.bytes_stream().eventsource();

    // Send one notification
    send_notification(&pg_client, &channel, "broadcast_message").await?;

    // Both clients should receive it
    let result1 = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream1.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("broadcast_message") {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Client 1 did not receive message"))
    });

    let result2 = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream2.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("broadcast_message") {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Client 2 did not receive message"))
    });

    let (r1, r2) = tokio::join!(result1, result2);

    bridge.terminate();
    assert!(r1.is_ok(), "Client 1 should receive notification");
    assert!(r2.is_ok(), "Client 2 should receive notification");

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_channel_isolation() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19010).await?;
    let pg_client = create_postgres_client().await?;

    // Client 1 subscribes to channel_x
    // Client 2 subscribes to channel_y
    let client = reqwest::Client::new();
    let response1 = client
        .get(format!("{}/sse?channels=channel_x", server_url))
        .send()
        .await?;
    let response2 = client
        .get(format!("{}/sse?channels=channel_y", server_url))
        .send()
        .await?;

    assert_eq!(response1.status(), 200);
    assert_eq!(response2.status(), 200);

    let mut stream1 = response1.bytes_stream().eventsource();
    let mut stream2 = response2.bytes_stream().eventsource();

    // Send notification only to channel_x
    let channel_x = ChannelName::new("channel_x")?;
    send_notification(&pg_client, &channel_x, "only_for_x").await?;

    // Client 1 should receive, Client 2 should not
    let result1 = timeout(Duration::from_secs(3), async {
        while let Some(event) = stream1.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("only_for_x") {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Client 1 did not receive message"))
    });

    let result2 = timeout(Duration::from_millis(1000), async {
        while let Some(event) = stream2.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("only_for_x") {
                        return Err(anyhow::anyhow!(
                            "Client 2 should not receive message for channel_x"
                        ));
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Ok::<_, anyhow::Error>(()) // Timeout is expected (no message received)
    });

    let (r1, r2) = tokio::join!(result1, result2);

    bridge.terminate();
    assert!(
        r1.is_ok(),
        "Client 1 should receive notification for its channel"
    );
    assert!(
        r2.is_err(),
        "Client 2 should timeout (not receive notification for different channel)"
    );

    Ok(())
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_payload_with_quotes_and_escapes() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let (server_url, bridge) = start_test_server(19011).await?;
    let pg_client = create_postgres_client().await?;

    let channel = ChannelName::new("escape_test")?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/sse?channels=escape_test", server_url))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream().eventsource();

    // Send payload with quotes, backslashes, and special characters
    let special_payload = r#"{"message":"He said \"Hello\\World\"\nNew line\tTab"}"#;
    send_notification(&pg_client, &channel, special_payload).await?;

    let result = timeout(Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    if event.data.contains("Hello") && event.data.contains("World") {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
            }
        }
        Err(anyhow::anyhow!("Did not receive escaped message"))
    })
    .await;

    bridge.terminate();
    assert!(
        result.is_ok(),
        "Should handle escaped characters in payloads"
    );

    Ok(())
}
