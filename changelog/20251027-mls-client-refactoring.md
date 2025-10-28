# MLS Client Architecture Refactoring

**Date:** 2025-10-27
**Objective:** Split `MlsClient` to support multiple groups per user by separating concerns into `MlsConnection`, `MlsUser`, and `MlsMembership`.

## Task Specification

Currently, `MlsClient` is a monolithic struct that combines:
1. External service interfaces (LocalStore, MlsProvider, ServerApi, WebSocket)
2. User identity and key management (signature_key, credential_with_key)
3. Single group state (mls_group, group_id, group_name)

To support multiple groups per user, we need to refactor into three layers:
- **MlsConnection**: Manages external service interfaces
- **MlsUser**: Manages user identity and registration (instantiated once per user)
- **MlsMembership**: Manages session state for a specific group (instantiated per group)

## Architecture Design

### MlsConnection (Message Hub & Infrastructure Orchestrator)
**Responsibility:**
- Own and manage external service interfaces (LocalStore, MlsProvider, ServerApi, WebSocket)
- Accept incoming messages from server (WebSocket)
- Route/fan-out messages to relevant entities (memberships)
- Coordinate user identity initialization and lifecycle

**Owning fields:**
- `server_url: String`
- `username: String`
- `metadata_store: LocalStore` (app-level metadata)
- `mls_provider: MlsProvider` (OpenMLS storage, credentials, keypackages)
- `api: ServerApi` (HTTP communication)
- `websocket: MessageHandler` (WebSocket connection)
- `user: Option<MlsUser>` (loaded on initialization)
- `memberships: HashMap<String, MlsMembership>` (one per group, keyed by group_id)

**Methods:**
- `new(server_url, username, storage_dir) -> Result<Self>` - Initialize infrastructure
- `initialize() -> Result<()>` - Load/create user identity and keypackage registration
- `process_incoming_envelope(envelope) -> Result<()>` - Accept and route messages:
  - WelcomeMessage → Create new MlsMembership from envelope
  - ApplicationMessage/CommitMessage → Route to relevant MlsMembership
- `next_envelope() -> Result<Option<MlsMessageEnvelope>>` - Receive from WebSocket
- Accessors: `get_user()`, `get_provider()`, `get_api()`, `get_username()`, `get_metadata_store()`
- Lifecycle: `connect_websocket()`, `disconnect()`

### MlsUser (Manages Identity)
**Responsibility:** User identity, signature keys, credential, keypackage registration

**Owning fields:**
- `username: String`
- `signature_key: openmls_basic_credential::SignatureKeyPair` (cached persistent key)
- `credential_with_key: openmls::prelude::CredentialWithKey` (reused across all groups)
- `identity: Identity` (metadata about the user)

**Methods:**
- Created by: `MlsConnection::initialize()`
- `new(username, identity, signature_key, credential_with_key) -> Self`
- `get_username() -> &str`
- `get_credential_with_key() -> &CredentialWithKey`
- `get_signature_key() -> &SignatureKeyPair`
- `get_identity() -> &Identity`

### MlsMembership (Manages Group Session)
**Responsibility:** Single group session state and group-specific operations

**Owning fields:**
- `group_name: String`
- `group_id: Vec<u8>`
- `mls_group: openmls::prelude::MlsGroup`
- `connection: &'a MlsConnection` (lifetime reference to connection, allows access to services without parameter passing)

**Methods (connection available as self field, user passed as parameter):**
- `from_welcome_message(inviter, welcome_blob, ratchet_tree_blob, user, connection) -> Result<Self>`
  - **Key:** Instantiated when Welcome is received
  - Joins user to group
  - Stores group_id mapping
  - Stores connection reference for later use
  - Returns new MlsMembership ready for use
- `connect_to_existing_group(group_name, user, connection) -> Result<Self>`
  - Load existing group from storage (for reconnection)
  - Stores connection reference
- `send_message(&mut self, text, user: &MlsUser) -> Result<()>`
  - Uses self.connection internally
- `invite_user(&mut self, invitee_username, user: &MlsUser) -> Result<()>`
  - Uses self.connection internally
- `list_members(&self) -> Vec<String>`
- `process_incoming_message(&mut self, envelope, user: &MlsUser) -> Result<()>`
  - Uses self.connection internally
  - ApplicationMessage: decrypt and display
  - CommitMessage: process and merge
- Accessors: `get_group_name()`, `get_group_id()`

**Note on Lifetime Management:**
- MlsMembership holds `&'a MlsConnection` with a lifetime equal to the HashMap that contains it
- This is safe because MlsConnection owns both the HashMap and the lifetime of its contents
- Methods take `&MlsUser` as parameter (borrowed from MlsConnection::user)

### Ownership Hierarchy
```
MlsClient (public API - thin wrapper)
└── connection: MlsConnection (message hub & infrastructure)
    ├── Owns external services:
    │   ├── metadata_store: LocalStore
    │   ├── mls_provider: MlsProvider
    │   ├── api: ServerApi
    │   └── websocket: MessageHandler
    ├── Owns user identity:
    │   └── user: Option<MlsUser>
    │       ├── signature_key
    │       ├── credential_with_key
    │       └── identity
    └── Owns group memberships:
        └── memberships: HashMap<String, MlsMembership<'a>>
            For each group the user is in:
            ├── connection: &'a MlsConnection (reference to parent)
            ├── mls_group
            ├── group_id
            └── group_name
```

**Borrowing & Lifetime Pattern:**
- `MlsMembership<'a>` holds a reference to `MlsConnection` with lifetime `'a`
- `'a` equals the lifetime of the HashMap containing it (owned by MlsConnection)
- Safe because MlsConnection owns both the HashMap and guarantees the connection's lifetime
- Methods take `&MlsUser` as parameter (borrowed from `MlsConnection::user` field)
- No ownership cycles: MlsMembership can't be moved out of HashMap (held by reference)
- Each membership can access connection services without parameter passing

