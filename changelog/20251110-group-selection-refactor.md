# Group Selection Refactoring - Welcome Message Authority

## Task Specification
Refactor the Rust client's `sync_selected_group_after_welcome` logic to make the Welcome message processor the authority for group selection, rather than the command-line argument. After processing a Welcome message, the client should switch to the group that received it, regardless of the initially selected group.

As a corollary, the client should only track "current group name" rather than "initial group name" since the current group dynamically changes based on Welcome messages.

## Requirements
1. **Multiple Welcomes**: Always switch to the most recent Welcome message's group
2. **Initial Group Creation**: Keep command-line group name for creating new MLS groups if not in database
3. **Group Names in Current Context**: Use current group name (from membership) in logging/lookups, not command-line name
4. **Message Operations**: Client can work with groups from previous runs
5. **Data Structure Simplification**: Remove `initial_group_name`, rely on `selected_group_id` + membership HashMap

## Status
- [x] Explore current implementation
- [x] Clarify requirements
- [x] Plan implementation approach
- [x] Implement changes
- [x] Test changes - all 71 tests passing

## Implementation Plan

### Changes Overview

**Core Refactoring:**
1. Remove `initial_group_name` field from `MlsClient` struct
2. Refactor `process_incoming_envelope()` to return the group_id when Welcome creates a new membership
3. Update `cli.rs` to directly set `selected_group_id` from Welcome message results
4. Remove `sync_selected_group_after_welcome()` method (no longer needed)
5. Update any logging/lookups to use the current membership instead of the initial name

**Files to Modify:**
- `client/rust/src/client.rs` - Remove field, update `sync_selected_group_after_welcome()` usage
- `client/rust/src/mls/connection.rs` - Change return type of `process_incoming_envelope()`
- `client/rust/src/cli.rs` - Handle the new return type, set `selected_group_id` directly
- Any other files referencing `initial_group_name`

### Detailed Steps
1. Examine all usages of `initial_group_name` in the codebase
2. Change `process_incoming_envelope()` signature to return `Result<Option<Vec<u8>>>` (group_id on Welcome)
3. Update Welcome message handler to return the new group_id
4. Remove `sync_selected_group_after_welcome()` from `MlsClient`
5. Update `cli.rs` event loop to handle the return value directly
6. Remove `initial_group_name` field from struct
7. Update any initialization code that sets this field
8. Test that group selection works correctly with Welcome messages

## Files Modified

1. **client/rust/src/client.rs**
   - Removed `initial_group_name: String` field from `MlsClient` struct
   - Updated `connect_to_group()` signature to accept `group_name: &str` parameter (was storing in field)
   - Removed `sync_selected_group_after_welcome()` method entirely
   - Removed `get_group_name()` test helper method
   - Added `set_selected_group_id()` public method for CLI to update selected group after Welcome
   - Updated test assertions to reflect new struct and API

2. **client/rust/src/mls/connection.rs**
   - Changed `process_incoming_envelope()` return type from `Result<()>` to `Result<Option<Vec<u8>>>`
   - Welcome message handler now returns `Ok(Some(group_id))` instead of `Ok(())`
   - ApplicationMessage and CommitMessage handlers return `Ok(None)` instead of `Ok(())`
   - Updated documentation to explain return value semantics
   - Updated all three integration tests to verify Welcome messages return Some(group_id)

3. **client/rust/src/cli.rs**
   - Added import: `use base64::{engine::general_purpose, Engine as _}`
   - Updated event loop to handle `Result<Option<Vec<u8>>>` from `process_incoming_envelope()`
   - Removed call to `sync_selected_group_after_welcome()`
   - When Welcome message is processed, directly set `selected_group_id` via `client.set_selected_group_id()`
   - Added user feedback message when group switches due to Welcome

4. **client/rust/src/main.rs**
   - Updated `connect_to_group()` call to pass `&args.group_name` as parameter
   - Added comment explaining behavior: CLI group name creates initial group, Welcome messages can switch active group

## Key Changes Summary

**Before:** Client stored `initial_group_name`, used `sync_selected_group_after_welcome()` to match it with actual membership after Welcome
**After:** Client only stores `selected_group_id` + membership HashMap; Welcome messages directly return group_id for immediate switching

**Benefits:**
- Simpler data structure (one less field)
- More explicit control flow (Welcome directly updates selection)
- Automatic multi-Welcome support (latest always wins)
- No need for post-processing sync method
