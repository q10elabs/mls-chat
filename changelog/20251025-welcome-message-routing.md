# Welcome Message Routing Bug Fix

## Task Specification
Fix a critical bug where invitees did not receive Welcome messages sent by inviters. The Welcome message is essential for the invitee to join the MLS group, so this prevented newly invited users from participating in group communications.

## Problem Analysis
The bug occurred in the message routing flow:
1. Inviter sends a Welcome message with only the inviter's username
2. Server broadcasts the Welcome to group "welcome" (not targeted)
3. Invitee never receives the message because they weren't subscribed to the right channel
4. Invitee remains unable to process the Welcome and join the group

## High-Level Decisions

### Client-Side Changes
- Added explicit username subscription alongside group subscription
- Modified Welcome envelope to include both `inviter` and `invitee` fields
- Ensures invitee can receive direct messages by listening to their own username channel

### Server-Side Changes
- Changed routing logic to send Welcome directly to the invitee's username
- Instead of broadcasting to generic "welcome" group, target the specific invitee
- Updated message validation to require `invitee` field for routing

### Testing
- Created comprehensive E2E test (`test_welcome_routing.expect`) that:
  - Sets up isolated environments for server and clients
  - Verifies Alice can invite Bob
  - Confirms Bob receives and processes Welcome message
  - Validates Bob can see Alice in member list post-Welcome
  - Tests basic message exchange
  - Cleans up temporary state directories

## Files Modified

1. **client/rust/src/models.rs**
   - Added `invitee: String` field to `WelcomeMessage` enum variant
   - Field contains username of person being invited for server routing

2. **client/rust/src/client.rs**
   - Line 305-316: Added subscription to username in addition to group name
   - Line 446: Added `invitee` field when creating Welcome envelope
   - Line 1011: Updated pattern match to accept new `invitee` field

3. **server/src/handlers/websocket.rs**
   - Lines 244-265: Refactored Welcome message handling
   - Changed from broadcast to "welcome" → direct send to invitee
   - Added validation for `invitee` field
   - Updated message payload to include all required fields (invitee, ratchet_tree_blob)

4. **client/rust/.gitignore**
   - Added `.bob` and `.alice` to ignore test database directories

5. **client/rust/e2e_tests/.gitignore** (new file)
   - Ignores temporary test state directories

6. **client/rust/e2e_tests/test_welcome_routing.expect** (new file)
   - Comprehensive E2E test script for Welcome message flow

## Rationale & Alternatives

### Why Channel-Based Routing?
- Client already uses username-based subscriptions for direct messaging
- Server already has broadcasting capabilities per channel
- Leverages existing infrastructure (no new message queue needed)
- Simple and reliable: if you're subscribed to your username, you get messages meant for you

### Alternative Considered (Not Taken)
- Database-persisted message queue: Would require schema changes and persistent storage overhead
- Special "welcome" channel with recipient info: More complex routing logic, unclear semantics
- Chosen approach is simpler and uses existing patterns

## Obstacles & Solutions

1. **Invitee wasn't subscribed to their username** → Added explicit subscription alongside group subscription
2. **Server didn't know who to route Welcome to** → Added invitee field to envelope for explicit targeting
3. **E2E test needed for verification** → Created expect script with isolated temp directories for clean testing

## Current Status

✅ **Complete** - All staged changes implement the fix:
- Client correctly sends Welcome with invitee identifier
- Client subscribes to both group and personal username channels
- Server routes Welcome directly to invitee instead of broadcast
- E2E test validates complete invite flow and Welcome reception
- Test cleanup preserves state directories for debugging if needed

The fix ensures that invitees receive Welcome messages through direct username-based routing, allowing them to properly join MLS groups when invited.