## Public API & Compatibility

### MlsClient Public Interface (Preserved for Backward Compatibility)
The refactoring maintains MlsClient's public API so `main.rs` and tests need minimal changes:

**Current API (as used in main.rs and tests):**
```rust
impl MlsClient {
    // Core lifecycle methods
    pub fn new_with_storage_path(url, username, group_name, storage_dir) -> Result<Self>
    pub async fn initialize() -> Result<()>
    pub async fn connect_to_group() -> Result<()>

    // Group operations (delegated to membership)
    pub async fn send_message(text) -> Result<()>
    pub async fn invite_user(username) -> Result<()>
    pub fn list_members() -> Vec<String>

    // Test helpers (preserved)
    pub fn get_identity() -> Option<&Identity>
    pub fn is_group_connected() -> bool
    pub fn has_signature_key() -> bool
    pub fn is_websocket_connected() -> bool
    pub fn get_username() -> &str
    pub fn get_group_name() -> &str
    pub fn get_provider() -> &MlsProvider
    pub fn get_api() -> &ServerApi
    pub fn get_group_id() -> Option<Vec<u8>>
}
```

**What Changes Internally (hidden from users):**
- Delegates to MlsConnection, MlsUser, MlsMembership
- `initialize()` now creates MlsUser and stores in connection
- `connect_to_group()` creates MlsMembership and stores in connection
- `send_message()`, `invite_user()`, `list_members()` delegate to current membership
- Control loop (formerly `run()`) moved to `cli.rs` to separate CLI from core logic

**Compatibility Impact:**
- ✅ `main.rs` - Minimal changes (calls `cli::run_client_loop()` instead of `client.run()`)
- ✅ `client_tests.rs` - No changes (tests individual methods, not run loop)
- ✅ `invitation_tests.rs` - No changes (tests individual methods, not run loop)
- ✅ E2E test - No changes (spawns client and sends input, still works)

### Usage Pattern Breakdown

**From main.rs (minimal changes):**
```rust
// Before: client.run() was the control loop
let mut client = MlsClient::new_with_storage_path(&url, &username, &group, &storage)?;
client.initialize().await?;
client.connect_to_group().await?;
client.run().await?;  // ← Was handling CLI and WebSocket messages

// After: control loop moved to cli.rs
let mut client = MlsClient::new_with_storage_path(&url, &username, &group, &storage)?;
client.initialize().await?;
client.connect_to_group().await?;
cli::run_client_loop(&mut client).await?;  // ← CLI handles event loop
```

**From tests (client_tests.rs):**
```rust
// Test helpers create MlsClient, then call methods
let client = MlsClient::new_with_storage_path(&url, &username, &group, &storage)?;
assert!(client.get_identity().is_none());
assert!(client.get_provider() != null);
```

**From tests (invitation_tests.rs):**
```rust
// Tests create clients, initialize, and interact
let mut alice = MlsClient::new_with_storage_path(&url, "alice", "group", &storage)?;
alice.initialize().await?;
alice.connect_to_group().await?;
alice.invite_user("bob").await?;
```

## Implementation Plan with Success Criteria

### Phase 1: Extract MlsUser

**Implementation:**
1. Create `src/mls/user.rs` - User identity module
   - Move from `client.rs`:
     - `identity: Option<Identity>`
     - `signature_key: Option<openmls_basic_credential::SignatureKeyPair>`
     - `credential_with_key: Option<openmls::prelude::CredentialWithKey>`
   - Extract methods:
     - Identity loading logic from `initialize()`
     - KeyPackage validation and registration logic
   - Create constructor: `MlsUser::new(username, identity, sig_key, credential_with_key)`
   - Methods: getters for identity material
   - No ownership of external services (LocalStore, MlsProvider, etc.)

**Success Criteria:**
- ✅ Code compiles without warnings
- ✅ MlsUser struct is properly defined with all required fields
- ✅ New unit tests exist:
  - `test_mls_user_creation` - Create MlsUser with valid identity
  - `test_mls_user_getters` - Verify all getters return correct values
  - `test_mls_user_signature_key_persistence` - Verify signature key is retained across operations
- ✅ Test command: `cargo test mls::user` passes with 3+ tests
- ✅ No MlsUser methods access external services (LocalStore, MlsProvider, ServerApi)
- ✅ Code review: MlsUser has clear, single responsibility

### Phase 2: Extract MlsMembership

**Implementation:**
1. Create `src/mls/membership.rs` - Group membership module
   - Move from `client.rs`:
     - `mls_group: Option<openmls::prelude::MlsGroup>`
     - `group_id: Option<Vec<u8>>`
     - `group_name: String`
   - Add field:
     - `connection: &'a MlsConnection` (lifetime reference)
   - Extract methods (updated signatures to use self.connection):
     - `send_message(&mut self, text, user: &MlsUser) -> Result<()>`
     - `invite_user(&mut self, invitee, user: &MlsUser) -> Result<()>`
     - `list_members(&self) -> Vec<String>`
     - `process_incoming_message(&mut self, envelope, user: &MlsUser) -> Result<()>`
   - New key constructors:
     - `from_welcome_message(inviter, welcome_b64, ratchet_b64, user, connection) -> Result<Self>`
       - Handles `handle_welcome_message()` logic
       - Returns new MlsMembership instance
     - `connect_to_existing_group(group_name, user, connection) -> Result<Self>`
       - Handles `connect_to_group()` logic for existing groups
   - No ownership of external services (accesses via self.connection)

