# E2E Test Bug: Bob Cannot Receive Encrypted Messages

## Task Specification
Investigate and fix a bug discovered during e2e test iteration with the rust client:
- Client 1 (Alice) sends welcome message → Bob successfully accepts it
- Bob attempts to receive encrypted messages from Alice → FAILS
- Need to understand why message acceptance works but encrypted message reception doesn't

## Current Status
- FIX IMPLEMENTED - Testing phase
- Bug location: server/src/handlers/websocket.rs:261
- Solution: Add client subscription to group after accepting Welcome message

## Root Cause Analysis

### The Bug
In `server/src/handlers/websocket.rs` at line 261, the Welcome message is routed incorrectly:

```rust
// WRONG: Uses invitee username as if it were a group_id
server.broadcast_to_group(&invitee, &msg).await;
```

### What Should Happen
The Welcome message should be sent to the invitee's direct connection/subscription, not broadcast to a "group" with that name.

### Why This Breaks Encrypted Messages
1. **Welcome works** because it's sent to just the invitee (even though routing is wrong, the message happens to reach them)
2. **Encrypted application messages fail** because:
   - They're broadcast to the actual group_id (correct)
   - But Bob never properly subscribed to the real group because he received Welcome via wrong routing
   - The Welcome message doesn't add Bob to the group's subscription list

### Contrast with Encrypted Messages
At line 235, application messages are correctly routed:
```rust
server.broadcast_to_group(&group_id, &msg).await;  // CORRECT
```

The `broadcast_to_group()` function works with a `groups` map that tracks which members are subscribed to which groups. Welcome routing bypasses this subscription mechanism entirely.

## Files to Investigate
- server/src/handlers/websocket.rs - The routing bug is here
- How subscription/registration works for group members
- How Bob should be added to the group's subscriber list after Welcome

## High-Level Decisions
- Use option 2: Subscribe client to group after receiving Welcome
- This approach maintains the existing message routing pattern
- Welcome is still sent to the invitee's username (for discovery/routing)
- But after receiving Welcome, the client immediately subscribes to the real group_id
- This ensures clients receive all encrypted messages broadcast to the group

## Solution Implementation
After Bob successfully processes the Welcome message and joins the MLS group, he now immediately subscribes to the group via WebSocket:

```rust
// In handle_welcome_message() after successfully joining the MLS group:
self.websocket
    .as_ref()
    .ok_or_else(|| ClientError::Config("WebSocket not connected".to_string()))?
    .subscribe_to_group(group_name)
    .await?;
```

This mirrors the same subscription pattern used during normal group creation/joining.

## Requirements Changes
(None yet)

## Files Modified
- client/rust/src/client.rs - Added group subscription in `handle_welcome_message()` (Step 8)

## Obstacles and Solutions
(To be tracked)
