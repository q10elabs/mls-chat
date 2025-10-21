# API Tests: HTTP Server Integration

## Task Specification

Update the client's API tests to use the actual HTTP server via REST endpoints instead of directly calling the server's database interface. This ensures proper integration testing of the real API contract.

## Requirements

- Use `mls_chat_server::server::create_test_http_server()` to spawn test server instances
- Replace all direct `Database::*` calls with HTTP requests via `reqwest`
- Use the `ServerApi` client (which is already properly implemented)
- Test the following HTTP endpoints:
  - POST `/users` - User registration
  - GET `/users/{username}` - User key retrieval
  - GET `/health` - Health check
- Maintain the same test scenarios but via HTTP API

## Current State

**Problem:** Current tests in `tests/api_tests.rs` bypass the HTTP layer:
- Using `create_test_pool()` directly
- Calling `Database::register_user()` and `Database::get_user()` directly
- Not testing the actual HTTP endpoints

**Solution:** Refactor tests to:
1. Spawn a test server with `create_test_http_server()`
2. Extract the bind address from the returned tuple
3. Create a `ServerApi` client with the address
4. Use `ServerApi` methods to make HTTP requests
5. Verify responses via the actual HTTP API

## Implementation Plan

### Step 1: Update test dependencies
- Ensure `tokio`, `reqwest`, and `mls-chat-server` (as dev-dependency) are in Cargo.toml

### Step 2: Refactor all 5 test cases
- `test_register_new_user` - Register via HTTP POST, verify via HTTP GET
- `test_register_duplicate_user` - Test HTTP 409 Conflict response
- `test_get_user_key` - Verify HTTP GET endpoint response
- `test_get_nonexistent_user` - Test HTTP 404 response
- `test_multiple_users` - Register and verify multiple users via HTTP

### Step 3: Add new test cases
- `test_health_check` - Verify GET `/health` endpoint

## Files Modified

- `client/rust/tests/api_tests.rs` - Complete refactor to use HTTP endpoints

## Decision Rationales

1. **Real API Testing:** HTTP integration tests provide confidence that the `ServerApi` client works correctly with the actual server API contract
2. **Test Isolation:** Each test spawns its own server instance, ensuring test isolation
3. **Error Handling:** Testing actual HTTP status codes (409, 404) rather than database errors
4. **Reuse:** Leverages existing `ServerApi` implementation that client code uses

## Implementation Summary

### What was done

1. **Refactored 5 existing test cases:**
   - `test_register_new_user` - Uses HTTP POST to register, verifies with HTTP GET
   - `test_register_duplicate_user` - Tests HTTP 409 Conflict error handling
   - `test_get_user_key` - Verifies HTTP GET endpoint response
   - `test_get_nonexistent_user` - Tests HTTP 404 Not Found error
   - `test_multiple_users` - Registers and verifies 3 users via HTTP

2. **Added 1 new test case:**
   - `test_health_check` - Verifies GET `/health` endpoint

3. **Replaced database layer calls with HTTP API calls:**
   - Removed: `Database::register_user()`, `Database::get_user()`
   - Added: `mls_chat_server::server::create_test_http_server()` for spawning test server
   - Added: `ServerApi::register_user()`, `ServerApi::get_user_key()`, `ServerApi::health_check()`
   - Added: `tokio::spawn()` to run server in background
   - Added: `tokio::time::sleep()` to allow server to bind

4. **Test server pattern:**
   - Each test spawns its own isolated HTTP server instance
   - Server binds to `127.0.0.1:0` (OS assigns random port)
   - Tests use `ServerApi` client to make actual HTTP requests
   - No database access - all operations go through REST API

### Test Results

✅ All 6 tests pass (5 refactored + 1 new)
✅ Real HTTP integration testing (not bypassing API layer)
✅ Proper error handling for 409 Conflict and 404 Not Found
✅ Test isolation with per-test server instances

## Current Status

✅ Implementation complete
✅ All tests passing
✅ Ready for integration with client implementation