**Success Criteria:**
- ✅ Code compiles without warnings
- ✅ MlsMembership<'a> struct properly defined with lifetime parameter
- ✅ All extracted methods work with borrowed connection reference
- ✅ New unit tests exist:
  - `test_membership_from_welcome_message` - Create membership from Welcome
  - `test_membership_connect_to_existing_group` - Load existing group
  - `test_membership_list_members` - List members returns correct data
  - `test_membership_send_message` - Message sending works with connection ref
- ✅ Test command: `cargo test mls::membership` passes with 5+ tests
- ✅ Lifetime errors resolved: MlsMembership lifetime tied to connection
- ✅ Integration test: `invitation_tests.rs` still passes (uses updated methods)
- ✅ Code review: Methods only access services through self.connection

### Phase 3: Create MlsConnection (Message Hub & Infrastructure)

**Implementation:**
1. Create `src/mls/connection.rs` - Infrastructure and message routing
   - Move from `client.rs`:
     - `server_url: String`
     - `username: String`
     - `metadata_store: LocalStore`
     - `mls_provider: MlsProvider`
     - `api: ServerApi`
     - `websocket: Option<MessageHandler>`
   - Add new fields:
     - `user: Option<MlsUser>` (initialized by `initialize()`)
     - `memberships: HashMap<String, MlsMembership>` (keyed by group_id as base64)
   - Move methods from `client.rs`:
     - `new_with_storage_path(url, username, dir) -> Result<Self>`
     - `initialize() -> Result<()>` - creates and stores MlsUser
     - `connect_websocket() -> Result<()>` - establishes WebSocket connection
   - Add new message routing methods:
     - `process_incoming_envelope(&mut self, envelope) -> Result<()>`
       - Matches on envelope type:
         - WelcomeMessage: creates new MlsMembership via `from_welcome_message()`
         - ApplicationMessage: finds membership by group_id, calls `process_incoming_message()`
         - CommitMessage: finds membership by group_id, calls `process_incoming_message()`
     - `next_envelope() -> Result<Option<MlsMessageEnvelope>>` - receive from websocket
   - Accessors: `get_user()`, `get_provider()`, `get_api()`, `get_username()`, `get_membership()`

**Success Criteria:**
- ✅ Code compiles without warnings
- ✅ MlsConnection struct properly integrates all infrastructure
- ✅ New unit tests exist:
  - `test_connection_initialization` - Create connection and initialize user
  - `test_connection_user_created` - User exists after initialization
  - `test_connection_membership_creation` - New membership created from Welcome
  - `test_connection_message_routing_welcome` - Welcome routed to membership creation
  - `test_connection_message_routing_app_message` - App message routed to correct membership
  - `test_connection_member_lookup_by_group_id` - Membership found by group_id
- ✅ Test command: `cargo test mls::connection` passes with 6+ tests
- ✅ Integration test: `client_tests.rs` tests pass (MlsConnection initialization works)
- ✅ Lifetime resolution: HashMap<String, MlsMembership<'a>> compiles with correct lifetime
- ✅ Message routing: All three envelope types handled correctly
- ✅ Code review: MlsConnection is clear message hub and orchestrator

### Phase 4: Refactor MlsClient as Core Logic Layer

**Implementation:**
1. Refactor `MlsClient` to core operations API (removes control loop):
   - Wraps MlsConnection internally
   - Removes `run()` method (moved to cli.rs)
   - Maintains operation methods:
     - Lifecycle: `new_with_storage_path()`, `initialize()`, `connect_to_group()`
     - Operations: `send_message()`, `invite_user()`, `list_members()`
   - Handles single-group case (stores selected_group_id for single CLI)
   - Methods delegate to connection/user/membership:
     ```
     client.send_message(text)
       → connection.memberships[selected_group_id]
         .send_message(text, user)
     ```
   - Preserves all test helpers: `get_identity()`, `is_group_connected()`, `get_provider()`, etc.

2. Extract control loop to `cli.rs`:
   - New function: `pub async fn run_client_loop(client: &mut MlsClient) -> Result<()>`
   - Implements tokio::select! for concurrent stdin and WebSocket I/O
   - Parses CLI commands (/invite, /list, /quit, messages)
   - Calls client operation methods
   - Displays messages using `format_message()`, `format_control()`
   - Manages user interaction (prompts, error display)
   - Notifies about new group memberships (from Welcome)
   - Updates selected_group_id when Welcome arrives

3. Update `main.rs`:
   - Change: `client.run().await?` → `cli::run_client_loop(&mut client).await?`
   - Add import: `use mls_chat_client::cli`
   - All other code remains identical

**Success Criteria:**
- ✅ Code compiles without warnings
- ✅ MlsClient has no run() method (moved to cli.rs)
- ✅ MlsClient has zero dependencies on CLI modules
- ✅ All public methods work correctly:
  - `new_with_storage_path()` - creates client with connection
  - `initialize()` - delegates to connection
  - `connect_to_group()` - delegates to connection
  - `send_message()` - delegates to selected membership
  - `invite_user()` - delegates to selected membership
  - `list_members()` - delegates to selected membership
- ✅ Test helpers still work:
  - `get_identity()`, `is_group_connected()`, `get_provider()`, `get_username()`, `get_group_name()`
- ✅ New unit tests exist:
  - `test_client_initialize_creates_user` - User created via connection
  - `test_client_connect_creates_membership` - Membership created via connection
  - `test_client_send_message_delegates` - Message delegated to membership
  - `test_client_operations_use_selected_group` - All operations use correct group
- ✅ Test command: `cargo test --lib client::` passes (existing tests still work)
- ✅ `cli::run_client_loop()` properly implements event loop
- ✅ `main.rs` compiles and uses new run_client_loop()
- ✅ All E2E test assertions still pass
- ✅ Code review: MlsClient is now purely operations, no UI logic

### Phase 5: Testing & Validation

