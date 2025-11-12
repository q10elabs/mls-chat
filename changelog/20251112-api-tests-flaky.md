# Task: Fix Flaky API Tests

## Task Specification
User reports non-deterministic failures in `cargo test --manifest-path client/rust/Cargo.toml --test api_tests` when run in interactive shell. Errors consistently appear as database file access failures:
- "unable to open database file"
- Database(SqliteFailure) with code CannotOpen
- Migration error: unable to open database file

## Root Cause Analysis
**IDENTIFIED**: The issue is a tempdir lifetime bug in test helper functions.

In `generate_keypackage_upload()` (line 14) and `generate_test_key_package()` (line 63):
1. `tempdir()` is called to create a temporary directory
2. Database file path is derived from this directory
3. `MlsProvider::new()` is called to initialize the provider with the db file path
4. The function returns while `temp_dir` is still in scope
5. However, the returned `KeyPackageUpload` or key data is used AFTER the function returns
6. When `temp_dir` goes out of scope at end of function, the directory is deleted
7. Later when the provider is actually used in tests, the database file no longer exists
8. This causes "unable to open database file" errors

The error is non-deterministic because it depends on timing - whether the temp directory is deleted before the database is actually accessed.

## High-Level Decisions
- Solution: Use `std::fs::tempdir()` with `std::env::temp_dir()` approach that persists for test duration
- Alternative: Keep `temp_dir` alive by storing it or using a static/lazy_static approach
- Chosen approach: Modify helper functions to accept a database path parameter instead of creating temp dirs internally, letting the caller manage lifetime

## Files To Modify
- `client/rust/tests/api_tests.rs` - Refactor test helper functions

## Implementation Details

### Changes Made
1. **Line 14-16 in `generate_keypackage_upload()`**: Replaced `tempdir()` with `MlsProvider::new_in_memory()`
   - Removed tempfile dependency usage
   - Provider now persists for the entire function scope

2. **Line 59-62 in `generate_test_key_package()`**: Replaced `tempdir()` with `MlsProvider::new_in_memory()`
   - Removed tempfile dependency usage
   - In-memory database works identically for test purposes

### Why This Fix Works
- In-memory providers don't create/delete temporary files
- No lifetime issues since the provider only needs to exist during the function
- Key packages are serialized to bytes before returning, so no database access needed after function returns
- Reduces I/O overhead and makes tests faster

## Current Status
✅ COMPLETE
- Root cause identified and documented: Premature tempdir cleanup
- Fix implemented: Replaced file-based temp databases with in-memory providers
- Testing: Ran API tests 5 times - all 14 tests passed consistently
- Linting: `cargo fmt` and `cargo clippy` run with no errors/warnings
