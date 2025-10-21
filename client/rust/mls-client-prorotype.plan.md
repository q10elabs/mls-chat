# MLS Chat Client Prototype Implementation (Test-Driven)

## Overview

Build a Rust client implementing MLS group messaging with OpenMLS using test-driven development. Each component has tests that must pass before proceeding to the next step.

## Test-Driven Development Approach

**For each implementation step:**

1. Write tests first (or alongside implementation)
2. Implement the feature
3. Run tests: `cargo test <module_name>`
4. All tests must pass before proceeding to next step
5. If tests fail, debug and fix until all pass

## Architecture

**Core Components:**

- `MlsClient`: Main orchestrator managing MLS operations and server communication
- `LocalStore`: SQLite-based persistence for MLS state and key material (keyed by username)
- `ServerApi`: REST client for user registration and key retrieval
- `MessageHandler`: WebSocket handler for real-time message delivery
- `CliInterface`: Command-line input/output handler

**Key Principles:**

- Server is MLS-opaque (only relays encrypted blobs)
- Client-side state for all group membership and invitations
- Identity reuse via username-keyed storage
- All data in `~/.mlschat/client.db`

**Dependencies:** openmls (0.7.1), openmls_rust_crypto (0.4.1), openmls_basic_credential (0.4.1), openmls_sqlite_storage (0.1.1), reqwest, tokio-tungstenite, serde_json, clap, anyhow, thiserror, directories

## Implementation Steps

### 1. Project Setup + Error Types

**Files:** `Cargo.toml`, `src/main.rs`, `src/error.rs`, `src/lib.rs`

Create project structure with all dependencies and error types.

**Error types in `src/error.rs`:**

- `ClientError`, `StorageError`, `NetworkError`, `MlsError`, `InvalidCommand`

**Tests:** Basic compilation test

- ✅ `cargo build` succeeds
- ✅ Error types compile

**Checkpoint:** `cargo build` - Must succeed

---

### 2. Local Storage Layer

**File:** `src/storage.rs`

**Use `openmls_sqlite_storage` for MLS state persistence:**

- Integrate with `openmls_sqlite_storage::SqliteStorageProvider`
- Store MLS group states, key packages, and credentials
- Custom wrapper for username-keyed storage and group metadata

**Methods:**

- `new(db_path) -> Result<Self>`
- `get_storage_provider() -> &SqliteStorageProvider`
- `save_group_metadata(username, group_id, members)`
- `load_group_metadata(username, group_id) -> Option<GroupMetadata>`
- `list_user_groups(username) -> Vec<GroupId>`

**Tests in `src/storage.rs` #[cfg(test)]:**

- ✅ `test_initialize_storage_provider`
- ✅ `test_save_and_load_group_metadata`
- ✅ `test_list_user_groups`
- ✅ `test_multiple_users_same_db`

**Checkpoint:** `cargo test storage` - All tests must pass

---

### 3. MLS Crypto Operations

**File:** `src/crypto.rs`

**Functions using current OpenMLS API:**

- `generate_credential_with_key(username) -> (CredentialWithKey, SignatureKeyPair)`
- `generate_key_package(credential, signer, provider) -> KeyPackageBundle`
- `create_group(provider, signer, config, credential) -> MlsGroup`
- `create_application_message(group, provider, signer, plaintext) -> MlsMessageOut`
- `process_message(group, provider, message) -> ProcessedMessage`
- `add_members(group, provider, signer, key_packages) -> (MlsMessageOut, Welcome)`
- `process_welcome(provider, config, welcome, ratchet_tree) -> MlsGroup`

**API Usage:**

- Provider: `OpenMlsRustCrypto::default()`
- Credentials: `BasicCredential::new(identity)` + `SignatureKeyPair::new()`
- Groups: `MlsGroup::new(provider, signer, config, credential)`
- Messages: `group.create_application_message()` + `group.process_message()`
- Ciphersuite: `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`

**Tests in `src/crypto.rs` #[cfg(test)]:**