**Implementation:**
1. Add new unit tests (in src/mls/mod.rs or separate test files):
   - MlsUser tests (3+ tests)
   - MlsMembership tests (5+ tests)
   - MlsConnection tests (6+ tests)
   - MlsClient tests (4+ tests)
   - cli::run_client_loop tests (3+ tests)

2. Update existing integration tests:
   - `client_tests.rs` - Verify MlsClient still works (no changes to test code)
   - `invitation_tests.rs` - Verify two-party and three-party invitations work
   - `message_processing_tests.rs` - Still tests low-level message decryption
   - `websocket_tests.rs` - Still tests WebSocket functionality
   - `api_tests.rs` - Still tests HTTP API functionality

3. Run all test suites:
   - Unit tests
   - Integration tests
   - E2E test

**Success Criteria:**
- ✅ Overall test results:
  - **Unit tests:** `cargo test --lib mls::` passes with 20+ tests
  - **Unit tests:** `cargo test --lib client::` passes (all existing tests)
  - **Integration tests:** `cargo test --test client_tests` passes with all tests
  - **Integration tests:** `cargo test --test invitation_tests` passes with all tests
  - **E2E tests:** E2E test passes completely (registration → invite → welcome → messages)

- ✅ Code coverage:
  - All new modules (mls::user, mls::membership, mls::connection) have unit tests
  - All public methods have at least one test
  - Critical paths (message routing, lifetime management) have dedicated tests

- ✅ Compilation:
  - `cargo build` succeeds with no warnings
  - `cargo check` shows no errors
  - All lifetime issues resolved

- ✅ Backward compatibility:
  - All existing test helpers still work
  - `client_tests.rs` tests unchanged (still pass)
  - `invitation_tests.rs` tests unchanged (still pass)
  - E2E test behavior identical to before

- ✅ Quality checks:
  - `cargo clippy` has no new warnings
  - No compiler warnings
  - Code documentation complete for all new modules

### Phase 6: Future - Multiple Groups (After Approval)

**Success Criteria (Optional, for future work):**
- ✅ CLI supports group switching
- ✅ `/groups list` command shows all memberships
- ✅ `/groups switch <name>` changes selected group
- ✅ Prompt shows current group: `[groupname]>`
- ✅ User can participate in multiple groups from single client session

---

## Success Criteria Summary Table

| Phase | Main Deliverable | Key Tests | Success Metric |
|-------|------------------|-----------|-----------------|
| **1** | MlsUser extraction | Unit: 3+ | `cargo test mls::user` passes |
| **2** | MlsMembership extraction | Unit: 5+ | `cargo test mls::membership` passes + `invitation_tests` passes |
| **3** | MlsConnection creation | Unit: 6+ | `cargo test mls::connection` passes + message routing verified |
| **4** | MlsClient refactoring + CLI extraction | Unit: 4+ + CLI: 3+ | `cargo test client::` passes + `cli::run_client_loop()` works + `main.rs` compiles |
| **5** | Complete testing & validation | Integration: 5+ | `cargo test --lib mls::` (20+ total) + `cargo test --test *` (all pass) + E2E passes |
| **Overall** | Full refactoring done | **27+ unit tests** | All phases pass + Zero compiler warnings + No clippy warnings |

**Final Success Criteria:**
- ✅ **Compilation:** `cargo build` succeeds with zero warnings
- ✅ **Tests:**
  - Unit tests: `cargo test --lib mls::` has 20+ passing tests
  - Integration tests: All existing tests still pass unchanged
  - E2E test: Full flow works (register → create → invite → welcome → message)
- ✅ **Architecture:**
  - MlsUser has no external service dependencies
  - MlsMembership only accesses services via self.connection
  - MlsConnection is clear message hub with routing logic
  - MlsClient is pure operations API, no UI logic
  - cli.rs owns all control flow and UI logic
- ✅ **Backward Compatibility:**
  - `main.rs` works with minimal changes (2 lines)
  - All existing tests pass without modification
  - E2E test behavior identical
  - Public API of MlsClient unchanged
- ✅ **Code Quality:**
  - No clippy warnings
  - Clear separation of concerns
  - All modules have documentation
  - Lifetime safety verified

## Adaptation Guide for Existing Code

### main.rs
**Current flow:**
```rust
let mut client = MlsClient::new_with_storage_path(&url, &username, &group, &storage)?;
client.initialize().await?;
client.connect_to_group().await?;
client.run().await?;  // ← Handles CLI loop
```

**After refactoring:**
```rust
use mls_chat_client::cli;

let mut client = MlsClient::new_with_storage_path(&url, &username, &group, &storage)?;
client.initialize().await?;
client.connect_to_group().await?;
cli::run_client_loop(&mut client).await?;  // ← CLI handles event loop
```

**Changes:**
- ✅ Add import: `use mls_chat_client::cli;`
- ✅ Change: `client.run().await?` → `cli::run_client_loop(&mut client).await?`
- Everything else identical

### client_tests.rs
**Current approach:**
```rust
let client = MlsClient::new_with_storage_path(&url, &username, &group, &storage)?;
assert!(client.get_identity().is_none());
assert!(!client.is_group_connected());
```

**After refactoring:**
- ✅ Test code stays mostly identical
- Test helpers: `create_test_client()` still returns `(MlsClient, TempDir)`
- Assertions: `get_identity()` now delegates to `connection.user`, `is_group_connected()` delegates to selected membership
- No changes needed to test structure

### invitation_tests.rs
**Current approach:**
```rust
let mut alice = MlsClient::new_with_storage_path(&url, "alice", "group", &storage)?;
alice.initialize().await?;
alice.connect_to_group().await?;
alice.invite_user("bob").await?;
```

**After refactoring:**
- ✅ Test code stays identical - no changes needed
- Internally: `invite_user()` delegates to membership.invite_user()
- No changes to test structure or assertions

