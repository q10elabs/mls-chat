# Test Failures Analysis - 2025-11-12

## Task Specification
Run and analyze cargo tests for the Rust client (`cargo test --manifest-path client/rust/Cargo.toml`).

## Test Results Summary
- **Total tests**: 71
- **Passed**: 68
- **Failed**: 3

## Failing Tests
All 3 failures are in `mls::connection::tests`:
1. `test_process_welcome_message_creates_membership`
2. `test_process_application_message_routes_to_membership`
3. `test_process_commit_message_routes_to_membership`

All panics occur at the same assertion point:
```
assert!(result.is_ok(), "Welcome processing should succeed")
```

## Root Cause Analysis

### The Issue
The failing tests all call `process_incoming_envelope()` with a `WelcomeMessage`, which triggers this code path in `src/mls/connection.rs:406-428`:

```rust
let membership = MlsMembership::from_welcome_message(...)?;

// Subscribe to group for receiving messages
let group_id = membership.get_group_id().to_vec();
self.subscribe_to_group(&group_id).await?;  // ← FAILURE POINT
```

The `subscribe_to_group()` method at line 336-346 attempts to use a WebSocket connection:

```rust
pub async fn subscribe_to_group(&mut self, group_id: &[u8]) -> Result<()> {
    let group_id_b64 = general_purpose::STANDARD.encode(group_id);
    log::debug!("Subscribing to group: {}", group_id_b64);

    if let Some(websocket) = &self.websocket {
        websocket.subscribe_to_group(&group_id_b64).await?;
        Ok(())
    } else {
        Err(ClientError::Config("WebSocket not connected".to_string()))
    }
}
```

### Why Tests Fail
The unit tests create `MlsConnection` instances without:
1. Establishing a WebSocket connection
2. Mocking or providing a WebSocket client

When `process_incoming_envelope()` calls `subscribe_to_group()`, it tries to access `self.websocket` which is `None`, causing the error:
```
ClientError::Config("WebSocket not connected".to_string())
```

This error is returned from `from_welcome_message()` (via the `?` operator), causing the assertion to fail.

## Files Involved
- `src/mls/connection.rs` - Contains `process_incoming_envelope()` which calls `subscribe_to_group()`
- `src/mls/membership.rs` - Contains `from_welcome_message()` which processes Welcome messages
- Tests in `src/mls/connection.rs:1010-1070, 1078-1250+`

## Solution Implemented: Mock WebSocket Injection

### Implementation Details

1. **MockWebSocket Implementation** (`src/websocket.rs`):
   - Added `MessageHandler::new_mock()` method that creates a mock WebSocket handler
   - The mock uses in-memory `mpsc` channels instead of network connections
   - Channels are kept alive using a spawned task that forwards outgoing messages to the receive channel
   - Works seamlessly with existing `subscribe_to_group()` and other WebSocket methods

2. **Test Injection Method** (`src/mls/connection.rs`):
   - Added `set_websocket()` test-only method to inject mock WebSocket into `MlsConnection`
   - Marked with `#[cfg(test)]` to only be available in test builds
   - Updated three failing tests to call `bob_connection.set_websocket(MessageHandler::new_mock())` after initialization

3. **Modified Tests**:
   - `test_process_welcome_message_creates_membership` (line 1023)
   - `test_process_application_message_routes_to_membership` (line 1115)
   - `test_process_commit_message_routes_to_membership` (line 1218)

### Test Results
- **Before**: 68 passed, 3 failed
- **After**: 71 passed, 0 failed ✅
- All unit tests pass without breaking any existing functionality

### Design Benefits
- **Non-intrusive**: No changes to production code logic
- **Type-safe**: Uses real `MessageHandler` type with real method signatures
- **Realistic**: Mock still exercises the actual message sending/receiving channels
- **Testable**: Tests now can verify Welcome/Commit message handling without network
