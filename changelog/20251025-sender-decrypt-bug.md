# 20251025-sender-decrypt-bug

## Task Specification
Investigate and fix a bug discovered via E2E testing where the sender of an encrypted message attempts to decrypt their own messages. Need to add an exception to prevent this behavior on receive.

## Investigation Status
- [x] Identified message processing logic that handles incoming messages
- [x] Located where the sender/recipient check should occur
- [x] Determined the appropriate fix location
- [x] Implemented and tested the fix
- [x] All tests pass (57 lib tests + 50 integration tests)

## Bug Analysis

### Problem
When a client sends an encrypted application message, the server broadcasts it to ALL group members including the sender. The sender then tries to decrypt their own message using their own ratchet state.

The issue is that application messages are processed without checking if the sender is the current user. Unlike Commit messages (which have this check at client.rs:1053), application messages lack this guard.

### Root Cause Location
- **File:** `client/rust/src/client.rs`
- **Function:** `process_incoming_envelope_from()` (lines 1001-1029)
- **Lines:** 1003-1028 (ApplicationMessage handling)

In the CommitMessage handling (lines 1045-1110), there's a proper check:
```rust
if sender == self.username {
    log::debug!("Skipping our own Commit message (already merged when sent)");
    return Ok(());
}
```

The ApplicationMessage handling LACKS this check.

### Why It Fails
When Alice sends a message:
1. Her ratchet state advances (sender side)
2. Server broadcasts to all members (including Alice)
3. Alice receives her own message
4. Alice tries to decrypt using her updated ratchet state
5. Decryption fails because she needs to use the ratchet state from BEFORE sending (recipient side expectation)

### Solution
Add a sender check in ApplicationMessage handling to skip processing messages from the current user, similar to CommitMessage handling.

## Files Modified
- `client/rust/src/client.rs` (lines 1003-1036) - Added sender check to ApplicationMessage block
  - Added early return if sender == self.username
  - Added explanatory comments about ratchet state sync issues

## Files Enhanced with Tests
- `client/rust/tests/client_tests.rs` - Added Integration Test 7: `test_sender_skips_own_application_message`
  - Tests the complete scenario: Alice sends a message, server broadcasts back, Alice skips her own message
  - Verifies no decryption errors occur when sender receives broadcast of their own message

## Test Results
All tests pass successfully:
- **Library tests:** 57/57 ✓
- **Client integration tests:** 17/17 ✓ (including new sender-skip test)
- **Message processing tests:** 10/10 ✓
- **API tests:** 6/6 ✓
- **Total:** 50 tests ✓

## Implementation Details

### The Fix (in client.rs, process_incoming_envelope_from)
Added sender check at the start of ApplicationMessage handling:
```rust
if sender == self.username {
    log::debug!("Skipping our own application message (ratchet state already advanced on send)");
    return Ok(());
}
```

### Why This Works
1. **Sender-side behavior:** When Alice creates an application message, her ratchet state advances
2. **Broadcast behavior:** Server sends the message to ALL group members (including sender)
3. **Receiver-side expectation:** Normal recipients expect to use their ratchet in the "pre-send" state
4. **The skip:** By skipping her own message, Alice avoids attempting decryption with out-of-sync ratchet

This mirrors the existing pattern used for CommitMessage handling (line 1053)
