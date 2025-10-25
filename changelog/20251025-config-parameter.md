# --config Parameter for Custom Storage Directory

## Task Specification
Add an optional `--config` command-line parameter to the Rust client to specify a custom directory for the MLS state database. The client should default to `~/.mlschat` if the flag is not specified.

## Requirements
- Add `--config` optional parameter to client CLI args
- Default to `~/.mlschat` if not specified
- Pass the config directory to MlsClient initialization
- Update both the `new()` and `new_with_storage_path()` methods as needed

## High-Level Decisions
1. **Argument Parsing**: Add the `--config` flag to the Args struct in main.rs using clap
2. **Default Behavior**: Maintain backward compatibility by defaulting to `~/.mlschat`
3. **Implementation Location**: Modify main.rs to handle the config parameter and pass it to MlsClient::new_with_storage_path()

## Files Modified
- `client/rust/src/main.rs` - Added `--config` argument and updated client initialization
- `client/rust/src/client.rs` - Removed unused `MlsClient::new()` constructor and unused import
- `client/rust/tests/client_tests.rs` - Updated `test_create_client_with_various_urls()` to use `new_with_storage_path()` with temp directories

## Current Architecture
- `MlsClient::new_with_storage_path()` - Primary constructor that accepts custom storage directory
- Storage structure: Uses `metadata.db` for identities and `mls-{username}.db` for MLS group state
- Default storage location resolution now happens in main.rs before client initialization

## Implementation Details

### Changes Made
1. **main.rs (lines 17-19)**: Added `--config` optional argument to Args struct with help text
2. **main.rs (lines 53-72)**: Updated main() function to:
   - Check if `--config` flag is provided
   - If provided, use the specified directory path
   - If not provided, default to `~/.mlschat` using BaseDirs::home_dir()
   - Log the resolved config directory for debugging
   - Call `MlsClient::new_with_storage_path()` with the resolved directory

3. **client_tests.rs (lines 140-153)**: Updated `test_create_client_with_various_urls()` test to:
   - Use separate temp directories for each client instance
   - Replace `MlsClient::new()` calls with `MlsClient::new_with_storage_path()`
   - Improves test isolation and prevents any interaction with user's ~/.mlschat directory
   - Test still passes successfully

4. **client.rs**: Removed unused `MlsClient::new()` constructor:
   - Deleted the async `new()` method (was only 7 lines, delegated to `new_with_storage_path()`)
   - Removed unused import of `directories::BaseDirs` that was only used in the deleted constructor
   - Updated doc comment to reflect that `new_with_storage_path()` is the primary constructor
   - Code now builds cleanly without warnings

### Usage Examples
```bash
# Default behavior - uses ~/.mlschat
cargo run -- mygroup alice

# With custom config directory
cargo run -- --config /path/to/config mygroup alice
cargo run -- --config ./test-config mygroup alice
```

## Current Status
- ✅ Implementation complete
- ✅ Code compiles cleanly without warnings
- ✅ Help text properly documented
- ✅ Test updated with improved isolation
- ✅ Unused constructor removed
- ✅ All files cleaned up (no dead code)
