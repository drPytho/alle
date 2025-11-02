use alle_pg::{Bridge, BridgeConfig, Frontend, auth::AuthConfig};
use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// WebSocket to PostgreSQL NOTIFY/LISTEN bridge with dynamic subscriptions
#[derive(Parser, Debug)]
#[command(name = "alle")]
#[command(about = "Bridge WebSocket clients to PostgreSQL NOTIFY/LISTEN", long_about = None)]
#[command(version)]
#[command(subcommand_required = true, arg_required_else_help = true)]
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

    /// PostgreSQL authentication function name (optional)
    /// If provided, clients must authenticate before subscribing to channels
    /// The function should accept a token and return (user_id TEXT, authenticated BOOLEAN)
    #[arg(long, global = true, env = "AUTH_FUNCTION")]
    auth_function: Option<String>,

    #[command(subcommand)]
    command: Commands,
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
            long = "ws-bind-addr",
            env = "WS_BIND_ADDR",
            default_value = None,
        )]
        ws_bind_addr: Option<String>,

        #[arg(
            long = "sse-bind-addr",
            env = "SSE_BIND_ADDR",
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
        Commands::Serve {
            ws_bind_addr,
            se_bind_addr,
            channels,
        } => {
            // Build frontend configuration based on what's provided
            let mut frontend = Frontend::new();
            if let Some(addr) = ws_bind_addr {
                frontend = frontend.with_websocket(addr);
            }
            if let Some(addr) = se_bind_addr {
                frontend = frontend.with_server_push(addr);
            }

            run_server(args.postgres_url, frontend, channels, args.auth_function).await?;
        }
    }

    Ok(())
}

/// Run the WebSocket bridge server
async fn run_server(
    postgres_url: String,
    frontend: Frontend,
    _channels: Vec<String>,
    auth_function: Option<String>,
) -> Result<()> {
    tracing::info!("Starting Alle WebSocket-Postgres bridge");
    tracing::info!("PostgreSQL: {}", postgres_url);

    if let Some(ref addr) = frontend.websocket {
        tracing::info!("WebSocket: {}", addr);
    }
    if let Some(ref addr) = frontend.server_push {
        tracing::info!("Server-Sent Events: {}", addr);
    }

    if let Some(ref func) = auth_function {
        tracing::info!("Authentication: enabled (function: {})", func);
    } else {
        tracing::info!("Authentication: disabled");
    }

    let mut config = BridgeConfig::new(postgres_url, frontend);
    if let Some(func) = auth_function {
        config = config.with_auth(AuthConfig::new(func));
    }

    let bridge = Bridge::new(config);
    bridge.run().await?;

    Ok(())
}