### message_processing_tests.rs
**Current:** Tests via direct calls to `process_application_message()`

**After refactoring:**
- ✅ No changes needed
- Still tests low-level message processing functions directly
- Can optionally add tests through MlsMembership::process_incoming_message()

### websocket_tests.rs
**Current:** Tests via direct calls to `MessageHandler`

**After refactoring:**
- ✅ No changes needed
- Still tests WebSocket functionality directly
- Can optionally test message routing through MlsConnection

### api_tests.rs
**Current:** Tests via direct calls to `ServerApi`

**After refactoring:**
- ✅ No changes needed
- Still tests API functionality directly

### E2E test (test_welcome_routing.expect)
**Current:** Spawns client processes and sends commands

**After refactoring:**
- ✅ No changes needed
- Tests the CLI/user interface, which is preserved
- MlsClient's public interface unchanged

## Key Design Decisions

### Why Separate MlsConnection?
- **Message Hub:** Acts as central point accepting WebSocket messages and fanning out to relevant memberships
- **Infrastructure Owner:** Centralizes all external service management (LocalStore, MlsProvider, ServerApi, WebSocket)
- **Service Coordination:** Single point for initialize user, manage lifecycle, coordinate between user and memberships
- **Future Growth:** Enables easy support for multiple users or multiple connections

### Why MlsConnection Owns Memberships (HashMap)?
- **Clear Lifecycle:** Memberships are created when Welcome is received, destroyed when group is left
- **Message Routing:** Connection naturally routes incoming messages to appropriate membership by group_id
- **Consistent Ownership:** All group state owned by connection, not scattered
- **Future Ready:** HashMap enables easy iteration for features like "leave all groups" or "list all groups"

### Why Separate MlsUser?
- **Identity Invariant:** User identity doesn't change during session
- **Shared Across Groups:** Same signature_key and credential_with_key used for all groups
- **Clear Responsibility:** Only manages identity, doesn't touch group state or external services
- **Testability:** Can validate identity independently

### Why Separate MlsMembership?
- **Group State Encapsulation:** All group-specific state in one place (mls_group, group_id, group_name)
- **Operation Locality:** All group-specific operations (send, invite, list members) in one module
- **Independent Lifecycle:** Can join/leave groups without affecting others
- **Future Flexibility:** Can add per-group configuration (permissions, notifications, etc.)

### Why MlsMembership Keeps a Reference to MlsConnection?
- **Self-Contained:** Methods don't require connection as parameter
- **Cleaner Interface:** `membership.send_message(user, text)` is simpler than `membership.send_message(user, connection, text)`
- **Access to Services:** Can internally call `self.connection.get_provider()`, `self.connection.get_api()`, etc.
- **Natural Callback Pattern:** When routing messages, connection can call `membership.process_incoming_message(user, envelope)` without passing itself back
- **Safe Lifetime:** Membership lives inside HashMap owned by connection, so reference lifetime is guaranteed

### Why MlsMembership Takes User as Parameter (Not Stored)?
- **Avoid Multiple References:** Prevents multiple paths to same data
- **Correct Borrowing Model:** User is global to connection, membership is local to group
- **Method Clarity:** Explicit parameter shows that method needs identity information
- **Future Flexibility:** Enables different users in same group (future: group sharing)

### Why MlsMembership::from_welcome_message() Creates New Instance?
- **Constructor Pattern:** Welcome message brings user into group, so it creates new membership
- **Clear Intent:** Method name signals that this is group creation point, not just message handling
- **Consistency:** Parallels `connect_to_existing_group()` for reconnection scenarios
- **Single Responsibility:** Welcome handling is self-contained, returns ready-to-use membership

### Why Extract run() to cli.rs?
- **Separation of Concerns:** Control loop is UI/CLI logic, not core MLS logic
- **Testability:** Core operations (send_message, invite_user, etc.) can be tested without CLI
- **Flexibility:** Can create different UIs (TUI, GUI) by writing different run loop functions
- **No UI Dependencies in Core:** MlsClient, MlsConnection, MlsUser, MlsMembership have no knowledge of CLI
- **Clear Responsibilities:** cli.rs owns command parsing, prompts, event loop; client.rs owns operations
- **Single Responsibility:** MlsClient is now purely about MLS operations, not control flow

## Files to Create/Modify

**New files:**
- `client/rust/src/mls/mod.rs` - Module organization, re-exports
- `client/rust/src/mls/user.rs` - MlsUser struct and methods
- `client/rust/src/mls/membership.rs` - MlsMembership struct and methods
- `client/rust/src/mls/connection.rs` - MlsConnection struct, message routing, orchestration

**Modified files:**
- `client/rust/src/client.rs` - Refactor MlsClient to core operations layer, remove run() method
- `client/rust/src/cli.rs` - Add control loop function: `pub async fn run_client_loop(client: &mut MlsClient)`
- `client/rust/src/lib.rs` - Update module organization (change from single file to mls module)
- `client/rust/src/main.rs` - Change `client.run().await?` to `cli::run_client_loop(&mut client).await?`

**Updated existing modules (no structural changes):**
- `client/rust/src/message_processing.rs` - Keep as-is, tested through MlsMembership
- `client/rust/src/crypto.rs` - Keep as-is, called by membership methods
- `client/rust/src/api.rs` - Keep as-is, called by connection
- `client/rust/src/websocket.rs` - Keep as-is, used by connection
- `client/rust/src/identity.rs` - Keep as-is, called by connection during initialize()
- `client/rust/src/provider.rs` - Keep as-is, owned by connection
- `client/rust/src/storage.rs` - Keep as-is, owned by connection
- `client/rust/src/extensions.rs` - Keep as-is, used by membership
- `client/rust/src/error.rs` - Keep as-is
- `client/rust/src/models.rs` - Keep as-is
- `client/rust/src/cli.rs` - Keep as-is

