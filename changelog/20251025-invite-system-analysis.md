# Invite System and Credential Handling Analysis

**Date:** 2025-10-25  
**Scope:** Investigation of invite functionality, error handling, and OpenMLS API usage

## Task Specification

Comprehensive search and analysis of:
1. Where `/invite` command is handled
2. Location of "Duplicate signature key in proposals and group" error
3. How invites are processed and MLS group state updated
4. Credential and key package management during invites
5. OpenMLS API calls for credentials and key packages

## Key Findings

### 1. Invite Command Handling

**Location:** `/home/kena/src/quintessence/mls-chat/client/rust/src/cli.rs`
- **Lines 80-85:** Command parsing for `/invite` commands
- **Test function:** `test_parse_invite_command()` at lines 65-68 validates parsing

**How it works:**
- User types `/invite alice` which is parsed by `Command::parse()` method
- Returns `Command::Invite(username)` variant containing the invitee username

**Main invite logic:** `/home/kena/src/quintessence/mls-chat/client/rust/src/client.rs`
- **Function:** `invite_user()` at lines 384-502
- **Called from:** Main loop in `run()` method (line 896)

### 2. Duplicate Signature Key Error

**Location:** `/home/kena/src/quintessence/mls-chat/openmls/openmls/src/group/errors.rs`
- **Line 444-445:** Error definition in `ProposalValidationError` enum
- **Definition:**
  ```rust
  #[error("Duplicate signature key in proposals and group.")]
  DuplicateSignatureKey,
  ```
- **Context:** Part of proposal validation errors that occur during MLS commit/proposal processing
- **Triggers when:** OpenMLS detects that a signature key appears both in pending proposals AND in the existing group member list

### 3. Invite Processing Flow

The complete invite workflow (lines 384-502 in client.rs):

#### Step 1: Verify Invitee Exists (lines 397-407)
```rust
let invitee_key_package_bytes = self.api.get_user_key(invitee_username).await?;
```
- Fetches the invitee's KeyPackage from the server
- Ensures the invitee exists and is registered

#### Step 2: Validate KeyPackage (lines 409-419)
```rust
let invitee_key_package_in = 
    KeyPackageIn::tls_deserialize(&mut &invitee_key_package_bytes[..])?;

let invitee_key_package = invitee_key_package_in
    .validate(self.mls_provider.crypto(), ProtocolVersion::Mls10)?;

self.validate_key_package_security(&invitee_key_package)?;
```
- Deserializes TLS-encoded KeyPackage from server response
- OpenMLS built-in validation: signature verification, protocol version, key distinction, lifetime
- Custom validation: ciphersuite compatibility, credential identity content

#### Step 3: Add Member to Group (lines 421-432)
```rust
let (commit_message, welcome_message, _group_info) = crypto::add_members(
    group,
    &self.mls_provider,
    sig_key,
    &[&invitee_key_package],
)?;

crypto::merge_pending_commit(group, &self.mls_provider)?;
```
- **OpenMLS `add_members()` API call** at `crypto.rs` lines 125-142
- Generates: Commit message, Welcome message, GroupInfo
- **Merge required** before sending messages (critical fix noted in code)

#### Step 4: Export Ratchet Tree (line 435)
```rust
let ratchet_tree = crypto::export_ratchet_tree(group);
```
- Exports the group's key tree for new member
- Used in Welcome processing by joining member

#### Step 5: Send Welcome Message (lines 437-461)
- Serializes Welcome + ratchet tree using TLS codec
- Sends directly to invitee via WebSocket (not broadcast)
- Envelope type: `WelcomeMessage` (no group_id field)

#### Step 6: Broadcast Commit (lines 466-487)
- Broadcasts Commit message to all existing members
- Serialized as `CommitMessage` envelope
- Allows existing members to update their group state

### 4. Credential Handling

**Initialization Phase** (`initialize()` at client.rs lines 107-196):
- Creates/loads persistent identity via `IdentityManager::load_or_create()`
- Stores signature key in OpenMLS provider storage (can be reused across groups)
- Stores public key in metadata database for recovery

**Identity Manager** (`identity.rs` lines 23-128):
- **`load_or_create()`:** Loads existing or creates new identity
  - Checks metadata store for public key
  - Uses public key to retrieve signature key from OpenMLS storage
  - Generates new if missing
