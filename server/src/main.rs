/// MLS Chat Server - OpenMLS-based group chat
///
/// Main server entry point. Handles:
/// - Command-line argument parsing
/// - Database initialization
/// - HTTP and WebSocket server startup
mod config;
mod db;
mod handlers;
mod server;

use actix_web::web;
use config::Config;
use handlers::{ServerConfig, WsServer};
use std::fs;
use std::process;
use std::sync::Arc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_default_env()
        .format_timestamp_millis()
        .init();

    let config = Config::from_args();

    log::info!("Starting MLS Chat Server");
    log::info!("Database: {:?}", config.database);
    log::info!("Port: {}", config.port);
    log::info!(
        "KeyPackage reservation timeout: {}s",
        config.reservation_timeout_seconds
    );

    // Write PID file if specified
    if let Some(pidfile) = &config.pidfile {
        let pid = process::id().to_string();
        fs::write(pidfile, pid).expect("Failed to write PID file");
        log::info!("PID file written to: {:?}", pidfile);
    }

    // Initialize database
    let pool =
        db::create_pool(config.database.to_str().unwrap()).expect("Failed to create database pool");

    log::info!("Database initialized");

    let pool_data = web::Data::new(pool.clone());
    let ws_server = web::Data::new(WsServer::new(Arc::new(pool_data.clone())));
    let server_config = web::Data::new(ServerConfig {
        reservation_timeout_seconds: config.reservation_timeout_seconds,
    });

    // Start HTTP server
    let bind_addr = format!("127.0.0.1:{}", config.port);
    log::info!("Starting HTTP server on {}", bind_addr);

    let http_server = server::create_http_server(pool_data, ws_server, server_config, &bind_addr)?;
    http_server.await
}
