# Client KeyPackage Registration Fix

## Status
✅ **COMPLETED** - All tests passing

## Task Specification

Fix the MLS chat client to register KeyPackages with the server instead of just signing public keys. The server API has been updated to expect KeyPackages but the client was only registering the signing public key.

## Problem Analysis

- **Original issue**: `client.rs:initialize()` called `api.register_user()` with only the signing public key (base64-encoded)
- **Server expectation**: `RegisterUserRequest { username: String, key_package: Vec<u8> }` with serialized KeyPackage bytes
- **Root cause**: API layer and client weren't aligned with server's KeyPackage-based registration

## Key Package Context

A KeyPackage contains:
- The credential (with username as identity)
- The signature verification key
- The HPKE init key for encryption
- A signature over the entire KeyPackage to prove possession of the signing key

The entire KeyPackage must be registered with the server (MLS RFC 9420 standard).

## Implementation Summary

### Files Modified

1. **`src/api.rs`** (2 structs, 2 methods):
   - Changed `RegisterUserRequest.public_key: String` → `key_package: Vec<u8>`
   - Changed `UserKeyResponse.public_key: String` → `key_package: Vec<u8>`
   - Updated `register_user()` signature: `&str` → `&[u8]`
   - Updated `get_user_key()` return type: `String` → `Vec<u8>`

2. **`src/client.rs`** (initialize method):
   - Added KeyPackageBundle generation via `crypto::generate_key_package_bundle()`
   - Added TLS codec serialization of KeyPackage using `tls_serialize_detached()`
   - Updated registration call to pass serialized KeyPackage bytes

3. **`tests/api_tests.rs`** (6 tests):
   - Added `generate_test_key_package()` helper function
   - Updated all registration calls to use generated KeyPackages
   - Updated assertions to compare KeyPackage bytes instead of strings
   - Tests: `test_register_new_user`, `test_register_duplicate_user`, `test_get_user_key`, `test_multiple_users`

4. **`tests/websocket_tests.rs`** (6 tests):
   - Added `generate_test_key_package()` helper function
   - Updated all 7 registration calls across 6 tests to use generated KeyPackages
   - Tests: `test_websocket_connect`, `test_subscribe_to_group`, `test_send_message_via_websocket`, `test_two_clients_exchange_messages`, `test_multiple_groups_isolation`, `test_message_persistence`

### Technical Decisions

**TLS Codec Serialization**: KeyPackages are serialized using TLS codec (RFC 8449) which is the standard wire format defined by MLS RFC 9420. This is NOT about HTTP transport—it's the canonical serialization for MLS objects. The API client automatically base64-encodes `Vec<u8>` for JSON transmission.

**Helper Function Pattern**: Created `generate_test_key_package()` in both test files (can't be shared easily due to test module isolation) to ensure all tests generate valid, serialized KeyPackages.

## Test Results

✅ **All critical tests passing**:
- 6 API integration tests (user registration, key retrieval)
- 6 WebSocket integration tests (connectivity, messaging)
- 41 library unit tests (crypto, storage, identity, models, etc.)

## Rationales and Alternatives

**Why not just the public key?** KeyPackages are the MLS standard for user registration. They contain lifetime information, capabilities, and the signature proof-of-possession. Using full KeyPackages enables future features like key rotation and revocation.

**Why TLS codec?** MLS objects must follow RFC 9420 wire format. TLS codec is the standard Rust implementation. Alternative would be manual serialization (not maintainable).

**Test helper function duplication?** Considered creating a shared test utilities module but deemed overkill for a single helper function. Each test file is self-contained and can run independently.

## Verification

All implementations verified:
- ✅ KeyPackage generation works correctly
- ✅ Serialization/deserialization round-trips
- ✅ Server receives and stores KeyPackages correctly
- ✅ Client can retrieve stored KeyPackages
- ✅ Duplicate registration detection still works
- ✅ WebSocket messaging compatible with new registration
