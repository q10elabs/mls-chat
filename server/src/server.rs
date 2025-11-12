use crate::db::DbPool;
use crate::handlers::{
    get_backup, get_keypackage_status, get_user_key, health, register_user, reserve_key_package,
    spend_key_package, store_backup, upload_key_packages, ws_connect, ServerConfig, WsServer,
};
/// HTTP server factory and configuration.
/// Provides a reusable function to create and configure the HTTP server
/// for use in both the main binary and tests.
use actix_web::{middleware, web, App, HttpServer};
use std::sync::Arc;

/// Create a configured HTTP server
///
/// Takes a database pool, WebSocket server, server config, and bind address,
/// then returns a fully configured `HttpServer` ready to be run.
///
/// # Arguments
/// * `pool` - Database connection pool wrapped in web::Data
/// * `ws_server` - WebSocket server instance wrapped in web::Data
/// * `server_config` - Server configuration (e.g., reservation timeout)
/// * `bind_addr` - Address to bind the server to (e.g., "127.0.0.1:4000")
///
/// # Example
/// ```ignore
/// let pool = web::Data::new(db::create_pool("chatserver.db")?);
/// let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
/// let config = web::Data::new(ServerConfig::default());
/// let server = server::create_http_server(pool, ws_server, config, "127.0.0.1:4000")?;
/// server.run().await?;
/// ```
pub fn create_http_server(
    pool: web::Data<DbPool>,
    ws_server: web::Data<WsServer>,
    server_config: web::Data<ServerConfig>,
    bind_addr: &str,
) -> std::io::Result<actix_web::dev::Server> {
    let pool_clone = pool.clone();
    let ws_server_clone = ws_server.clone();
    let config_clone = server_config.clone();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(pool_clone.clone())
            .app_data(ws_server_clone.clone())
            .app_data(config_clone.clone())
            .wrap(middleware::Logger::default())
            // REST endpoints
            .route("/health", web::get().to(health))
            .route("/users", web::post().to(register_user))
            .route("/users/{username}", web::get().to(get_user_key))
            .route("/backup/{username}", web::post().to(store_backup))
            .route("/backup/{username}", web::get().to(get_backup))
            .route("/keypackages/upload", web::post().to(upload_key_packages))
            .route("/keypackages/reserve", web::post().to(reserve_key_package))
            .route("/keypackages/spend", web::post().to(spend_key_package))
            .route(
                "/keypackages/status/{username}",
                web::get().to(get_keypackage_status),
            )
            // WebSocket endpoint
            .route("/ws/{username}", web::get().to(ws_connect))
    })
    .bind(bind_addr)?
    .run();

    Ok(server)
}