- ✅ `test_generate_credential_with_key`
- ✅ `test_generate_key_package_bundle`
- ✅ `test_create_group_with_config`
- ✅ `test_create_and_process_application_message`
- ✅ `test_add_member_flow` (Alice adds Bob, Bob processes Welcome)
- ✅ `test_two_party_messaging` (Alice and Bob exchange messages)
- ✅ `test_three_party_messaging` (Alice, Bob, Carol)

**Checkpoint:** `cargo test crypto` - All tests must pass

---

### 4. Server API Client

**File:** `src/api.rs`

**ServerApi struct with methods:**

- `new(base_url) -> Self`
- `register_user(username, public_key) -> Result<()>`
- `get_user_key(username) -> Result<String>`
- `health_check() -> Result<()>`

Uses `reqwest` for HTTP requests to server REST endpoints.

**Tests in `tests/api_tests.rs`:**

- ✅ `test_register_new_user` (spawn test server, register, verify)
- ✅ `test_register_duplicate_user` (register twice, check idempotent)
- ✅ `test_get_user_key` (register, fetch key, verify match)
- ✅ `test_get_nonexistent_user` (fetch non-existent, verify 404)
- ✅ `test_health_check` (verify health endpoint)

**Test setup helper:** `spawn_test_server() -> (ServerHandle, String)` using `mls-chat-server` as dev-dependency

**Checkpoint:** `cargo test api` - All tests must pass

---

### 5. WebSocket Message Handler

**File:** `src/websocket.rs`

**MessageHandler struct:**

- `connect(server_url, username) -> Result<Self>`
- `subscribe_to_group(group_id) -> Result<()>`
- `send_message(group_id, encrypted_content) -> Result<()>`
- `next_message() -> Result<Option<IncomingMessage>>`

**Message types:** Subscribe, Send, Receive (all with opaque encrypted content)

**Tests in `tests/websocket_tests.rs`:**

- ✅ `test_connect_to_server`
- ✅ `test_subscribe_to_group`
- ✅ `test_send_message`
- ✅ `test_receive_message` (two clients, one sends, other receives)
- ✅ `test_broadcast_to_group` (multiple clients in group)

**Checkpoint:** `cargo test websocket` - All tests must pass

---

### 6. Models and DTOs

**File:** `src/models.rs`

**Data structures:**

- `Identity { username, keypair, credential }`
- `GroupState { group_id, mls_group, members }`
- `IncomingMessage { sender, group_id, encrypted_content }`
- `Command` enum (Invite, List, Message, Quit)

**Tests in `src/models.rs` #[cfg(test)]:**

- ✅ `test_command_parsing` (parse various command strings)
- ✅ `test_message_serialization` (serialize/deserialize messages)

**Checkpoint:** `cargo test models` - All tests must pass

---

### 7. Main Client Orchestrator

**File:** `src/client.rs`

**MlsClient struct coordinating all components:**

- `new(server_url, username, group_name, db_path) -> Result<Self>`
- `initialize() -> Result<()>` (load or create identity, register with server)
- `connect_to_group() -> Result<()>` (create or load group)
- `send_message(text) -> Result<()>` (encrypt and send)
- `process_incoming() -> Result<()>` (receive, decrypt, display)
- `invite_user(invitee_username) -> Result<()>` (add member, send Welcome)
- `list_members() -> Vec<String>`

**Tests in `tests/client_tests.rs`:**

- ✅ `test_client_initialization` (create client, verify identity)
- ✅ `test_identity_reuse` (create client twice with same username)
- ✅ `test_create_group` (verify group creation)
- ✅ `test_send_and_receive_message` (two clients exchange messages)
- ✅ `test_invite_user` (A invites B, B joins via Welcome)
- ✅ `test_list_members` (verify member list accuracy)
- ✅ `test_persistence` (restart client, verify state restored)

**Checkpoint:** `cargo test client` - All tests must pass

---

### 8. CLI Interface

