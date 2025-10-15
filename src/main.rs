use alle::{Bridge, BridgeConfig, ClientMessage, ServerMessage};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// WebSocket to PostgreSQL NOTIFY/LISTEN bridge with dynamic subscriptions
#[derive(Parser, Debug)]
#[command(name = "alle")]
#[command(about = "Bridge WebSocket clients to PostgreSQL NOTIFY/LISTEN", long_about = None)]
#[command(version)]
struct Args {
    /// PostgreSQL connection string
    #[arg(
        short = 'p',
        long,
        global = true,
        env = "POSTGRES_URL",
        default_value = "postgresql://localhost/postgres"
    )]
    postgres_url: String,

    /// Log level filter
    #[arg(
        short = 'l',
        long,
        global = true,
        env = "RUST_LOG",
        default_value = "alle=info,info"
    )]
    log_level: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the WebSocket bridge server (default)
    Serve {
        /// Comma-separated list of channels to listen on initially (optional)
        /// Clients can dynamically subscribe to any channel at runtime
        #[arg(
            short = 'c',
            long,
            env = "LISTEN_CHANNELS",
            value_delimiter = ',',
            default_value = ""
        )]
        channels: Vec<String>,

        /// WebSocket server URL
        #[arg(
            short = 'w',
            long,
            env = "WS_BIND_ADDR",
            default_value = "127.0.0.1:8080"
        )]
        ws_bind_addr: String,
    },

    /// Listen to channels via WebSocket connection
    Listen {
        /// Channels to listen on (optional - use REPL to subscribe interactively)
        #[arg(value_delimiter = ',')]
        channels: Vec<String>,
        /// WebSocket server URL
        #[arg(
            short = 'w',
            long,
            env = "WS_URL",
            default_value = "ws://127.0.0.1:8080"
        )]
        ws_url: String,
    },

    /// Publish a notification via WebSocket connection
    Publish {
        /// Channel to publish to
        #[arg(required = true)]
        channel: String,

        /// Message payload to send
        #[arg(required = true)]
        payload: String,
        /// WebSocket server URL
        #[arg(
            short = 'w',
            long,
            env = "WS_URL",
            default_value = "ws://127.0.0.1:8080"
        )]
        ws_url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing with specified log level
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    match args.command {
        Some(Commands::Serve {
            ws_bind_addr,
            channels,
        }) => {
            let channels: Vec<String> = channels
                .into_iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            run_server(args.postgres_url, ws_bind_addr, channels).await?;
        }

        Some(Commands::Listen { ws_url, channels }) => {
            run_listen(ws_url, channels).await?;
        }

        Some(Commands::Publish {
            ws_url,
            channel,
            payload,
        }) => {
            run_publish(ws_url, channel, payload).await?;
        }

        None => {
            // Default command: start the bridge server with defaults
        }
    }

    Ok(())
}

/// Run the WebSocket bridge server
async fn run_server(
    postgres_url: String,
    ws_bind_addr: String,
    channels: Vec<String>,
) -> Result<()> {
    tracing::info!("Starting Alle WebSocket-Postgres bridge");
    tracing::info!("PostgreSQL: {}", postgres_url);
    tracing::info!("WebSocket: {}", ws_bind_addr);
    if !channels.is_empty() {
        tracing::info!("Initial channels: {}", channels.join(", "));
    } else {
        tracing::info!("No initial channels - clients will subscribe dynamically");
    }

    let config = BridgeConfig::new(postgres_url, ws_bind_addr).with_channels(channels);
    let bridge = Bridge::new(config);
    bridge.run().await?;

    Ok(())
}