/// Create a test HTTP server with custom database pool
///
/// Allows tests to provide their own database pool, enabling testing of
/// data persistence across multiple server instantiations. The server binds
/// to a random available port.
///
/// # Arguments
/// * `pool` - Database pool to use (can be shared across multiple servers)
///
/// # Returns
/// A tuple of (server, bind_address) where bind_address can be used to make requests
///
/// # Example
/// ```ignore
/// // Create a persistent pool shared across servers
/// let pool = web::Data::new(crate::db::create_test_pool());
///
/// // First server instance
/// let (server1, addr1) = server::create_test_http_server_with_pool(pool.clone())?;
/// tokio::spawn(server1);
///
/// // Make requests to first server...
///
/// // Second server instance with same pool - data persists
/// let (server2, addr2) = server::create_test_http_server_with_pool(pool.clone())?;
/// tokio::spawn(server2);
///
/// // Data from first server is still available in second server
/// ```
pub fn create_test_http_server_with_pool(
    pool: web::Data<DbPool>,
) -> std::io::Result<(actix_web::dev::Server, String)> {
    let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
    let server_config = web::Data::new(ServerConfig::default());

    // Bind to 127.0.0.1:0 to get a random available port
    let bind_addr = "127.0.0.1:0";
    let pool_clone = pool.clone();
    let ws_server_clone = ws_server.clone();
    let config_clone = server_config.clone();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(pool_clone.clone())
            .app_data(ws_server_clone.clone())
            .app_data(config_clone.clone())
            .wrap(middleware::Logger::default())
            // REST endpoints
            .route("/health", web::get().to(health))
            .route("/users", web::post().to(register_user))
            .route("/users/{username}", web::get().to(get_user_key))
            .route("/backup/{username}", web::post().to(store_backup))
            .route("/backup/{username}", web::get().to(get_backup))
            .route("/keypackages/upload", web::post().to(upload_key_packages))
            .route("/keypackages/reserve", web::post().to(reserve_key_package))
            .route("/keypackages/spend", web::post().to(spend_key_package))
            .route(
                "/keypackages/status/{username}",
                web::get().to(get_keypackage_status),
            )
            // WebSocket endpoint
            .route("/ws/{username}", web::get().to(ws_connect))
    })
    .bind(bind_addr)?;

    // Get the actual bind address (including the assigned port)
    let addrs = server.addrs();
    let addr_str = addrs
        .first()
        .ok_or_else(|| std::io::Error::other("No bind address found"))?
        .to_string();

    let server = server.run();

    Ok((server, addr_str))
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
    create_test_http_server_with_pool(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;

    #[tokio::test]
    async fn test_create_http_server_with_test_pool() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
        let server_config = web::Data::new(ServerConfig::default());

        let result = create_http_server(pool, ws_server, server_config, "127.0.0.1:0");
        assert!(result.is_ok(), "create_http_server should succeed");
    }

    #[tokio::test]
    async fn test_create_http_server_invalid_address() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
        let server_config = web::Data::new(ServerConfig::default());

        // Try to bind to an invalid address
        let result = create_http_server(pool, ws_server, server_config, "invalid_address:99999");
        assert!(
            result.is_err(),
            "create_http_server should fail with invalid address"
        );
    }

    #[tokio::test]
    async fn test_create_test_http_server_with_pool() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let result = create_test_http_server_with_pool(pool);
        assert!(
            result.is_ok(),
            "create_test_http_server_with_pool should succeed"
        );

        let (_server, addr) = result.unwrap();
        assert!(
            addr.contains("127.0.0.1:"),
            "Address should contain 127.0.0.1:"
        );
    }

    #[tokio::test]
    async fn test_create_test_http_server_with_pool_persistence() {
        // Create a shared pool
        let pool = web::Data::new(crate::db::create_test_pool());

        // Register a user in the pool
        let key_package = vec![0x19, 0x1a, 0x1b, 0x1c];
        crate::db::Database::register_user(&pool, "persistence_test", &key_package)
            .await
            .expect("Failed to register test user");

        // Create first server with shared pool
        let (server1, _addr1) = create_test_http_server_with_pool(pool.clone())
            .expect("First server creation should succeed");

        // Data is persisted in the pool
        let user = crate::db::Database::get_user(&pool, "persistence_test")
            .await
            .expect("Query should succeed")
            .expect("User should exist");
        assert_eq!(user.username, "persistence_test");

        // Create second server with same pool - data still persists
        let (server2, _addr2) = create_test_http_server_with_pool(pool.clone())
            .expect("Second server creation should succeed");

        // Verify user still exists in pool
        let user_again = crate::db::Database::get_user(&pool, "persistence_test")
            .await
            .expect("Query should succeed")
            .expect("User should still exist");
        assert_eq!(user_again.username, "persistence_test");

        // Drop servers to clean up (they would normally be spawned)
        drop(server1);
        drop(server2);
    }

    #[tokio::test]
    async fn test_create_test_http_server() {
        let result = create_test_http_server();
        assert!(result.is_ok(), "create_test_http_server should succeed");

        let (_server, addr) = result.unwrap();
        // Verify address is in expected format
        assert!(
            addr.contains("127.0.0.1:"),
            "Address should contain 127.0.0.1:"
        );
        // Verify we got a port number
        let port_part = addr.split(':').nth(1).unwrap_or("");
        assert!(!port_part.is_empty(), "Port should be assigned");
    }

    #[tokio::test]
    async fn test_create_test_http_server_assigns_random_port() {
        let (_, addr1) = create_test_http_server().expect("First server creation should succeed");
        let (_, addr2) = create_test_http_server().expect("Second server creation should succeed");

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
        let server_config = web::Data::new(ServerConfig::default());

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .app_data(server_config)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect)),
        )
        .await;

        let req = test::TestRequest::get().uri("/health").to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn test_register_user_endpoint() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
        let server_config = web::Data::new(ServerConfig::default());

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .app_data(server_config)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect)),
        )
        .await;

        // Create a key package as bytes (base64 encoded in JSON)
        let key_package = vec![0x1d, 0x1e, 0x1f, 0x20];
        let req = test::TestRequest::post()
            .uri("/users")
            .set_json(serde_json::json!({
                "username": "alice",
                "key_package": key_package
            }))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 201); // Created
    }

    #[actix_web::test]
    async fn test_get_user_key_endpoint() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
        let server_config = web::Data::new(ServerConfig::default());

        // First register a user
        let key_package = vec![0x21, 0x22, 0x23, 0x24];
        crate::db::Database::register_user(&pool, "bob", &key_package)
            .await
            .expect("Failed to register test user");

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .app_data(server_config)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect)),
        )
        .await;

        let req = test::TestRequest::get().uri("/users/bob").to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn test_get_nonexistent_user_returns_404() {
        let pool = web::Data::new(crate::db::create_test_pool());
        let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
        let server_config = web::Data::new(ServerConfig::default());

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .app_data(server_config)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect)),
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
        let server_config = web::Data::new(ServerConfig::default());

        // First register a user
        let key_package = vec![0x25, 0x26, 0x27, 0x28];
        crate::db::Database::register_user(&pool, "charlie", &key_package)
            .await
            .expect("Failed to register test user");

        let app = test::init_service(
            App::new()
                .app_data(pool)
                .app_data(ws_server)
                .app_data(server_config)
                .wrap(middleware::Logger::default())
                // REST endpoints
                .route("/health", web::get().to(health))
                .route("/users", web::post().to(register_user))
                .route("/users/{username}", web::get().to(get_user_key))
                .route("/backup/{username}", web::post().to(store_backup))
                .route("/backup/{username}", web::get().to(get_backup))
                // WebSocket endpoint
                .route("/ws/{username}", web::get().to(ws_connect)),
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
        let get_req = test::TestRequest::get().uri("/backup/charlie").to_request();

        let get_resp = test::call_service(&app, get_req).await;
        assert!(get_resp.status().is_success());
    }
}