**File:** `src/cli.rs`

**CLI functions:**

- `parse_command(input) -> Result<Command>`
- `format_message(group, username, text) -> String` (returns `#group <user> text`)
- `format_control(group, action) -> String` (returns `#group action`)
- `run_input_loop(callback) -> Result<()>`

**Tests in `src/cli.rs` #[cfg(test)]:**

- ✅ `test_parse_invite_command`
- ✅ `test_parse_list_command`
- ✅ `test_parse_regular_message`
- ✅ `test_format_message`
- ✅ `test_format_control_message`
- ✅ `test_invalid_command`

**Checkpoint:** `cargo test cli` - All tests must pass

---

### 9. Main Entry Point

**File:** `src/main.rs`

Parse args with clap, create MlsClient, run event loop.

**Args:**

- `--server` (default: "localhost:4000")
- `<group_name>`
- `<username>`

**Main flow:**

1. Parse args
2. Create client
3. Initialize (load/create identity)
4. Connect to group
5. Spawn task for incoming messages
6. Run CLI input loop

**Manual testing:**

```bash
# Terminal 1: Start server
cd server && cargo run

# Terminal 2: Alice creates group
cd client/rust && cargo run testgroup alice

# Terminal 3: Bob joins
cd client/rust && cargo run testgroup bob

# Alice invites Bob: /invite bob
# Both exchange messages
```

---

### 10. End-to-End Integration Tests

**File:** `tests/integration_tests.rs`

Full workflow tests with test server.

**Tests:**

- ✅ `test_full_two_user_flow` (Alice creates, invites Bob, exchange messages)
- ✅ `test_identity_persistence` (restart client, verify identity reused)
- ✅ `test_three_party_group` (Alice, Bob, Carol)
- ✅ `test_member_list_accuracy`
- ✅ `test_reconnection_after_disconnect`
- ✅ `test_invite_sequence` (multiple sequential invites)

**Checkpoint:** `cargo test --test integration_tests` - All tests must pass

---

### 11. Documentation

**File:** `client/rust/README.md`

- Installation instructions
- Usage examples
- Architecture overview
- Testing guide
- Troubleshooting

---

## Test Execution Strategy

**After each step:**

1. Run specific module tests: `cargo test <module>`
2. Verify all pass
3. Run full test suite: `cargo test`
4. Fix any regressions
5. Only then proceed to next step

**Final acceptance:**

- `cargo test` - all tests pass
- `cargo clippy` - no warnings
- `cargo build --release` - clean build
- Manual smoke test with real server

## Key Files Summary

1. `Cargo.toml` - Dependencies
2. `src/lib.rs` - Library exports
3. `src/error.rs` - Error types
4. `src/storage.rs` - SQLite persistence (8 tests)
5. `src/crypto.rs` - MLS operations (7 tests)
6. `src/api.rs` - REST client
7. `src/websocket.rs` - WebSocket handler
8. `src/models.rs` - Data structures (2 tests)
9. `src/client.rs` - Main orchestrator
10. `src/cli.rs` - Terminal UI (6 tests)
11. `src/main.rs` - Entry point
12. `tests/api_tests.rs` - API tests (5 tests)
13. `tests/websocket_tests.rs` - WebSocket tests (5 tests)
14. `tests/client_tests.rs` - Client tests (7 tests)
15. `tests/integration_tests.rs` - E2E tests (6 tests)
16. `README.md` - Documentation

**Total: ~40 automated tests across all components**

## Server Requirements

**No modifications needed** - existing server already supports MLS-opaque operation:

- ✅ `POST /users` - User registration
- ✅ `GET /users/:username` - Key retrieval
- ✅ `ws://server/ws/:username` - WebSocket relay
- ✅ Subscribe/broadcast via WebSocket

## Identity Management

**Username-based bundles:**

- Storage: `~/.mlschat/client.db`
- Each username has unique identity bundle
- Running with existing username reuses identity
- Running with new username creates new bundle
- Multiple identities coexist in same database