/// Listen to channels via WebSocket connection
async fn run_listen(ws_url: String, channels: Vec<String>) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    println!("Connecting to WebSocket: {}", ws_url);

    if channels.is_empty() {
        println!("Starting with no initial subscriptions.");
    } else {
        println!("Initial channels: {}", channels.join(", "));
    }

    println!("\nCommands:");
    println!("  sub <channel>    - Subscribe to a channel");
    println!("  unsub <channel>  - Unsubscribe from a channel");
    println!("  list             - List subscribed channels");
    println!("  help             - Show this help");
    println!("  quit / exit      - Exit the program");
    println!("\nPress Ctrl+C to stop\n");

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .context("Failed to connect to WebSocket server")?;

    let (mut write, mut read) = ws_stream.split();

    // Track subscribed channels
    let mut subscribed_channels = std::collections::HashSet::new();

    // Subscribe to all specified channels
    for channel in &channels {
        let msg = ClientMessage::Subscribe {
            channel: channel.clone(),
        };
        let json = serde_json::to_string(&msg)?;
        write.send(Message::Text(json)).await?;
        println!("✓ Subscribing to channel '{}'", channel);
        subscribed_channels.insert(channel.clone());
    }

    if channels.is_empty() {
        println!("Type 'sub <channel>' to subscribe to channels");
    } else {
        println!("\nWaiting for notifications...");
    }
    println!();

    // Create stdin reader
    let stdin = tokio::io::stdin();
    let mut stdin_reader = BufReader::new(stdin).lines();

    loop {
        tokio::select! {
            // Handle WebSocket messages
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ServerMessage>(&text) {
                            Ok(ServerMessage::Notification { channel, payload }) => {
                                println!("[{}] {}", channel, payload);
                            }
                            Ok(ServerMessage::Subscribed { channel }) => {
                                subscribed_channels.insert(channel.clone());
                                println!("✓ Subscribed to channel '{}'", channel);
                            }
                            Ok(ServerMessage::Unsubscribed { channel }) => {
                                subscribed_channels.remove(&channel);
                                println!("✓ Unsubscribed from channel '{}'", channel);
                            }
                            Ok(ServerMessage::Error { message }) => {
                                eprintln!("Error: {}", message);
                            }
                            Err(e) => {
                                eprintln!("Failed to parse message: {}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        println!("Connection closed by server");
                        break;
                    }
                    Some(Err(e)) => {
                        eprintln!("WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        break;
                    }
                    _ => {}
                }
            }

            // Handle stdin input
            line = stdin_reader.next_line() => {
                match line {
                    Ok(Some(input)) => {
                        let input = input.trim();
                        if input.is_empty() {
                            continue;
                        }

                        let parts: Vec<&str> = input.split_whitespace().collect();
                        match parts.first().copied() {
                            Some("sub") | Some("subscribe") => {
                                if let Some(channel) = parts.get(1) {
                                    let msg = ClientMessage::Subscribe {
                                        channel: channel.to_string(),
                                    };
                                    let json = serde_json::to_string(&msg)?;
                                    write.send(Message::Text(json)).await?;
                                } else {
                                    println!("Usage: sub <channel>");
                                }
                            }
                            Some("unsub") | Some("unsubscribe") => {
                                if let Some(channel) = parts.get(1) {
                                    let msg = ClientMessage::Unsubscribe {
                                        channel: channel.to_string(),
                                    };
                                    let json = serde_json::to_string(&msg)?;
                                    write.send(Message::Text(json)).await?;
                                } else {
                                    println!("Usage: unsub <channel>");
                                }
                            }
                            Some("list") => {
                                if subscribed_channels.is_empty() {
                                    println!("No active subscriptions");
                                } else {
                                    println!("Subscribed channels:");
                                    let mut channels: Vec<_> = subscribed_channels.iter().collect();
                                    channels.sort();
                                    for channel in channels {
                                        println!("  - {}", channel);
                                    }
                                }
                            }
                            Some("help") => {
                                println!("\nCommands:");
                                println!("  sub <channel>    - Subscribe to a channel");
                                println!("  unsub <channel>  - Unsubscribe from a channel");
                                println!("  list             - List subscribed channels");
                                println!("  help             - Show this help");
                                println!("  quit / exit      - Exit the program");
                                println!();
                            }
                            Some("quit") | Some("exit") => {
                                println!("Exiting...");
                                break;
                            }
                            Some(cmd) => {
                                println!("Unknown command: {}. Type 'help' for available commands.", cmd);
                            }
                            None => {}
                        }
                    }
                    Ok(None) => {
                        // stdin closed
                        break;
                    }
                    Err(e) => {
                        eprintln!("Error reading input: {}", e);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Publish a notification via WebSocket connection
async fn run_publish(ws_url: String, channel: String, payload: String) -> Result<()> {
    println!("Connecting to WebSocket: {}", ws_url);

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .context("Failed to connect to WebSocket server")?;

    let (mut write, _read) = ws_stream.split();

    println!("Publishing to channel '{}': {}", channel, payload);

    let msg = ClientMessage::Notify {
        channel: channel.clone(),
        payload: payload.clone(),
    };
    let json = serde_json::to_string(&msg)?;
    write.send(Message::Text(json)).await?;

    // Give it a moment to send
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    println!("✓ Notification sent successfully");

    Ok(())
}
