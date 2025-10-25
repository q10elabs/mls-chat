/// MLS Chat Client - Main entry point
/// 
/// A command-line client for MLS group messaging using OpenMLS

use clap::Parser;
use log::info;
use mls_chat_client::{client::MlsClient, Result};

#[derive(Parser)]
#[command(name = "mls-client")]
#[command(about = "MLS Chat Client - Secure group messaging")]
struct Args {
    /// Server URL (default: http://localhost:4000)
    #[arg(long, default_value = "http://localhost:4000")]
    server: String,

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

    // Create client
    let mut client = MlsClient::new(
        &args.server,
        &args.username,
        &args.group_name,
    ).await?;

    // Initialize (load or create identity, register with server)
    client.initialize().await?;

    // Connect to group (create or load existing)
    client.connect_to_group().await?;

    // Run the client
    client.run().await?;

    Ok(())
}
