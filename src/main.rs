use alle::{Bridge, BridgeConfig, ChannelName, Frontend};
use anyhow::Result;
use clap::{Parser, Subcommand};
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
            default_value = None,
        )]
        ws_bind_addr: Option<String>,

        #[arg(
            short = 's',
            long,
            env = "SE_BIND_ADDR",
            default_value = None,
        )]
        se_bind_addr: Option<String>,
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
            se_bind_addr,
            channels,
        }) => {
            let channels: Vec<String> = channels
                .into_iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            if let Some(bind_addr) = ws_bind_addr {
                run_server(
                    args.postgres_url,
                    Frontend::WebSocket { bind_addr },
                    channels,
                )
                .await?;
            } else if let Some(bind_addr) = se_bind_addr {
                run_server(
                    args.postgres_url,
                    Frontend::ServerPush { bind_addr },
                    channels,
                )
                .await?;
            }
        }

        None => {
            // Default command: start the bridge server with defaults
            run_server(
                args.postgres_url,
                Frontend::ServerPush {
                    bind_addr: "0.0.0.0:8080".into(),
                },
                Vec::new(),
            )
            .await?;
        }
    }

    Ok(())
}

/// Run the WebSocket bridge server
async fn run_server(
    postgres_url: String,
    bind_addr: Frontend,
    channels: Vec<String>,
) -> Result<()> {
    tracing::info!("Starting Alle WebSocket-Postgres bridge");
    tracing::info!("PostgreSQL: {}", postgres_url);
    tracing::info!("WebSocket: {:?}", bind_addr);

    // Convert String channels to ChannelName
    let channel_names: Vec<ChannelName> = channels
        .into_iter()
        .filter_map(|ch| match ChannelName::new(ch) {
            Ok(name) => Some(name),
            Err(e) => {
                tracing::error!("Invalid channel name: {}", e);
                None
            }
        })
        .collect();

    if !channel_names.is_empty() {
        let channel_strs: Vec<String> = channel_names.iter().map(|ch| ch.to_string()).collect();
        tracing::info!("Initial channels: {}", channel_strs.join(", "));
    } else {
        tracing::info!("No initial channels - clients will subscribe dynamically");
    }

    let config = BridgeConfig::new(postgres_url, bind_addr).with_channels(channel_names);
    let bridge = Bridge::new(config);
    bridge.run().await?;

    Ok(())
}
