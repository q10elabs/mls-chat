/// MLS Chat Server - OpenMLS-based group chat
///
/// Main server entry point. Handles:
/// - Command-line argument parsing
/// - Database initialization
/// - HTTP and WebSocket server setup
/// - Request routing

mod config;
mod db;
mod handlers;

use actix_web::{web, App, HttpServer, middleware};
use config::Config;
use handlers::{
    get_backup, get_user_key, health, register_user, store_backup, ws_connect, WsServer,
};
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

    // Initialize database
    let pool = db::create_pool(config.database.to_str().unwrap()).expect("Failed to create database pool");

    log::info!("Database initialized");

    let pool_data = web::Data::new(pool.clone());
    let ws_server = web::Data::new(WsServer::new(Arc::new(pool_data)));

    // Start HTTP server
    let bind_addr = format!("127.0.0.1:{}", config.port);
    log::info!("Starting HTTP server on {}", bind_addr);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(ws_server.clone())
            .wrap(middleware::Logger::default())
            // REST endpoints
            .route("/health", web::get().to(health))
            .route("/users", web::post().to(register_user))
            .route("/users/{username}", web::get().to(get_user_key))
            .route("/backup/{username}", web::post().to(store_backup))
            .route("/backup/{username}", web::get().to(get_backup))
            // WebSocket endpoint
            .route("/ws/{username}", web::get().to(ws_connect))
    })
    .bind(&bind_addr)?
    .run()
    .await
}