- **Persistent storage:** Two-level system
  - OpenMLS provider: Stores signature keys with `SignatureKeyPair::store()`
  - LocalStore (metadata): Stores public key blob for recovery

**Critical design:** Same `credential_with_key` is reused across all groups for a single user (line 129-136 in client.rs)

### 5. Key Package Management

**Generation** (`crypto.rs` lines 34-52):
```rust
pub fn generate_key_package_bundle(
    credential: &CredentialWithKey,
    signer: &SignatureKeyPair,
    provider: &impl OpenMlsProvider,
) -> Result<KeyPackageBundle>
```
- Uses stored credential and signature key
- Builds with specified ciphersuite: `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`
- Returns `KeyPackageBundle` containing the signed `KeyPackage`

**Registration** (`client.rs` lines 143-193):
- Fetches existing key package from server (if user previously registered)
- Validates it's compatible with local identity
- Generates new one if doesn't exist
- Registers with server (idempotent - 409 duplicate OK)

**Validation** (`client.rs` lines 700-768):
Custom security checks beyond OpenMLS validation:
- Ciphersuite compatibility with group
- Credential identity non-empty
- Credential type is BasicCredential
- Lifetime validation

### 6. OpenMLS API Calls

**Key credential/key package OpenMLS calls:**

| Function | File:Lines | Purpose |
|----------|-----------|---------|
| `SignatureKeyPair::new()` | crypto.rs:18 | Generate signature keys |
| `SignatureKeyPair::store()` | identity.rs:110 | Persist signature key |
| `SignatureKeyPair::read()` | identity.rs:57 | Load persisted signature key |
| `KeyPackageIn::tls_deserialize()` | client.rs:410 | Deserialize received KeyPackage |
| `KeyPackageIn::validate()` | client.rs:414 | Validate KeyPackage structure |
| `KeyPackage::builder()` | crypto.rs:42 | Create new KeyPackage |
| `MlsGroup::add_members()` | crypto.rs:134 | Add invitee to group |
| `group.merge_pending_commit()` | crypto.rs:179 | Apply add_members commit |
| `StagedWelcome::new_from_welcome()` | crypto.rs:159 | Process Welcome message |
| `BasicCredential::new()` | crypto.rs:15 | Create credential |
| `BasicCredential::try_from()` | client.rs:639 | Extract credential from member |

### 7. MLS Group State Updates

**During invite operation:**
1. **Pending state:** After `add_members()`, group has pending commit
2. **Merge required:** `merge_pending_commit()` applies the changes:
   - Updates epoch counter
   - Extends ratchet tree with new leaf node
   - Derives new group secrets
3. **Persistence:** OpenMLS provider automatically persists state to storage

**For joining member (Welcome processing):**
- `process_welcome_message()` (crypto.rs lines 144-172)
- Extracts Welcome from MLS message
- Creates `StagedWelcome` with ratchet tree
- Converts to `MlsGroup` - member now has full group state
- Stores group ID mapping in metadata

## Important Implementation Details

### Credential Reuse Across Groups
- Single user has one credential (based on username)
- `credential_with_key` is cached and reused (client.rs line 34)
- Same signature key used for all group operations
- Prevents "Duplicate signature key" errors by avoiding re-adding same user to group

### Welcome Message Structure
- **No group_id field** in envelope (by design)
- Group name is encrypted in GroupContext extensions
- Extracted during `handle_welcome_message()` (lines 591-598)
- Provides authoritative group identity from encrypted metadata

### Commit Merging Critical Fix
- Must merge pending commit before sending other messages (line 432)
- Documented in test at lines 1280-1384
- Without merge, subsequent operations may fail with stale group state

## Potential Issues to Watch

1. **DuplicateSignatureKey error:** Would occur if:
   - Same user tries to be added to group twice
   - Signature key validation fails during proposal processing
   - Credential identity mismatch between proposals and group

2. **KeyPackage validation:** Strict checks on:
   - Ciphersuite mismatch (will cause add to fail)
   - Credential type must be BasicCredential
   - Signature must be valid (OpenMLS check)

3. **Storage persistence:** 
   - Two-level system (OpenMLS + LocalStore) must stay in sync
   - Group ID mapping critical for reconnection

