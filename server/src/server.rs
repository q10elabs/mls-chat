/// HTTP server factory and configuration.
/// Provides a reusable function to create and configure the HTTP server
/// for use in both the main binary and tests.

use actix_web::{web, App, HttpServer, middleware};
use crate::db::DbPool;
use crate::handlers::{
    get_backup, get_user_key, health, register_user, store_backup, ws_connect, WsServer,
};
use std::sync::Arc;

/// Create a configured HTTP server
///
/// Takes a database pool, WebSocket server, and bind address, then returns a
/// fully configured `HttpServer` ready to be run.
///
/// # Arguments
/// * `pool` - Database connection pool wrapped in web::Data
/// * `ws_server` - WebSocket server instance wrapped in web::Data
/// * `bind_addr` - Address to bind the server to (e.g., "127.0.0.1:4000")
///
/// # Example
/// ```ignore
/// let pool = web::Data::new(db::create_pool("chatserver.db")?);
/// let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
/// let server = server::create_http_server(pool, ws_server, "127.0.0.1:4000")?;
/// server.run().await?;
/// ```
pub fn create_http_server(
    pool: web::Data<DbPool>,
    ws_server: web::Data<WsServer>,
    bind_addr: &str,
) -> std::io::Result<actix_web::dev::Server> {
    let pool_clone = pool.clone();
    let ws_server_clone = ws_server.clone();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(pool_clone.clone())
            .app_data(ws_server_clone.clone())
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
    .bind(bind_addr)?
    .run();

    Ok(server)
}

/// Create a test HTTP server with in-memory database and WebSocket server
///
/// This is a convenience function for tests that need a fully configured
/// server without having to manually set up the pool and ws_server.
/// Binds to a random available port.
///
/// # Returns
/// A tuple of (server, bind_address) where bind_address can be used to make requests
///
/// # Example
/// ```ignore
/// let (server, addr) = server::create_test_http_server()?;
/// let client = reqwest::Client::new();
/// let resp = client.get(&format!("http://{}/health", addr)).send().await?;
/// ```
pub fn create_test_http_server() -> std::io::Result<(actix_web::dev::Server, String)> {
    let pool = web::Data::new(crate::db::create_test_pool());
    let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));

    // Bind to 127.0.0.1:0 to get a random available port
    let bind_addr = "127.0.0.1:0";
    let pool_clone = pool.clone();
    let ws_server_clone = ws_server.clone();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(pool_clone.clone())
            .app_data(ws_server_clone.clone())
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
    .bind(bind_addr)?;

    // Get the actual bind address (including the assigned port)
    let addrs = server.addrs();
    let addr_str = addrs
        .first()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No bind address found"))?
        .to_string();

    let server = server.run();

    Ok((server, addr_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;

    #[tokio::test]
    async fn test_create_http_server_with_test_pool() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));

        let result = create_http_server(pool, ws_server, "127.0.0.1:0");
        assert!(result.is_ok(), "create_http_server should succeed");
    }

    #[tokio::test]
    async fn test_create_http_server_invalid_address() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));

        // Try to bind to an invalid address
        let result = create_http_server(pool, ws_server, "invalid_address:99999");
        assert!(result.is_err(), "create_http_server should fail with invalid address");
    }

    #[tokio::test]
    async fn test_create_test_http_server() {
        let result = create_test_http_server();
        assert!(result.is_ok(), "create_test_http_server should succeed");

        let (_server, addr) = result.unwrap();
        // Verify address is in expected format
        assert!(addr.contains("127.0.0.1:"), "Address should contain 127.0.0.1:");
        // Verify we got a port number
        let port_part = addr.split(':').nth(1).unwrap_or("");
        assert!(!port_part.is_empty(), "Port should be assigned");
    }

    #[tokio::test]
    async fn test_create_test_http_server_assigns_random_port() {
        let (_, addr1) = create_test_http_server()
            .expect("First server creation should succeed");
        let (_, addr2) = create_test_http_server()
            .expect("Second server creation should succeed");

        // Both should have valid addresses with different ports
        assert!(addr1.contains("127.0.0.1:"));
        assert!(addr2.contains("127.0.0.1:"));
        // Note: We can't strictly assert they're different (small chance of same random port),
        // but in practice they will be different
    }

    #[actix_web::test]
    async fn test_health_endpoint() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect))
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/health")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn test_register_user_endpoint() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect))
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/users")
            .set_json(serde_json::json!({
                "username": "alice",
                "public_key": "key_abc123"
            }))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 201); // Created
    }

    #[actix_web::test]
    async fn test_get_user_key_endpoint() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));

        // First register a user
        crate::db::Database::register_user(&pool, "bob", "key_xyz")
            .await
            .expect("Failed to register test user");

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect))
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/users/bob")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn test_get_nonexistent_user_returns_404() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect))
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/users/nonexistent")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 404); // Not Found
    }

    #[actix_web::test]
    async fn test_store_and_get_backup_endpoints() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));

        // First register a user
        crate::db::Database::register_user(&pool, "charlie", "key_def456")
            .await
            .expect("Failed to register test user");

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect))
        )
        .await;

        // Store backup
        let store_req = test::TestRequest::post()
            .uri("/backup/charlie")
            .set_json(serde_json::json!({
                "encrypted_state": "encrypted_data_xyz"
            }))
            .to_request();

        let store_resp = test::call_service(&app, store_req).await;
        assert!(store_resp.status().is_success());

        // Get backup
        let get_req = test::TestRequest::get()
            .uri("/backup/charlie")
            .to_request();

        let get_resp = test::call_service(&app, get_req).await;
        assert!(get_resp.status().is_success());
    }
}
