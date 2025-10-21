# Changelog: Replace Public Key API with Key Package API

**Date:** 2025-10-21
**Task:** Replace the server's public key API endpoints and storage with key package API endpoints and storage
**Status:** ✅ COMPLETED

## Task Specification

Replace the current public key registration/retrieval API with a key package API:
- Endpoints register/retrieve key packages instead of public keys
- Key packages stored as opaque bytes (no parsing or validation on server)
- Updated all associated tests accordingly
- Breaking change: API now uses Vec<u8> instead of String

## Implementation Summary

### Files Modified:

1. **Models:** `server/src/db/models.rs`
   - `User` struct: `public_key: String` → `key_package: Vec<u8>`
   - `RegisterUserRequest`: `public_key: String` → `key_package: Vec<u8>`
   - `UserKeyResponse`: `public_key: String` → `key_package: Vec<u8>`
   - Updated model tests to use byte vectors

2. **Database Schema:** `server/src/db/init.rs`
   - `users` table: `public_key TEXT` → `key_package BLOB`
   - Updated schema validation test to check for `key_package` column

3. **Database Operations:** `server/src/db/mod.rs`
   - `register_user()`: accepts `key_package: &[u8]` instead of `public_key: &str`
   - Updated all database queries to use `key_package` column
   - Updated all 8 unit test functions to use Vec<u8> byte arrays

4. **REST Handlers:** `server/src/handlers/rest.rs`
   - `register_user` handler: updated to pass `&req.key_package` to database
   - `get_user_key` handler: updated to return `user.key_package` in response
   - Updated endpoint documentation comments

5. **Server Tests:** `server/src/server.rs`
   - Updated `test_create_test_http_server_with_pool_persistence`
   - Updated `test_register_user_endpoint` to use Vec<u8>
   - Updated `test_get_user_key_endpoint` to use Vec<u8>
   - Updated `test_store_and_get_backup_endpoints` to use Vec<u8>

6. **Integration Tests:** `server/tests/integration_tests.rs`
   - Updated all 10 integration tests to use Vec<u8> for key packages
   - Tests now verify `key_package` field equality instead of `public_key`

7. **WebSocket Tests:** `server/tests/websocket_tests.rs`
   - Updated `test_websocket_persist_message`
   - Updated `test_websocket_multiple_clients_same_group`

## Design Decisions Made

1. **Field Naming:** Changed to `key_package` throughout for clarity and semantics
2. **Data Format:** Changed to `Vec<u8>` for binary-safe opaque storage
3. **Database Column Type:** Changed to BLOB for efficient binary storage
4. **Endpoint Paths:** Kept `/users` and `/users/{username}` for API simplicity
5. **API Format:** Key packages in JSON will be automatically base64-encoded/decoded by serde

## Testing Results

All tests pass successfully:
- ✅ 32 unit tests pass (db, models, server, websocket)
- ✅ 10 integration tests pass
- ✅ 9 websocket tests pass
- ✅ **Total: 51 tests passing** with no failures

## Potential Issues & Solutions

| Issue | Solution |
|-------|----------|
| Binary data in JSON | Serde automatically base64-encodes Vec<u8> in JSON |
| SQLite BLOB support | SQLite BLOB type fully supports binary data |
| Migration path | Column can be left as-is; SQLite is flexible with TEXT/BLOB |

## Notes for API Clients

- REST API clients sending key packages in JSON will need to send base64-encoded strings (serde handles automatically)
- Response endpoints will base64-encode the key package in JSON responses
- Key packages are now fully opaque - no server-side parsing or validation
- Size/format constraints entirely client-side responsibility
