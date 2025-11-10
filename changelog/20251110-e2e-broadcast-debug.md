# E2E Test Broadcasting Debug

## Task Specification
Investigate failing end-to-end tests where bidirectional messaging is broken. Specifically:
- Alice can send a message to Bob and Bob receives it successfully
- When Bob sends a message back to Alice, Alice does not receive it

Goal: Verify the server broadcasts messages in both directions properly before investigating client-side issues.

## Investigation Progress

### Initial Observations
- Test file: `client/rust/e2e_tests/test_welcome_routing.expect`
- Issue appears to be unidirectional message flow (Alice → Bob works, Bob → Alice fails)
- User suspects server broadcasting may not be working in both directions

### Root Cause Found ✓
**Alice doesn't subscribe to the group after creating it.** The client creates a group membership but never sends the WebSocket subscription message to the server.

### Detailed Analysis

#### Client Code Flow
1. `client.rs:connect_to_group()` (line 169):
   - Connects WebSocket via `connection.connect_websocket()`
   - Creates/loads membership via `MlsMembership::create_new_group()`
   - Stores membership via `connection.add_membership(membership)`
   - **MISSING**: No call to `websocket.subscribe_to_group()`

2. `websocket.rs:subscribe_to_group()` (line 69) exists but is **never called** anywhere in the codebase

#### Server Broadcasting Logic
1. Server maintains a `groups` HashMap mapping group_id → HashSet<client_id>
2. When a client sends a message, server calls `broadcast_to_group(group_id, message)`
3. This only broadcasts to clients in `groups[group_id]`
4. **Problem**: If client never subscribes, it's not in the HashSet, so it doesn't receive messages

#### Test Scenario Explanation
- Alice creates group "mygroup" with group_id X
- Alice never subscribes to group X on the server (no subscription message sent)
- Bob receives Welcome, joins group, client code calls `subscribe_to_group` (from where?)
- Alice sends message → broadcast to group X → Bob receives it (Bob is subscribed)
- Bob sends message → broadcast to group X → Alice doesn't receive it (Alice NOT subscribed!)

### Files Involved
- `client/rust/src/client.rs` - Missing subscription call in `connect_to_group()`
- `client/rust/src/websocket.rs` - Has `subscribe_to_group()` but is unused
- `client/rust/src/mls/connection.rs` - Might need to add subscription logic
- `server/src/handlers/websocket.rs` - Added detailed logging to trace message flow

## Solution Implemented ✓

### Changes Made

1. **Added helper method to MlsConnection** (`src/mls/connection.rs:336-346`):
   - New `subscribe_to_group(&mut self, group_id: &[u8])` method
   - Handles base64 encoding of group_id
   - Sends WebSocket subscription message to server
   - Centralizes subscription logic to avoid duplication

2. **Updated client.rs** (`src/client.rs:195-196`):
   - Added `self.connection.subscribe_to_group(&group_id).await?` call in `connect_to_group()`
   - Subscription now happens immediately after membership is created/loaded
   - Ensures group creator subscribes to their own group

3. **Refactored Welcome message path** (`src/mls/connection.rs:412-414`):
   - Updated `process_incoming_envelope()` to use new helper method
   - Removed duplicate base64 encoding logic
   - Simplified from 6 lines to 1 call to helper

### How It Fixes the Issue

**Before**:
- Alice creates group → no subscription
- Bob receives Welcome → subscribes (via connection.rs line 395)
- Alice's messages reach Bob (Bob subscribed) ✓
- Bob's messages don't reach Alice (Alice not subscribed) ✗

**After**:
- Alice creates group → subscribes immediately (via client.rs line 196)
- Bob receives Welcome → subscribes (via connection.rs line 414)
- Both directions work bidirectionally ✓

### Files Modified
- `client/rust/src/client.rs` - Added subscription call in `connect_to_group()`
- `client/rust/src/mls/connection.rs` - Added `subscribe_to_group()` helper and refactored existing code
- `server/src/handlers/websocket.rs` - Added detailed logging for debugging

### Current Status
Fix verified and working. Bidirectional messaging now operates correctly in both directions.