## Files Modified/Relevant
- `/home/kena/src/quintessence/mls-chat/client/rust/src/client.rs` - Main invite logic
- `/home/kena/src/quintessence/mls-chat/client/rust/src/crypto.rs` - OpenMLS API wrappers
- `/home/kena/src/quintessence/mls-chat/client/rust/src/identity.rs` - Credential persistence
- `/home/kena/src/quintessence/mls-chat/client/rust/src/cli.rs` - Command parsing
- `/home/kena/src/quintessence/mls-chat/openmls/openmls/src/group/errors.rs` - Error definitions

## Root Cause Analysis: Duplicate Signature Key Error

### Problem Reproduction
Successfully reproduced the error using expect script with two clients:
1. Bob starts first: creates client, registers identity, creates mygroup
2. Alice starts second: creates client, registers identity, creates mygroup
3. Alice attempts `/invite bob`
4. Error: `Duplicate signature key in proposals and group`

### Root Cause: Shared MLS Storage Database

**The Bug:** Both Alice and Bob are using the same `mls.db` file at `~/.mlschat/mls.db`

When users are created sequentially in separate processes, they share the same OpenMLS provider storage database:

```
MlsClient::new_with_storage_path() [client.rs:72]
  ↓
  let mls_db_path = storage_dir.join("mls.db");  // Line 86
  let mls_provider = MlsProvider::new(&mls_db_path)?;  // Line 87
```

The storage directory is hardcoded to `~/.mlschat`, so both users share the same database.

### Why This Causes the Error

1. **Bob's initialization:**
   - Creates credential with signature key for "bob"
   - Signature key stored in shared `mls.db` via `SignatureKeyPair::store()`
   - Creates group, stores group state in shared `mls.db`

2. **Alice's initialization:**
   - Creates credential with signature key for "alice"
   - Signature key stored in same shared `mls.db`
   - Creates new group instance, stores in same `mls.db`

3. **Alice invites Bob:**
   - Calls `add_members(bob_key_package)`
   - OpenMLS tries to add Bob's key package to the group
   - When validating the proposal, OpenMLS checks the shared `mls.db`
   - Finds Bob's signature key already exists from step 1
   - Error: "Duplicate signature key in proposals and group"

### The Design Intent vs Implementation

The separate-groups-per-user design is correct, but storage isolation is missing:
- ✓ Each user has their own identity/credential
- ✓ Each user creates their own group instance
- ✗ Both users share the same MLS storage database

This violates OpenMLS storage assumptions where:
- Signature keys should be unique per user context
- The storage provider maintains credential and key state per identity

## Fix Implementation

### Change Made
**File:** `/home/kena/src/quintessence/mls-chat/client/rust/src/client.rs` (lines 85-88)

```rust
// Before (line 86):
let mls_db_path = storage_dir.join("mls.db");

// After (lines 86-88):
// Use per-user database to isolate credentials and group state
let mls_db_path = storage_dir.join(format!("mls-{}.db", username));
```

### Why This Fixes It
- Each user now gets an isolated MLS storage database: `mls-alice.db`, `mls-bob.db`, etc.
- OpenMLS signature keys are no longer shared across users
- When Alice tries to add Bob, OpenMLS doesn't see Bob's signature key in her isolated database
- The `DuplicateSignatureKey` error no longer occurs

### Storage Structure After Fix
```
~/.mlschat/
  ├── metadata.db          (shared application state - user mappings)
  ├── mls-alice.db         (Alice's OpenMLS credentials and groups)
  └── mls-bob.db           (Bob's OpenMLS credentials and groups)
```

### Test Results
Successfully reproduced and fixed the issue:

**Before Fix:**
```
[ERROR] Failed to invite bob: MLS error: OpenMLS error: Duplicate signature key in proposals and group.
```

**After Fix:**
```
[INFO] Sent Welcome message to bob (ratchet tree included)
[INFO] Broadcast Commit message to existing members
[INFO] Invited bob to the group
```

Both clients now have separate MLS databases with isolated credentials and group states.

## Files Modified
- `client/rust/src/client.rs` - Added per-user database naming

## Current Status
✅ FIXED - Invite now works correctly between two independent clients. Each user maintains isolated MLS storage while sharing application-level metadata.
