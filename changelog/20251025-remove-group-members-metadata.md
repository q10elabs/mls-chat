# Task: Remove Group Membership Information from Client Metadata Store

## Task Specification
Remove group membership information (group members tracking) from the client metadata store (storage.rs). This includes:
- Remove `group_members` table from SQLite schema
- Remove `save_group_members()` and `get_group_members()` methods from LocalStore
- Remove related tests for group members functionality
- Update `list_members()` in client.rs to rely ONLY on MlsGroup (remove fallback to metadata store)
- Remove any calls to group member metadata methods in client.rs

## High-Level Decisions
1. Members list should be derived exclusively from MlsGroup state, not cached in metadata store
2. Simplifies storage layer and removes redundancy
3. Ensures single source of truth for member list (MlsGroup)
4. Reduces database schema complexity (only identities table now)

## Files Modified
1. `client/rust/src/storage.rs`
   - Removed `group_members` table from schema initialization
   - Removed `save_group_members()` method
   - Removed `get_group_members()` method
   - Removed test: `test_save_and_get_group_members()`
   - Updated module documentation to reflect members are from MlsGroup

2. `client/rust/src/client.rs`
   - Updated `list_members()` to only return members from MlsGroup
   - Changed fallback behavior from metadata store to empty vector
   - Removed `save_group_members()` call in `connect_to_group()` when creating new group

## Rationales and Alternatives
- **Choice**: Remove metadata store group members entirely
- **Rationale**: Single source of truth (MlsGroup) is more reliable and eliminates sync issues
- **Alternative considered**: Keep cached version - rejected due to potential inconsistency

## Obstacles and Solutions
- None encountered - straightforward refactoring

## Test Results
- All 54 unit tests pass
- No compilation errors
- Removed 1 test related to group members functionality (test_save_and_get_group_members)
- Kept 3 identity-related tests

## Current Status
- Implementation complete
- All tests passing
- Ready for commit
