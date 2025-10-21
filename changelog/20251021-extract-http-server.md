# Extract HTTP Server Library

## Task Specification
Extract the HTTP server setup code from main.rs into a reusable library function so it can be used in tests from other packages. This allows test suites to easily spin up a test server instance without duplicating the HTTP server configuration logic.

## Status
✅ Complete (with persistence enhancement)

## Implementation Summary

### Files Modified:

1. **server/src/server.rs** (NEW)
   - Created new module with three public functions:
     - `create_http_server(pool, ws_server, bind_addr)` - Takes existing pool and ws_server, returns ready-to-run HttpServer
     - `create_test_http_server_with_pool(pool)` - Creates test server with custom database pool, enabling persistence testing across server instantiations
     - `create_test_http_server()` - Convenience function that creates an in-memory test database, binds to random port, returns (server, addr) tuple
   - HTTP app configuration (routes, middleware) inlined in each function to avoid complex type annotations with middleware

2. **server/src/lib.rs**
   - Added `pub mod server;` to expose the new module

3. **server/src/main.rs**
   - Refactored to use `server::create_http_server()` instead of inline HttpServer setup
   - Removed unnecessary imports (App, HttpServer, handlers re-imports)
   - Cleaner separation of concerns: main handles config/database init, server module handles app construction

### Design Decisions

- **HttpServer at module boundary**: Chose to return `HttpServer` instead of `App` because middleware type wrapping makes `App` return types impossible to express at module boundaries without boxing or losing type info
- **Inline app configuration**: The HTTP app routes and middleware configuration are inlined within the factory functions rather than extracted, avoiding complex type constraints
- **Test convenience function**: `create_test_http_server()` creates its own in-memory database and binds to port 0 (random available port), returning both the server and the actual bind address for easy test usage
- **No breaking changes**: Main.rs behavior is identical, just uses the library function

### Testing
- All 32 library tests pass (21 existing + 11 new server module tests)
- New tests added:
  - `test_create_http_server_with_test_pool` - Verifies server creation with custom pool
  - `test_create_http_server_invalid_address` - Verifies error handling for invalid addresses
  - `test_create_test_http_server_with_pool` - Verifies pool-based test server creation
  - `test_create_test_http_server_with_pool_persistence` - **NEW**: Verifies data persists across multiple server instantiations sharing same pool
  - `test_create_test_http_server` - Verifies convenience test server function
  - `test_create_test_http_server_assigns_random_port` - Verifies random port assignment
  - `test_health_endpoint` - Integration test for /health endpoint
  - `test_register_user_endpoint` - Integration test for user registration
  - `test_get_user_key_endpoint` - Integration test for retrieving user keys
  - `test_get_nonexistent_user_returns_404` - Verifies 404 handling
  - `test_store_and_get_backup_endpoints` - Integration test for backup endpoints
- Main server builds and runs identically to before

### Usage from Other Packages

From client tests, you can now:

```rust
// Option 1: Custom pool/ws_server for full control
let pool = web::Data::new(db::create_pool("test.db")?);
let ws_server = web::Data::new(WsServer::new(Arc::new(pool.clone())));
let server = mls_chat_server::server::create_http_server(pool, ws_server, "127.0.0.1:8080")?;
tokio::spawn(server);

// Option 2: Quick test setup (each call creates new in-memory database)
let (server, addr) = mls_chat_server::server::create_test_http_server()?;
tokio::spawn(server);
let client = reqwest::Client::new();
let resp = client.get(&format!("http://{}/health", addr)).send().await?;

// Option 3: Persistence testing - reuse pool across server restarts
let pool = web::Data::new(crate::db::create_test_pool());

// First server instance
let (server1, addr1) = mls_chat_server::server::create_test_http_server_with_pool(pool.clone())?;
tokio::spawn(server1);
// Make requests, register users, store data...

// Second server instance with same pool - all data persists
let (server2, addr2) = mls_chat_server::server::create_test_http_server_with_pool(pool.clone())?;
tokio::spawn(server2);
// Data from first server is still available in second server
```