**Test updates:**
- `client/rust/tests/client_tests.rs` - Refactor to test MlsConnection instead of MlsClient
- `client/rust/tests/invitation_tests.rs` - Test MlsMembership::from_welcome_message()
- `client/rust/tests/message_processing_tests.rs` - Integrate with MlsMembership
- `client/rust/tests/websocket_tests.rs` - Test through MlsConnection
- `client/rust/tests/api_tests.rs` - No changes (still tests ServerApi)
- E2E test: No changes needed (public interface preserved)

## Rationale for Alternative Approaches Not Chosen

### Alternative 1: Keep MlsClient monolithic, add group HashMap
**Rejected because:**
- Doesn't separate concerns (identity, infrastructure, group state mixed)
- Hard to test individual components
- Harder to enforce that signature_key/credential aren't duplicated
- Complicates the run() loop and message routing

### Alternative 2: Group state as separate struct, still in MlsClient
**Rejected because:**
- Still mixes user identity with group operations
- Doesn't improve testability
- Less clear ownership model

### Alternative 3: Trait-based abstraction layer
**Rejected because:**
- Over-engineering for current needs
- Harder to debug
- Would delay multi-group support without clarity improvement

## High-Level Summary

The refactoring separates `MlsClient` into three focused components:

- **MlsConnection** (Message Hub & Infrastructure)
  - Owns WebSocket, storage, crypto provider, API client
  - Accepts incoming messages and routes to relevant memberships
  - Initializes and owns user identity
  - Owns HashMap of group memberships
  - Coordinates lifecycle of all components

- **MlsUser** (Identity)
  - Holds signature_key, credential_with_key, identity metadata
  - Created by connection during initialize()
  - Shared by reference across all memberships
  - No external service dependencies

- **MlsMembership** (Group Session)
  - Holds group_id, group_name, mls_group state
  - Created from Welcome message or loaded from storage
  - Receives borrowed references to user and connection as needed
  - Implements group-specific operations (send, invite, list)
  - Can be destroyed independently

## Message Routing Flow

```
Server → WebSocket → MlsConnection::next_envelope()
                           ↓
                  MlsConnection::process_incoming_envelope()
                           ↓
                   match envelope {
                       WelcomeMessage → MlsMembership::from_welcome_message()
                                      → insert into memberships HashMap
                       ApplicationMessage/CommitMessage → find membership by group_id
                                                        → membership.process_incoming_message()
                   }
```

## Phase 4 Architectural Decisions - Critical Review

During Phase 4 implementation planning, Agent A identified three critical architectural decisions that require clarification. These decisions affect message flow from WebSocket → MlsConnection → MlsMembership → cli.rs for display.

### Decision 1: Message Display Strategy

**Problem:** After MlsMembership processes an incoming ApplicationMessage and decrypts it, how should the decrypted text reach cli.rs for display?

**Option A: Membership Returns Data, cli.rs Handles Display ✅ RECOMMENDED**

```rust
// In MlsMembership: return decrypted data
pub async fn process_application_message(
    &mut self,
    sender: &str,
    encrypted_content: &str,
    user: &MlsUser,
) -> Result<(String, String)> {  // Returns (sender, decrypted_text)
    // ... decrypt message ...
    Ok((sender.to_string(), decrypted_text))
}

// In cli.rs: receive and display
match envelope {
    ApplicationMessage { group_id, sender, encrypted_content } => {
        if let Some(membership) = connection.get_membership_mut(&group_id) {
            if let Ok((sender, text)) = membership.process_application_message(...) {
                println!("{}", format_message(&group_name, &sender, &text));
            }
        }
    }
}
```

