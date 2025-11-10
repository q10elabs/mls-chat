# E2E Test Debugging - Client Key Packages Implementation

## Task Specification
User has iterated on server and Rust client implementation to add client key packages support. End-to-end tests are now failing and need debugging.

Test command: `cd client/rust/e2e_tests && expect -f test_welcome_routing.expect`

## Status
- PARTIAL FIX IMPLEMENTED: Selected group ID mismatch resolved
- REMAINING ISSUE IDENTIFIED: Subscription race condition with Commit delivery

---

## Issue 1: Selected Group ID Mismatch (FIXED ✓)

### The Bug
When Bob connects to a group before being invited:
1. Bob calls `connect_to_group("mygroup")`
2. `create_new_group()` creates an **empty local MLS group** with `GROUP_ID_1`
3. Bob's client stores `selected_group_id = GROUP_ID_1`

4. Alice sends Welcome message for her group (`GROUP_ID_2`)
5. Bob processes Welcome and creates correct membership with `GROUP_ID_2` containing alice + bob
6. Membership is added to HashMap
7. **BUG**: Bob's `selected_group_id` still points to `GROUP_ID_1` (the empty group)
8. When Bob calls `/list`, he sees only himself from the empty group

### Root Cause
OpenMLS correctly processes the Welcome and creates a membership with all members listed in the GroupInfo. However, the client application maintains a separate `selected_group_id` variable that isn't updated when a new membership is created. This causes the CLI to query the wrong group.

### The Fix Implemented
- Added `MlsConnection::get_membership_by_name()` to search memberships by group name
- Added `MlsClient::sync_selected_group_after_welcome()` to update selected_group_id when a membership is created
- Call sync method in CLI loop after processing each incoming envelope
- Result: Bob's selected_group_id is now updated to point to the real membership with all members

### Files Modified
- `client/rust/src/mls/connection.rs`: Added `get_membership_by_name()` method (lines 604-619)
- `client/rust/src/client.rs`: Added `sync_selected_group_after_welcome()` method (lines 244-276)
- `client/rust/src/cli.rs`: Call sync after envelope processing (line 154)
- `client/rust/src/mls/membership.rs`: Added debug logging for Welcome member count (lines 219-223)

---

## Issue 2: Subscription Race Condition with Commit (IDENTIFIED - NOT YET FIXED)

### The Bug
After Welcome processing, Bob misses Alice's subsequent Commit message:

1. Alice invites Bob and sends Welcome message
2. Bob receives and processes Welcome (creates membership with alice + bob via GroupInfo)
3. Bob's client calls `websocket.subscribe_to_group()` to listen for future group messages
4. **RACE CONDITION**: The subscribe call is non-blocking and returns immediately
   - Client-side: `send_envelope()` sends message to unbounded channel and returns (websocket.rs:78)
   - Server-side: `actix::spawn()` creates background task to register subscription (server websocket.rs:202-204)
5. Meanwhile, Alice sends Commit message broadcast to group
6. Server receives Commit and broadcasts it to members already in the group's subscriber list
7. **PROBLEM**: Bob's subscription task hasn't completed yet - he's not in the subscriber list
8. Commit is broadcast to Alice only
9. Bob never receives the Commit

### Impact
- **Issue 1 Fix**: Bob's Welcome membership is correct with both members shown ✓
- **This Issue**: If Alice later adds/removes members, Bob won't see those changes until he receives a future Commit
- This is a separate transport-level race condition from the OpenMLS protocol itself
- RFC 9420 leaves such timing issues to the application layer

### Root Cause Analysis
- `websocket.subscribe_to_group()` is async but non-blocking: client doesn't wait for confirmation
- Server spawns subscription as background task: `actix::spawn(async move { server.subscribe(...).await })`
- No acknowledgment mechanism to confirm subscription is registered before broadcasting
- Commit is broadcast immediately to current subscribers

### Locations Affected
- Client sending subscribe: `client/rust/src/websocket.rs:78` - fire-and-forget send
- Client waiting for subscribe: `client/rust/src/mls/connection.rs:385` - await but no response
- Server spawning subscribe task: `server/src/handlers/websocket.rs:202-204` - spawned but not awaited
- Server broadcasting: `server/src/handlers/websocket.rs:76-87` - broadcasts immediately to current members

### Recommended Solution: Server-Side Message Buffering with Catch-Up/Replay

**This is the standard production approach used in real messaging systems.**

#### Overview
The server maintains a message log (buffer) of all group communications for a configurable retention period (e.g., N days). When a client subscribes to a group, the server:
1. Sends cached/buffered messages to bring the client up-to-date
2. Then streams live messages as they arrive
3. Also supports partially offline clients that reconnect after being unavailable

#### Benefits
- **Solves Issue 2**: New clients receive all missed messages, including Commits
- **Offline Support**: Clients can go offline and catch up when reconnecting
- **Simple Protocol**: No handshake complexity, purely server-side storage
- **Industry Standard**: Used by Signal, Telegram, WhatsApp, etc.

#### Implementation Strategy
1. **Server-Side Changes** (server/src/):
   - Add message persistence layer (or expand existing database)
   - Store all MLS messages (Welcome, Commit, ApplicationMessage) with timestamp
   - Implement `get_messages_since(group_id, timestamp)` query
   - Send buffered messages to client on subscribe before subscribing to live stream

2. **Client-Side Changes** (minimal):
   - On subscription, receive and process buffered messages first
   - Then open live stream for new messages
   - Already handles message processing correctly

3. **Configuration**:
   - Configurable buffer retention (e.g., 7 days = 604800 seconds)
   - Disk storage efficient (can use SQLite or append-only log)

#### Why This Over Other Solutions
- **vs Subscription Ack**: Solves more than just race condition (also offline)
- **vs Expected Group Size**: No need to predict; always correct
- **vs Delayed Commit**: Lowest latency; no coordination overhead
- **vs Client Resubscription**: More reliable; doesn't depend on timing

#### References
- OpenMLS philosophy: Application responsible for delivery semantics
- Signal Protocol Guide: Uses similar append-only log architecture
- RFC 9420 (MLS): Doesn't mandate delivery guarantees; leaves to app layer

---

## High-Level Decisions

### Decision: Implement sync_selected_group_after_welcome for Issue 1
**Rationale**: This is the correct application-level fix. The Welcome message contains the complete group state via GroupInfo, so Bob's membership IS correct. The issue is purely that the client application was looking at the wrong membership. This fix is minimal, non-invasive, and aligns with OpenMLS design.

### Decision: Defer Issue 2 fix for later
**Rationale**: Issue 1 fix resolves the `/list` test failures. Issue 2 (Commit delivery race) is a separate architectural concern that requires more design work. It only manifests when Alice adds/removes members after the initial invite, which is not part of the basic Welcome routing test.

### Recommendation for Future Issue 2 Work
**Implement server-side message buffering and catch-up mechanism:**
- This is the industry-standard approach (Signal, Telegram, WhatsApp all use this)
- Solves not just the race condition but also offline client support
- Aligns with OpenMLS philosophy (app layer responsible for delivery semantics)
- Minimal protocol complexity compared to acknowledgment schemes
- Use configurable retention (e.g., 7 days) for storage efficiency
