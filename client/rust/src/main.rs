/// MLS Chat Client - Main entry point
///
/// A command-line client for MLS group messaging using OpenMLS
use clap::Parser;
use log::info;
use mls_chat_client::{cli, client::MlsClient, Result};

#[derive(Parser)]
#[command(name = "mls-client")]
#[command(about = "MLS Chat Client - Secure group messaging")]
struct Args {
    /// Server URL (default: http://localhost:4000)
    #[arg(long, default_value = "http://localhost:4000")]
    server: String,

    /// Config directory for state database (default: ~/.mlschat)
    #[arg(long)]
    config: Option<String>,

    /// Group name to join or create
    group_name: String,

    /// Username for this client
    username: String,

    /// Enable verbose logging (DEBUG level)
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logger with appropriate level based on verbose flag
    let log_level = if args.verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    env_logger::Builder::from_default_env()
        .filter_level(log_level)
        .format_timestamp_millis()
        .init();

    info!("Starting MLS Chat Client");
    info!("Server: {}", args.server);
    info!("Group: {}", args.group_name);
    info!("Username: {}", args.username);

    // Determine storage directory
    let storage_dir = if let Some(config_path) = args.config {
        std::path::PathBuf::from(config_path)
    } else {
        // Default to ~/.mlschat
        use directories::BaseDirs;
        let base_dirs = BaseDirs::new().ok_or_else(|| {
            mls_chat_client::error::ClientError::Config("Failed to get home directory".to_string())
        })?;
        base_dirs.home_dir().join(".mlschat")
    };

    info!("Config directory: {}", storage_dir.display());

    // Create client
    let mut client = MlsClient::new_with_storage_path(
        &args.server,
        &args.username,
        &args.group_name,
        &storage_dir,
    )?;

    // Initialize (load or create identity, register with server)
    client.initialize().await?;

    // Connect to group (create or load existing)
    client.connect_to_group().await?;

    // Run the client control loop
    cli::run_client_loop(&mut client).await?;

    Ok(())
}