**Advantages:**
- Clean separation: membership handles crypto/state, cli handles UI
- MlsConnection and MlsMembership have ZERO UI dependencies
- Easy to add alternative UIs (TUI, GUI, REST API) later
- Matches current code pattern (client.rs:1025)
- Follows single responsibility principle
- Better testability (membership tests don't need to capture output)

**Disadvantages:**
- More logic in cli.rs event loop
- More return types to coordinate

---

**Option B: Membership Handles Display ✗ NOT RECOMMENDED**

```rust
// In MlsMembership: print directly
pub async fn process_application_message(...) -> Result<()> {
    // ... decrypt ...
    println!("{}", format_message(&self.group_name, &sender, &text));
    Ok(())
}
```

**Disadvantages:**
- MlsMembership imports cli module (circular dependency risk)
- Couples core logic to UI details
- Hard to add alternative UIs
- Violates "core logic has zero UI dependencies"
- Makes unit testing harder

---

### Decision 2: MlsConnection's Role in Message Processing

**Problem:** How should MlsConnection coordinate with MlsMembership when processing messages?

**Option A: cli.rs Coordinates - Calls Both next_envelope() and process_incoming_message() ✅ RECOMMENDED**

```rust
// In cli.rs event loop:
let envelope = connection.next_envelope().await?;

match envelope {
    ApplicationMessage { group_id, sender, encrypted_content } => {
        // Update state through connection
        connection.process_incoming_envelope(&envelope).await?;

        // Display based on decrypted message
        if let Some(membership) = connection.get_membership_mut(&group_id) {
            if let Ok((sender, text)) = membership.process_application_message(...).await {
                println!("{}", format_message(&group_name, &sender, &text));
            }
        }
    }
    WelcomeMessage { inviter, ... } => {
        // Update state (creates new membership in connection)
        connection.process_incoming_envelope(&envelope).await?;

        // Display welcome
        let membership = connection.get_newest_membership();
        println!("Joined group: {}", membership.get_group_name());
    }
    CommitMessage { group_id, sender, ... } => {
        // Update state
        connection.process_incoming_envelope(&envelope).await?;

        // Display notification
        println!("{}", format_control(&group_name, &format!("{} updated group", sender)));
    }
}
```

**Advantages:**
- MlsConnection focuses on state updates, not display coordination
- cli.rs owns event loop and display logic (clear separation)
- Minimal changes to Phase 3's connection.rs
- Information flow is explicit: receive → process → display
- Easier to extend for multi-group (multiple groups in single loop)
- Better debugging (each step is visible in cli.rs)

**Disadvantages:**
- More boilerplate in cli.rs
- Some pattern repetition (receive, then process)

---

**Option B: MlsConnection Returns Structured Display Information ✗ NOT RECOMMENDED**

```rust
pub enum ProcessedMessage {
    TextMessage { sender: String, group_name: String, text: String },
    NewGroup { group_name: String, inviter: String },
    MembershipUpdate { group_name: String, action: String },
}

impl MlsConnection {
    pub async fn next_envelope_processed(&mut self) -> Result<Option<ProcessedMessage>> {
        // ... process and return display info ...
    }
}
```

**Disadvantages:**
- MlsConnection couples itself to display concerns
- Return types must expand as UI needs change
- Hides processing logic (harder to debug)
- Violates separation of concerns
- Complicates Phase 3's routing

---

### Decision 3: Selected Group Tracking

**Problem:** For single-group CLI, where should we track which group the user is in?

**Option A: Track in Both - MlsClient Tracks ID, cli.rs Tracks Name ✅ RECOMMENDED**

```rust
// MlsClient.rs
pub struct MlsClient {
    connection: MlsConnection,
    selected_group_id: Option<Vec<u8>>,  // Used for operations
}

impl MlsClient {
    pub fn send_message(&mut self, text: &str) -> Result<()> {
        let user = self.connection.get_user().ok_or(...)?;
        let membership = self.connection.get_membership_mut(
            self.selected_group_id.as_ref().ok_or(...)?
        ).ok_or(...)?;
        // Delegate to membership operation
        membership.send_message(text, user, ...)?;
        Ok(())
    }
}

// cli.rs
pub async fn run_client_loop(client: &mut MlsClient) -> Result<()> {
    let mut current_group_name = client.get_group_name().to_string();

    // ... event loop ...

    if let WelcomeMessage { ... } = envelope {
        // Update display name
        current_group_name = connection.get_newest_membership().get_group_name().to_string();
    }
}
```

**Advantages:**
- Clear division of concerns: MlsClient tracks functional state (which group), cli.rs tracks display state (names)
- MlsClient operations (send_message) can find membership efficiently by ID
- cli.rs shows human-readable names
- Easy to extend to multi-group: track multiple selected_group_ids
- Matches current single-group CLI model
- Allows operations to work independently

**Disadvantages:**
- Two places track "current group"
- Must keep ID and name in sync

---

**Option B: Track Only in MlsClient ✗ LESS RECOMMENDED**

```rust
pub struct MlsClient {
    connection: MlsConnection,
    selected_group_id: Option<Vec<u8>>,
    selected_group_name: String,  // Also track name here
}
```

**Disadvantages:**
- MlsClient couples data from different scopes
- More state to synchronize
- Blurs responsibility lines

---

**Option C: Track Only in cli.rs ✗ LESS RECOMMENDED**

Requires passing group_id to every operation, breaking current API.

---

## Summary of Recommendations

For Phase 4 to proceed with clean architecture, I recommend:

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Message Display** | **Option A** | Clean separation, zero UI in core, testable, matches current code |
| **Connection Role** | **Option A** | Keeps connection focused on state, minimal Phase 3 changes |
| **Group Tracking** | **Option A** | Efficient operations, clean display, multi-group ready |

These choices will result in:
- ✅ Core logic (MlsConnection, MlsMembership, MlsUser, MlsClient) with zero UI dependencies
- ✅ CLI logic fully contained in cli.rs
- ✅ Easy to add alternative UIs (TUI, GUI, REST API)
- ✅ Clean information flow: data → process → display
- ✅ Extensible to multi-group support

---

## Phase 4 Architectural Decisions - APPROVED

User has approved the architectural approach for Phase 4:

### Approved Decision 1: Message Display Strategy
✅ **APPROVED: Option A** - Membership returns data, cli.rs handles display

**Implementation:**
- MlsMembership methods return `(sender: String, text: String)` for ApplicationMessage
- cli.rs receives result and calls `format_message()` for display
- Core modules have zero UI dependencies

### Approved Decision 2: MlsConnection's Role
✅ **APPROVED: Option A** - cli.rs coordinates (calls next_envelope + process + display)

**Implementation:**
- cli.rs receives envelope via `connection.next_envelope()`
- cli.rs calls `connection.process_incoming_envelope(envelope)` to update state
- cli.rs handles display based on envelope type
- MlsConnection focuses on state management only

### Approved Decision 3: Group Tracking Strategy
✅ **APPROVED: Custom approach** - MlsClient provides method, cli.rs calls it

**Implementation:**
```rust
// In MlsClient
impl MlsClient {
    pub fn get_current_group_name(&self) -> Result<String> {
        let group_id = self.selected_group_id.as_ref()
            .ok_or(ClientError::NoGroupSelected)?;
        let membership = self.connection.get_membership(group_id)
            .ok_or(ClientError::GroupNotFound)?;
        Ok(membership.get_group_name().to_string())
    }
}

// In cli.rs
let group_name = client.get_current_group_name()?;
println!("{}", format_message(&group_name, &sender, &text));
```

**Advantages:**
- Cleaner separation: MlsClient owns all state management
- cli.rs asks client for display information (single source of truth)
- No duplication between MlsClient and cli.rs
- Easy to maintain (only one place to update group name)
- More extensible (can add methods like `get_current_group_id()` as needed)

---

## Current Status

**Phases 1-3:** ✅ COMPLETE (73/73 tests passing, 0 warnings)

**Phase 4:** ✅ COMPLETE (71/71 tests passing, 0 warnings, clippy clean)

**Phase 5:** ▶️ PENDING

**Phase 6 (Future):** ▶️ PENDING - Add multi-group CLI support

**Next Steps:**
1. ▶️ Phase 5: Update and validate all integration tests
2. ▶️ Phase 6: Add multi-group CLI support (/groups list, /groups switch)

## Phase 4 Implementation Progress

**Started:** 2025-10-27

### Critical Architecture Clarification - RESOLVED

**Question:** How should cli.rs access MlsConnection?

**APPROVED ANSWER: Option 1** - Add accessor methods to MlsClient

```rust
impl MlsClient {
    pub fn get_connection(&self) -> &MlsConnection {
        &self.connection
    }

    pub fn get_connection_mut(&mut self) -> &mut MlsConnection {
        &mut self.connection
    }
}
```

**Usage in cli.rs:**
```rust
pub async fn run_client_loop(client: &mut MlsClient) -> Result<()> {
    loop {
        tokio::select! {
            // ... stdin branch ...

            // WebSocket branch
            envelope = client.get_connection_mut().next_envelope().await => {
                // Process envelope
                client.get_connection_mut().process_incoming_envelope(envelope).await?;

                // Display
                let membership = client.get_connection().get_membership(&group_id)?;
                // ...
            }
        }
    }
}
```

**Rationale:**
- Cleaner interface for cli.rs (single object to pass around)
- MlsClient retains full control and encapsulation
- Matches existing pattern (test helpers like `get_provider()`, `get_api()`)
- No need to destructure in main.rs
- Follows single responsibility: cli.rs doesn't need to know about internals

### Implementation Log

**Phase 4: MlsClient Refactoring - Started 2025-10-28**

Agent A beginning implementation of Phase 4 with approved architecture:
- Message Display Strategy: Option A (membership returns data, cli.rs displays)
- Connection Role: Option A (cli.rs coordinates - calls next_envelope + process + display)
- Group Tracking: Custom approach (MlsClient provides `get_current_group_name()` method)
- MlsConnection Access: Accessor methods (`get_connection()`, `get_connection_mut()`)

**Tasks:**
1. ✅ Refactor MlsClient struct (connection + selected_group_id)
2. ✅ Implement MlsClient methods (delegation to connection/membership)
3. ✅ Extract run() to cli::run_client_loop()
4. ✅ Update main.rs (2-line change)
5. ✅ Create unit tests (5+ tests in client.rs)

**Progress:**
- ✅ Refactored MlsClient to wrap MlsConnection
- ✅ Added selected_group_id tracking
- ✅ Implemented all delegation methods: initialize(), connect_to_group(), send_message(), invite_user(), list_members()
- ✅ Added get_current_group_name() per approved architecture
- ✅ Added get_connection() and get_connection_mut() accessors for cli.rs
- ✅ All test helpers updated and working
- ✅ Added 5 unit tests (test_client_creation, test_client_initialization, test_client_connect_to_group, test_client_operations_delegate, test_client_get_current_group_name)
- ✅ Code compiles with zero warnings
- ✅ Added helper methods to MlsConnection (add_membership, get_newest_membership, send_message_to_group, invite_user_to_group, get_websocket)
- ✅ Created pub async fn run_client_loop(client: &mut MlsClient) in cli.rs
- ✅ Implemented tokio::select! loop with stdin and WebSocket branches
- ✅ Updated main.rs to call cli::run_client_loop() instead of client.run()
- ✅ Fixed unused variable warning in connection.rs test
- ✅ All 71 library tests passing, 0 warnings, clippy clean

**Architecture Notes:**
- Solved borrow-checker issues by adding helper methods to MlsConnection that encapsulate the complex borrowing (send_message_to_group, invite_user_to_group)
- This allows MlsClient to delegate cleanly without managing multiple borrows
- MlsConnection now has all methods needed for Phase 4

## Phase 4 Completion Summary

**Completed: 2025-10-28**

### Files Modified

1. **client/rust/src/cli.rs**
   - Added `pub async fn run_client_loop(client: &mut MlsClient)` function
   - Implements tokio::select! loop with:
     - stdin branch: reads user commands, parses via parse_command(), executes via client methods
     - WebSocket branch: receives envelopes, calls client.get_connection_mut().process_incoming_envelope()
   - Handles routing, error logging, user feedback (eprintln, println)

2. **client/rust/src/main.rs**
   - Added `cli` to use statement: `use mls_chat_client::{client::MlsClient, cli, Result};`
   - Changed `client.run().await?` to `cli::run_client_loop(&mut client).await?`

3. **client/rust/src/mls/connection.rs**
   - Fixed unused variable warning: renamed `storage_dir` to `_storage_dir` in test

### Test Results
- ✅ 71/71 library tests passing
- ✅ 0 compilation warnings
- ✅ Clippy: Clean
- ✅ Code compiles successfully

### Architecture Validation
- ✅ Separation of concerns: Core logic in connection/membership, CLI logic in cli.rs
- ✅ Delegation pattern works cleanly with accessor methods
- ✅ Message routing via connection.process_incoming_envelope() to memberships
- ✅ CLI controls flow: reads input, processes via client, displays results
- ✅ Borrow checker satisfied with current design

### Success Criteria Met
- ✅ cli::run_client_loop() created and fully functional
- ✅ Concurrent I/O event loop with tokio::select!
- ✅ User input handling (commands, messages)
- ✅ Incoming message processing via connection routing
- ✅ Error handling with logging and user feedback
- ✅ All tests passing
- ✅ Zero warnings
- ✅ Code quality: Clean clippy output
