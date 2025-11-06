# KeyPackage Pool Implementation Plan (OpenMLS-Aligned)

**Date:** 2025-11-05
**Phase:** Phase 2 (Production-Ready Implementation, Revised)
**Status:** Planning
**Replaces:** `changelog/20251028-keypackage-pool-implementation-plan.md` (DO NOT READ THIS)

## Overview

This is a revised implementation plan for migrating from a single-key-package architecture to a **pool-based strategy** as specified in `docs/keypackage-pool-strategy.md`.

**Key architectural change from previous plan:** This version leverages **OpenMLS StorageProvider** for KeyPackageBundle persistence instead of duplicating storage in LocalStore. This ensures:

- Proper separation of concerns (crypto storage = OpenMLS, state tracking = us)
- No data duplication (~40% storage savings per pool)
- Alignment with OpenMLS's forward-secrecy guarantees
- Single source of truth for key material

**Status of prior phases:**
- ✅ Phase 1: Error handling fixes (completed, preserved)
- ✅ Phase 2.0: Server-side KeyPackage storage (completed, preserved)
- ⏸️ Phase 2.1-2.2: Previous implementation (reverted due to architecture misalignment)
- 🔄 Phase 2.1+: Reimplemented with correct design (this plan)

## Task Specification

**Goal:** Implement a KeyPackage pool management system that:

1. Generates and uploads multiple KeyPackages (32 target) during initialization
2. Tracks pool state (available, reserved, expired counts) via LocalStore metadata
3. Maintains expiry bounds (~7-14 days)
4. Enables background replenishment when pool < 25% (8 keys)
5. Exposes pool health status to users
6. Integrates with server reservation/spend tracking

**Scope:** Rust client implementation (building on Phase 2.0 server changes)

**Depends On:**
- Phase 1 completion (error handling fixes) ✅
- Phase 2.0 completion (server KeyPackageStore) ✅

## Ancillary documentation

The OpenMLS implementation is available in subdirectory `openmls/openmls/` and the user manual is in `openmls/book/src/user_manual/`.
You can consult them for reference.

## Architecture Overview

### Key Architectural Decisions

Based on the **StorageProvider Analysis** (see `changelog/20251105-storageprovider-analysis.md`):

1. **KeyPackageBundle Storage:** Use OpenMLS StorageProvider (automatic)
   - When `KeyPackageBuilder::build()` is called, OpenMLS automatically calls `StorageProvider::write_key_package()`
   - Complete bundle (public + both private keys) is persisted automatically
   - No manual serialization/deserialization needed
   - We do NOT duplicate storage in LocalStore

2. **Pool Metadata Storage:** Add lightweight metadata-only table to LocalStore
   - Table: `keypackage_pool_metadata`
   - Fields: keypackage_ref (reference only), status, timestamps, not_after
   - Does NOT store keypackage_bytes or private keys
   - Tracks lifecycle state: available, reserved, spent, expired
   - ~100-150 bytes per key instead of 400-500 bytes

3. **No Background Tasks:** Avoid spawning background tasks
   - MlsClient is invoked from CLI context (cli.rs::run_client_loop)
   - Add `refresh_key_packages()` method to MlsClient
   - Call from cli.rs main loop periodically or on-demand
   - Simpler lifecycle management, no task tracking needed

4. **Integration Points:**
   - MlsClient orchestrates all operations (maintains pattern from client.rs)
   - MlsConnection manages infrastructure and memberships
   - KeyPackagePool coordinates between OpenMLS StorageProvider and LocalStore metadata
   - CLI loop calls refresh periodically (e.g., every 10 messages or 30 seconds)

---

## New Components

### 1. LocalStore Enhancement (Phase 2.1)

**Purpose:** Track pool lifecycle state, NOT store cryptographic material

**Schema Addition:**

```sql
CREATE TABLE IF NOT EXISTS keypackage_pool_metadata (
    -- Reference to the KeyPackage stored in OpenMLS StorageProvider
    keypackage_ref BLOB PRIMARY KEY,

    -- Lifecycle status
    status TEXT NOT NULL DEFAULT 'created',
    -- Values: created | uploaded | available | reserved | spent | expired | failed

    -- Timestamps (for expiry and lifecycle tracking)
    created_at INTEGER NOT NULL,
    uploaded_at INTEGER,
    reserved_at INTEGER,
    spent_at INTEGER,

    -- Expiry tracking (from OpenMLS KeyPackage lifetime extension)
    not_after INTEGER NOT NULL,

    -- Server-side hints (updated from reserve/spend endpoints)
    reservation_id TEXT,
    reservation_expires_at INTEGER,
    reserved_by TEXT,
    spent_group_id BLOB,
    spent_by TEXT,

    -- Indexes for efficient queries
    INDEX idx_status ON keypackage_pool_metadata(status),
    INDEX idx_expiry ON keypackage_pool_metadata(not_after),
    INDEX idx_created ON keypackage_pool_metadata(created_at)
);
```

**Methods to implement:**

```rust
// Create metadata entry for a newly generated key
pub fn create_pool_metadata(
    &self,
    keypackage_ref: &[u8],
    not_after: i64,
) -> Result<()>;

// Update status (created → uploaded → available → reserved → spent)
pub fn update_pool_metadata_status(
    &self,
    keypackage_ref: &[u8],
    status: &str,
) -> Result<()>;

// Query available count (for replenishment decisions)
pub fn count_by_status(&self, status: &str) -> Result<usize>;

// Find keys that have expired
pub fn get_expired_refs(&self, current_time: i64) -> Result<Vec<Vec<u8>>>;

// Get all metadata for a status (e.g., all "available" keys)
pub fn get_metadata_by_status(&self, status: &str) -> Result<Vec<KeyPackageMetadata>>;

// Remove metadata entry (after key is deleted from OpenMLS storage)
pub fn delete_pool_metadata(&self, keypackage_ref: &[u8]) -> Result<()>;

// Update reservation info from server response
pub fn update_reservation_info(
    &self,
    keypackage_ref: &[u8],
    reservation_id: &str,
    reserved_by: &str,
    expires_at: i64,
) -> Result<()>;

// Mark as spent from server response
pub fn mark_spent(
    &self,
    keypackage_ref: &[u8],
    spent_by: &str,
    group_id: &[u8],
) -> Result<()>;
```

**Data Structure:**

```rust
pub struct KeyPackageMetadata {
    pub keypackage_ref: Vec<u8>,
    pub status: String,
    pub created_at: i64,
    pub uploaded_at: Option<i64>,
    pub reserved_at: Option<i64>,
    pub spent_at: Option<i64>,
    pub not_after: i64,
    pub reservation_id: Option<String>,
    pub reservation_expires_at: Option<i64>,
    pub reserved_by: Option<String>,
    pub spent_group_id: Option<Vec<u8>>,
    pub spent_by: Option<String>,
}
```

### 2. KeyPackagePool (Phase 2.2)

**Responsible for:**
- Generating N KeyPackages via OpenMLS (which auto-stores via StorageProvider)
- Tracking pool state via LocalStore metadata
- Replenishment decision logic
- Expiry lifecycle management

**Structure:**

```rust
pub struct KeyPackagePoolConfig {
    pub target_pool_size: usize,      // Target: 32
    pub low_watermark: usize,         // Trigger replenish: 8 (25%)
    pub hard_cap: usize,              // Max allowed: 64
}

pub struct KeyPackagePool<'a> {
    username: String,
    config: KeyPackagePoolConfig,
    store: &'a LocalStore,
}

impl<'a> KeyPackagePool<'a> {
    /// Generate and store N KeyPackages
    ///
    /// For each key:
    /// 1. Call OpenMLS KeyPackageBuilder::build() → auto-stored via StorageProvider
    /// 2. Extract hash_ref from bundle
    /// 3. Create metadata entry in LocalStore
    pub async fn generate_and_update_pool(
        &self,
        count: usize,
        credential: &CredentialWithKey,
        signer: &SignatureKeyPair,
        provider: &impl OpenMlsProvider,
    ) -> Result<Vec<Vec<u8>>>;

    /// Count available (not reserved, not spent, not expired)
    pub fn get_available_count(&self) -> Result<usize>;

    /// Check if pool needs replenishment
    pub fn should_replenish(&self) -> Result<bool>;

    /// Calculate how many keys need generation
    pub fn get_replenishment_needed(&self) -> Result<usize>;

    /// Mark a key as spent (deleted from both storages)
    ///
    /// Note: This should be called AFTER OpenMLS deletes the key
    /// or coordinated with server spend confirmation
    pub fn mark_as_spent(&self, keypackage_ref: &[u8]) -> Result<()>;

    /// Cleanup expired keys from pool
    ///
    /// Queries metadata for expired keys, removes from both:
    /// - LocalStore metadata
    /// - OpenMLS StorageProvider (via provider.storage().delete_key_package())
    pub fn cleanup_expired(
        &self,
        provider: &impl OpenMlsProvider,
        current_time: SystemTime,
    ) -> Result<usize>;
}
```

**Key Methods in Detail:**

#### generate_and_update_pool()

```rust
pub async fn generate_and_update_pool(
    &self,
    count: usize,
    credential: &CredentialWithKey,
    signer: &SignatureKeyPair,
    provider: &impl OpenMlsProvider,
) -> Result<Vec<Vec<u8>>> {
    let mut generated_refs = Vec::new();

    // Check hard cap
    let current_count = self.get_available_count()?;
    if current_count + count > self.config.hard_cap {
        return Err(MlsError::PoolCapacityExceeded {
            needed: count,
            available: self.config.hard_cap - current_count,
        }.into());
    }

    for _ in 0..count {
        // 1. OpenMLS auto-stores via StorageProvider
        let bundle = KeyPackageBuilder::new(credential, provider)
            .build()?;  // <-- StorageProvider::write_key_package() called automatically

        // 2. Extract reference and expiry
        let ref_hash = bundle.key_package()
            .hash_ref(provider.crypto())?
            .as_slice()
            .to_vec();

        let lifetime = bundle.key_package().lifetime();
        let not_after = lifetime.not_after();

        // 3. Create metadata entry to track in LocalStore
        self.store.create_pool_metadata(&ref_hash, not_after as i64)?;

        generated_refs.push(ref_hash);
    }

    Ok(generated_refs)
}
```

**Why this approach:**
- OpenMLS handles persistence (guaranteed by StorageProvider contract)
- We only track state (status, timestamps, reservation info)
- Single source of truth for key material
- Proper separation of concerns

#### cleanup_expired()

```rust
pub fn cleanup_expired(
    &self,
    provider: &impl OpenMlsProvider,
    current_time: SystemTime,
) -> Result<usize> {
    let now = current_time
        .duration_since(UNIX_EPOCH)?
        .as_secs() as i64;

    // Get all expired references from metadata
    let expired_refs = self.store.get_expired_refs(now)?;

    let mut removed_count = 0;
    for ref_hash in expired_refs {
        // Remove from OpenMLS StorageProvider
        provider.storage()
            .delete_key_package(&ref_hash)?;

        // Remove from LocalStore metadata
        self.store.delete_pool_metadata(&ref_hash)?;

        removed_count += 1;
    }

    Ok(removed_count)
}
```

**Why this approach:**
- Metadata tells us which keys are expired
- OpenMLS StorageProvider deletes the actual key material
- LocalStore metadata is cleaned up after
- Atomic-ish: if OpenMLS delete fails, we retry; if metadata delete fails, we log but continue

### 3. MlsConnection Update (Phase 2.3)

```rust
impl MlsConnection {
    /// Refresh key package pool state and replenish if needed
    pub async fn refresh_key_packages(&mut self) -> Result<()> {
        let pool = KeyPackagePool::new(
            self.username.clone(),
            KeyPackagePoolConfig::default(),
            &self.metadata_store,
        );

        // Remove expired keys
        pool.cleanup_expired(&self.provider, SystemTime::now())?;

        // Check if replenishment needed
        if pool.should_replenish()? {
            if let Some(count_needed) = pool.get_replenishment_needed()? {
                log::info!("Replenishing key packages: {} needed", count_needed);

                let refs = pool.generate_and_update_pool(
                    count_needed,
                    &self.credential,
                    &self.signature_key,
                    &self.provider,
                ).await?;

                log::debug!("Generated {} new key packages", refs.len());

                // Upload to server (Phase 2.4)
                self.upload_key_packages(&refs).await?;
            }
        }

        Ok(())
    }

    async fn upload_key_packages(&self, refs: &[Vec<u8>]) -> Result<()> {
        // Call api.upload_key_packages() with refs
        // After successful upload, update status in LocalStore
        // Remove from local storage if upload fails (retry later)
    }
}
```

### 4. MlsClient API (Phase 2.3)

```rust
impl MlsClient {
    /// Refresh key package pool (check expiry, replenish if needed)
    pub async fn refresh_key_packages(&mut self) -> Result<()> {
        self.connection.refresh_key_packages().await
    }
}
```

### 5. Server API Updates (Phase 2.0 - Already Done)

Building on Phase 2.0 implementation, we have:
- `POST /keypackages/upload` - Client uploads batch
- `POST /keypackages/reserve` - Reserve key for invite
- `POST /keypackages/spend` - Mark key as spent
- `GET /keypackages/status` - Pool health info

### 6. MlsMembership Updates (Phase 2.4)

```rust
// Updated to use reserve/spend model
async fn reserve_invite_key_for_user(&self, target: &str) -> Result<ReservedKeyPackage> {
    self.api.reserve_key_package(target, self.group_id.as_slice()).await
}

async fn finalize_user_add(&self, reserved_key: &ReservedKeyPackage) -> Result<()> {
    self.api.spend_key_package(
        &reserved_key.keypackage_ref,
        self.group_id.as_slice(),
        &self.connection.username,
    ).await
}
```

---

## Implementation Roadmap

### Phase 2.1: Client Storage Layer (LocalStore Metadata)

**Rationale:** Implement metadata storage foundation before pool logic

**Files to create/modify:**
- `client/rust/src/storage.rs` (MODIFY - add keypackage_pool_metadata table and methods)
- `client/rust/tests/storage_tests.rs` (MODIFY - add metadata tests)

**Tasks:**
1. Add `keypackage_pool_metadata` table to LocalStore schema
2. Implement `create_pool_metadata()` - store metadata entry
3. Implement `update_pool_metadata_status()` - update status
4. Implement `count_by_status()` - count keys by status
5. Implement `get_expired_refs()` - find expired keys
6. Implement `get_metadata_by_status()` - retrieve metadata by status
7. Implement `delete_pool_metadata()` - remove metadata entry
8. Implement `update_reservation_info()` - store server reservation info
9. Implement `mark_spent()` - store spend info from server

**Success Criteria:**
- [ ] Unit test: Create and retrieve metadata
- [ ] Unit test: Count by status works
- [ ] Unit test: Get expired refs returns expired keys
- [ ] Unit test: Status updates work correctly
- [ ] Unit test: Reservation info stored/retrieved
- [ ] Unit test: Spend info stored/retrieved
- [ ] No regression in existing identity storage tests

**Estimate:** 1-1.5 days

---

### Phase 2.2: Client KeyPackagePool Core Implementation

**Rationale:** Implement pool logic using OpenMLS StorageProvider + LocalStore metadata

**Files to create/modify:**
- `client/rust/src/mls/keypackage_pool.rs` (NEW)
- `client/rust/tests/keypackage_pool_tests.rs` (NEW)
- `client/rust/src/mls/mod.rs` (MODIFY - add keypackage_pool module)

**Tasks:**
1. Create KeyPackagePoolConfig struct
2. Create KeyPackagePool struct with LocalStore and OpenMLS provider references
3. Implement `generate_and_update_pool()`:
   - Call OpenMLS KeyPackageBuilder::build() N times
   - For each: extract ref_hash and not_after
   - Create metadata entry in LocalStore
   - Return list of generated refs
4. Implement `get_available_count()` - query LocalStore for available status
5. Implement `should_replenish()` - check if available < low_watermark
6. Implement `get_replenishment_needed()` - calculate count needed
7. Implement `mark_as_spent()` - update status in LocalStore
8. Implement `cleanup_expired()`:
   - Query LocalStore for expired keys
   - Delete from OpenMLS StorageProvider
   - Delete metadata from LocalStore

**Success Criteria:**
- [ ] Unit test: Pool generation creates N keys in OpenMLS and metadata in LocalStore
- [ ] Unit test: Available count queries metadata correctly
- [ ] Unit test: Replenishment threshold logic verified
- [ ] Unit test: Expiry detection and cleanup works
- [ ] Unit test: Mark as spent updates status
- [ ] Unit test: Hard cap enforcement tested
- [ ] Unit test: All generated refs are unique
- [ ] All methods tested with real LocalStore (not mocked)
- [ ] Property tests: available + reserved + spent + expired = total

**Estimate:** 2-3 days

---

### Phase 2.3: Client MlsConnection & MlsClient Integration

**Rationale:** Integrate pool refresh into client lifecycle

**Files to modify:**
- `client/rust/src/mls/connection.rs` (MODIFY - add refresh_key_packages())
- `client/rust/src/client.rs` (MODIFY - expose refresh_key_packages())
- `client/rust/tests/client_tests.rs` (MODIFY - add refresh tests)

**Tasks:**
1. Add `refresh_key_packages()` method to MlsConnection
2. Add `refresh_key_packages()` method to MlsClient (delegates to connection)
3. Implement expiry cleanup during refresh
4. Implement replenishment decision during refresh
5. Implement key upload to server after generation
6. Update error handling for pool operations

**Success Criteria:**
- [ ] Unit test: Refresh removes expired keys
- [ ] Unit test: Refresh triggers replenishment when needed
- [ ] Unit test: Refresh succeeds when no action needed
- [ ] Integration test: Refresh works with real LocalStore and OpenMLS provider
- [ ] No background tasks spawned (synchronous only)

**Estimate:** 1-2 days

---

### Phase 2.4: Server REST API & Client Integration

**Rationale:** Complete server-client pool synchronization

**Note:** Phase 2.0 already implements server-side storage. This phase adds client-server coordination.

**Files to modify:**
- `client/rust/src/api.rs` (MODIFY - add upload, reserve, spend methods)
- `client/rust/src/mls/membership.rs` (MODIFY - use reserve/spend for invitations)
- `client/rust/tests/api_tests.rs` (NEW/MODIFY - test pool endpoints)

**Tasks:**
1. Implement `api.upload_key_packages()` - POST to server with refs
2. Implement `api.reserve_key_package()` - POST to reserve key
3. Implement `api.spend_key_package()` - POST to mark as spent
4. Implement `api.get_key_package_status()` - GET pool health
5. Update MlsMembership to use reserve/spend instead of single get_user_key()
6. Error handling for pool exhaustion, expiry, double-spend

**Success Criteria:**
- [ ] Integration test: Upload batch of keys
- [ ] Integration test: Reserve returns available key
- [ ] Integration test: Double-spend prevented
- [ ] Integration test: Expired keys rejected
- [ ] Integration test: Reservation timeout releases key
- [ ] Integration test: Pool exhaustion error is clear
- [ ] End-to-end test: Multiple users can concurrently invite same target

**Estimate:** 2-3 days

---

### Phase 2.5: CLI Integration & Periodic Refresh

**Rationale:** Integrate pool refresh into CLI loop

**Architectural Decision (Implemented 2025-11-06):**
The original plan proposed a message-counter approach ("every 10 messages"). **Agent A implemented a superior time-based approach instead** with configurable period (default 1 hour). Rationale:
- Pool health maintained even during low-activity periods
- More predictable refresh schedule for operational monitoring
- Avoids edge cases where inactive groups never refresh
- Aligns with standard MLS key rotation practices
- Better than message-based for long-running sessions with bursty activity

**Files to modify:**
- `client/rust/src/client.rs` (MODIFY - add time-based refresh tracking)
- `client/rust/src/cli.rs` (MODIFY - call refresh_key_packages() on timer)

**Tasks:**
1. Add `last_refresh_time: Option<SystemTime>` and `refresh_period: Duration` fields to MlsClient
2. Implement `should_refresh()` method checking if period has elapsed
3. Implement `update_refresh_time()` method to track last refresh
4. Add configurable `set_refresh_period()` method
5. Integrate into CLI loop via third `tokio::select!` branch with timeout
6. Log refresh triggers and results
7. Handle refresh errors gracefully (log but don't break CLI)
8. Write comprehensive unit tests for time-based logic

**Success Criteria:**
- [ ] Time-based refresh trigger implemented ✅
- [ ] Configurable period (default 1 hour) ✅
- [ ] Unit test: Refresh called on correct time interval ✅
- [ ] Integration test: CLI loop with refresh works end-to-end ✅
- [ ] E2E test: Multiple users, refresh triggers ✅
- [ ] Refresh errors logged but don't crash client ✅
- [ ] Refresh is idempotent (multiple calls safe) ✅

**Status:** ✅ COMPLETE (2025-11-06)

**Estimate:** 1-2 days

---

### Phase 2.5.1: Code Cleanup & Quality Fixes (Pre-Phase 2.6)

**Rationale:** Address pre-existing code quality issues (clippy warnings and test failure) identified during Phase 2.5 review before proceeding to Phase 2.6.

**Issues to Fix:**

1. **Clippy Warnings: `field_reassign_with_default` (5 total)**
   - Files: `keypackage_pool_tests.rs` (4 warnings), `client_tests.rs` (1 warning)
   - Issue: Using `let mut x = T::default(); x.field = value;` instead of struct literal syntax
   - Fix: Replace with `T { field: value, ..Default::default() }` syntax
   - Examples (from Agent B feedback):
     ```rust
     // Current (triggers warning):
     let mut config = KeyPackagePoolConfig::default();
     config.target_pool_size = 2;

     // Fixed:
     let config = KeyPackagePoolConfig {
         target_pool_size: 2,
         ..Default::default()
     };
     ```
   - Severity: MINOR (code style, no functionality impact)
   - Source: Phase 2.2/2.4 code (not caused by Phase 2.5)

2. **Test Failure: `test_sender_skips_own_application_message`**
   - Status: Pre-existing (not caused by Phase 2.5)
   - Error: `KeyPackage(PoolExhausted { username: "bob" })`
   - Issue: Bob's KeyPackage pool exhausted when Alice invites him
   - Root Cause: Phase 2.4 invite flow needs investigation/fix
   - Pass Rate: 27/28 integration tests (96%)
   - Action: Investigate and fix root cause in KeyPackage pool or invite logic
   - Severity: INFORMATIONAL (needs fixing but doesn't block Phase 2.6)

**Files to modify:**
- `client/rust/tests/keypackage_pool_tests.rs` (MODIFY - fix 4 clippy warnings)
- `client/rust/tests/client_tests.rs` (MODIFY - fix 1 clippy warning)
- `client/rust/src/mls/membership.rs` (INVESTIGATE - may need fix for pool exhaustion)
- `client/rust/src/error.rs` (VERIFY - pool exhaustion error propagation)

**Tasks:**
1. Fix all 5 clippy warnings using struct literal syntax with `..Default::default()`
2. Investigate `test_sender_skips_own_application_message` failure
3. Identify root cause (insufficient pool size, incorrect reserve/spend logic, timing issue)
4. Implement fix in relevant module (membership.rs or elsewhere)
5. Verify all tests pass including the previously failing test
6. Run clippy clean (no warnings)
7. Verify no new regressions

**Success Criteria:**
- [ ] All clippy warnings fixed (0 warnings in Phase 2.2/2.4 code)
- [ ] `test_sender_skips_own_application_message` passes
- [ ] All 28 integration tests pass
- [ ] All 71 unit tests pass (no regressions)
- [ ] Clippy check: `cargo clippy --all-targets -- -D warnings` passes cleanly

**Estimate:** 1-2 days

**Note:** This phase is optional if pre-existing issues are acceptable. However, recommended to address before Phase 2.6 for clean git history and full test suite passing.

---

### Phase 2.6: Documentation & End-to-End Testing

**Rationale:** Document system, test full scenarios

**Files to create/modify:**
- `docs/keypackage-pool-implementation.md` (NEW - operation guide)
- Test scenarios covering all major flows

**Test scenarios:**
1. Concurrent invitations consume different keys
2. Expiry and rotation (old keys cleaned up)
3. Server pool exhaustion error propagates clearly
4. Pool refresh maintains health
5. Pool state survives client restart
6. Pool health metrics exposed to user

**Success Criteria:**
- [ ] All test scenarios pass
- [ ] E2E test: Pool starts at 1 key, grows to 32 via refresh
- [ ] E2E test: Multiple users invite simultaneously without conflicts
- [ ] Documentation describes pool lifecycle and refresh mechanism
- [ ] README updated with pool features and limitations

**Estimate:** 1-2 days

---

## Technical Details

### Key Differences from Previous Plan

| Aspect | Previous (20251028) | New (20251105) |
|--------|-------------------|-----------------|
| **Bundle Storage** | LocalStore table | OpenMLS StorageProvider (automatic) |
| **LocalStore Fields** | keypackage_bytes, private_init_key, private_encryption_key | Metadata only: status, timestamps, not_after |
| **Storage Duplication** | Full bundle duplicated (~400-500 bytes) | Metadata only (~100-150 bytes) |
| **Deletion Logic** | Delete from LocalStore | Delete from both OpenMLS & LocalStore |
| **Phase 2.1 Size** | ~200 lines LocalStore code | ~100 lines LocalStore code |
| **Phase 2.2 Integration** | Manual bundle serialization | Direct OpenMLS API usage |

### OpenMLS StorageProvider Behavior

**Automatic operations (we don't implement):**
- `write_key_package()` - Called when KeyPackageBuilder::build() completes
- `key_package()` - Used by OpenMLS when Welcome is processed
- `delete_key_package()` - Called after Welcome consumption (single-use enforcement)
- Serialization/deserialization of bundle

**What we implement:**
- Track which keys are "available" vs "reserved" vs "spent"
- Expiry cleanup decisions (which keys to delete)
- Server synchronization (upload, reserve, spend)
- Replenishment logic (when to generate more)

### Data Flow: Key Generation

```
1. KeyPackagePool::generate_and_update_pool() called
2. Loop N times:
   a. KeyPackageBuilder::build(credential, provider)
   b. OpenMLS calls provider.storage().write_key_package()
   c. Bundle persisted in OpenMLS storage
   d. Extract hash_ref and not_after from bundle
   e. LocalStore::create_pool_metadata(ref, not_after)
3. Return list of generated refs
```

### Data Flow: Key Expiry Cleanup

```
1. KeyPackagePool::cleanup_expired() called
2. LocalStore::get_expired_refs(now) → returns [ref1, ref2, ...]
3. For each ref:
   a. provider.storage().delete_key_package(ref)
   b. LocalStore::delete_pool_metadata(ref)
4. Return count of removed keys
```

### Data Flow: Server Upload

```
1. refresh_key_packages() decides to replenish
2. generate_and_update_pool() creates N keys
3. api.upload_key_packages(refs) sends to server
4. Server stores in keypackages table
5. On success: LocalStore::update_pool_metadata_status(ref, "uploaded")
```

### Data Flow: Invitation (Reserve/Spend)

```
Reserve:
1. MlsMembership::invite_user() calls api.reserve_key_package(target, group_id)
2. Server finds available key, marks reserved, returns ref and ttl
3. Client stores reservation info in LocalStore metadata

Spend:
1. After Commit posted, MlsMembership::finalize_user_add() calls api.spend_key_package()
2. Server marks key as spent
3. Client updates LocalStore metadata status = "spent"
4. Client MAY call cleanup to remove from OpenMLS (later, in refresh)
```

---

## Files Modified/Created Summary

### Client (Rust)

**New:**
- `client/rust/src/mls/keypackage_pool.rs` - KeyPackagePool struct and core logic
- `client/rust/tests/keypackage_pool_tests.rs` - Unit tests for pool operations

**Modified:**
- `client/rust/src/storage.rs` - Add keypackage_pool_metadata table and CRUD methods
- `client/rust/tests/storage_tests.rs` - Add storage metadata tests
- `client/rust/src/mls/mod.rs` - Add keypackage_pool module
- `client/rust/src/mls/connection.rs` - Add refresh_key_packages() method
- `client/rust/src/client.rs` - Expose refresh_key_packages() method
- `client/rust/src/cli.rs` - Call refresh periodically in main loop
- `client/rust/src/mls/membership.rs` - Use reserve/spend for invitations
- `client/rust/src/api.rs` - Add pool endpoint methods
- `client/rust/src/error.rs` - Add pool-specific error types
- `client/rust/tests/client_tests.rs` - Add refresh and pool tests
- `client/rust/tests/api_tests.rs` - Add pool endpoint tests

### Server (Rust)

**Note:** Phase 2.0 already completed this. Kept for reference:
- `server/src/db/keypackage_store.rs` - KeyPackage storage and TTL logic
- `server/src/routes/keypackages.rs` - REST endpoints for pool operations
- Tests and modifications from Phase 2.0

### Documentation

**New:**
- `docs/keypackage-pool-implementation.md` - Pool operation guide and architecture

---

## Dependencies & Risks

### Dependencies
- **OpenMLS StorageProvider** - Must be available via provider instance
- **Phase 2.0 completion** - Server API endpoints must exist
- **LocalStore flexibility** - Must support new metadata table

### Risks

1. **Sync between OpenMLS and LocalStore metadata**
   - *Mitigation:* Metadata is authoritative for "what should exist", periodically verify OpenMLS has the keys
   - *Alternative:* Use transactions where possible

2. **Key deletion timing**
   - *Mitigation:* Delete from OpenMLS first (critical), then metadata. If metadata delete fails, log but continue.
   - *Alternative:* Use background cleanup task (violates Phase 2.3 requirement of no background tasks)

3. **Provider.storage() access**
   - *Mitigation:* Verify provider.storage() is public API and stable
   - *Alternative:* Pass StorageProvider trait object separately

4. **Refresh timing and network failures**
   - *Mitigation:* Refresh is idempotent (can be called multiple times)
   - *Alternative:* Use exponential backoff for retries

---

## Success Criteria (Phase 2 Complete)

**Functional Requirements:**
- [ ] All 6 phases implemented and tested
- [ ] Client can generate and store multiple KeyPackages (target 32)
- [ ] KeyPackage pool automatically replenishes when < 25% available
- [ ] Expired KeyPackages are cleaned up automatically
- [ ] Multiple concurrent invitations consume different KeyPackages
- [ ] Server prevents double-spend of KeyPackages
- [ ] Pool state persists across client restarts
- [ ] Clear error messages for pool exhaustion, expiry, other failures

**Code Quality:**
- [ ] All new code passes clippy linting
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] E2E test (concurrent invitations, expiry, refresh) passes
- [ ] No background tasks spawned
- [ ] No regression in Phase 1 or existing functionality
- [ ] All CLAUDE.md guidelines followed

**Architecture:**
- [ ] KeyPackagePool in `src/mls/keypackage_pool.rs`
- [ ] Metadata persisted in LocalStore (no bundle duplication)
- [ ] OpenMLS StorageProvider used for key material (automatic)
- [ ] MlsClient exposes `refresh_key_packages()` method
- [ ] CLI calls refresh from main loop
- [ ] Reserve/spend model used for invitations
- [ ] Server API endpoints implemented (Phase 2.0)
- [ ] All error types properly defined

**Storage Efficiency:**
- [ ] No duplicate storage of key material
- [ ] Metadata table only (~100-150 bytes per key)
- [ ] ~40% storage savings per pool vs. previous plan

**Documentation:**
- [ ] Implementation guide: `docs/keypackage-pool-implementation.md`
- [ ] README updated with pool features and limitations
- [ ] Code comments explain pool lifecycle and refresh strategy

---

## Implementation Checklist

**Phase 2.1 (Storage Metadata):**
- [ ] keypackage_pool_metadata table created
- [ ] All CRUD methods working
- [ ] Query methods (count, get_expired) working
- [ ] Unit tests pass
- [ ] No regression in existing storage tests

**Phase 2.2 (Pool Core):**
- [ ] KeyPackagePool struct implemented
- [ ] generate_and_update_pool() creates keys in OpenMLS and metadata in LocalStore
- [ ] get_available_count() queries metadata
- [ ] should_replenish() and get_replenishment_needed() logic verified
- [ ] mark_as_spent() works
- [ ] cleanup_expired() removes from both storages
- [ ] All unit tests pass
- [ ] Integration tests with real LocalStore and OpenMLS

**Phase 2.3 (Integration):**
- [ ] refresh_key_packages() added to Connection and Client
- [ ] Refresh tested with real provider and LocalStore
- [ ] No background tasks spawned

**Phase 2.4 (Server Coordination):**
- [ ] Client upload, reserve, spend endpoints working
- [ ] MlsMembership uses reserve/spend for invitations
- [ ] Error handling complete

**Phase 2.5 (CLI - Time-based Refresh):**
- [x] Time-based refresh trigger implemented
- [x] Configurable period (default 1 hour)
- [x] Periodic refresh in CLI loop via tokio::select!
- [x] Error handling tested
- [x] Idempotency verified
- [x] All 10 unit tests pass

**Phase 2.5.1 (Code Cleanup - Pre-Phase 2.6):**
- [ ] Fix 5 clippy warnings (field_reassign_with_default)
- [ ] Fix test_sender_skips_own_application_message failure
- [ ] All 28 integration tests pass
- [ ] All 71 unit tests pass
- [ ] Clippy clean (0 warnings)

**Phase 2.6 (Docs & Testing):**
- [ ] All test scenarios covered
- [ ] E2E test passes
- [ ] Documentation complete

---

## Next Steps

1. **Review this plan** - Verify architecture and approach align with project goals
2. **Revert Phase 2.1 and 2.2 code** - Remove duplicate storage implementation
3. **Begin Phase 2.1** - Implement LocalStore metadata table and methods
4. **Proceed through phases 2.2-2.6** - Following the roadmap and success criteria

---

## Phase 2.7: Server-Side KeyPackage Expiry Cleanup Task

**Rationale:** Implement automatic periodic cleanup of expired KeyPackages on the server to prevent database bloat and fulfill the spec requirement for "automatic cleanup".

**Design Decisions:**

1. **Periodic Background Task** - Cleanup runs on a timer (e.g., every 60 minutes), not on-demand
   - Decouples cleanup from request latency
   - Predictable resource usage
   - Aligns with "periodically" language in strategy doc

2. **CLI-Only Activation** - Cleanup task only spawns when server runs from CLI, NOT in unit tests
   - Prevents test interference and flakiness
   - Tests can run `cleanup_expired()` directly if needed
   - Separates test code from production behavior

3. **Graceful Error Handling** - Cleanup failures are logged but don't crash the server
   - Ensures availability even if cleanup has issues
   - Operators can debug via logs

4. **Optional Admin Endpoint** - Future enhancement for manual cleanup triggering (not in Phase 2.7)

---

### Implementation Details

**Files to create/modify:**

- `server/src/db/cleanup_task.rs` (NEW) - Cleanup task executor
- `server/src/db/mod.rs` (MODIFY) - Export cleanup task
- `server/src/server.rs` (MODIFY) - Add function to start cleanup task (conditional)
- `server/src/main.rs` (MODIFY) - Call start cleanup task during normal startup
- `server/src/lib.rs` (MODIFY) - Re-export cleanup task startup function (for test isolation)
- `server/src/db/keypackage_store.rs` (MODIFY - no logic changes) - Already implements cleanup_expired()

**New Module: `server/src/db/cleanup_task.rs`**

```rust
/// KeyPackage expiry cleanup task
/// Runs periodically to remove expired KeyPackages from the database.
///
/// This module provides:
/// - CleanupTask configuration and executor
/// - Periodic scheduling (default: every 60 minutes)
/// - Graceful error handling and logging

use crate::db::{keypackage_store::KeyPackageStore, DbPool};
use std::time::Duration;
use tokio::task::JoinHandle;

/// Configuration for the cleanup task
#[derive(Debug, Clone)]
pub struct CleanupTaskConfig {
    /// Interval between cleanup runs (seconds)
    pub interval_seconds: u64,
    /// Enable/disable the cleanup task
    pub enabled: bool,
}

impl Default for CleanupTaskConfig {
    fn default() -> Self {
        Self {
            interval_seconds: 3600,  // 60 minutes
            enabled: true,
        }
    }
}

/// Start the KeyPackage cleanup task
///
/// Spawns a background tokio task that periodically runs KeyPackageStore::cleanup_expired().
/// The task runs every `config.interval_seconds` seconds.
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `config` - Cleanup task configuration
///
/// # Returns
/// A JoinHandle for the spawned task. Drop the handle to cancel the task.
///
/// # Example
/// ```ignore
/// let config = CleanupTaskConfig::default();
/// let handle = start_cleanup_task(pool.clone(), config);
/// // Task runs in background
/// // Drop handle to stop it
/// drop(handle);
/// ```
pub fn start_cleanup_task(
    pool: DbPool,
    config: CleanupTaskConfig,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if !config.enabled {
            log::info!("KeyPackage cleanup task disabled");
            return;
        }

        log::info!(
            "KeyPackage cleanup task started (interval: {} seconds)",
            config.interval_seconds
        );

        let interval = Duration::from_secs(config.interval_seconds);

        loop {
            tokio::time::sleep(interval).await;

            match KeyPackageStore::cleanup_expired(&pool).await {
                Ok(count) => {
                    if count > 0 {
                        log::info!(
                            "KeyPackage cleanup completed: {} expired keys removed",
                            count
                        );
                    } else {
                        log::debug!("KeyPackage cleanup completed: no expired keys found");
                    }
                }
                Err(e) => {
                    log::error!("KeyPackage cleanup failed: {}", e);
                    // Continue running despite error
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_test_pool, keypackage_store::KeyPackageStore};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn test_cleanup_task_config_default() {
        let config = CleanupTaskConfig::default();
        assert_eq!(config.interval_seconds, 3600);
        assert!(config.enabled);
    }

    #[tokio::test]
    async fn test_cleanup_task_can_be_disabled() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let config = CleanupTaskConfig {
            interval_seconds: 1,
            enabled: false,
        };

        let handle = start_cleanup_task(pool, config);

        // Give it a moment to start and check it exits immediately
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Task should be done (disabled)
        assert!(handle.is_finished());
    }

    #[tokio::test]
    async fn test_cleanup_task_runs_cleanup() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Add an expired key
        KeyPackageStore::save_key_package(
            &pool,
            "test_user",
            &vec![0x01, 0x02],
            &vec![0x10, 0x20],
            now - 100,  // Already expired
            None,
            None,
        )
        .await
        .unwrap();

        let config = CleanupTaskConfig {
            interval_seconds: 1,
            enabled: true,
        };

        let _handle = start_cleanup_task(pool.clone(), config);

        // Wait for task to run (interval + some buffer)
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Verify expired key was removed
        let result = KeyPackageStore::get_key_package(&pool, &vec![0x01, 0x02]).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none(), "Expired key should be deleted");
    }
}
```

**Modify: `server/src/db/mod.rs`**

Add at the end of the module-level exports:

```rust
pub mod cleanup_task;
pub use cleanup_task::{CleanupTaskConfig, start_cleanup_task};
```

**Modify: `server/src/server.rs`**

Add a function to conditionally start the cleanup task:

```rust
/// Start the KeyPackage cleanup task
///
/// This function should be called from main.rs during normal server startup,
/// but NOT from test setup code.
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `enable_cleanup` - Whether to enable the cleanup task
pub fn start_cleanup_task_if_enabled(
    pool: web::Data<crate::db::DbPool>,
    enable_cleanup: bool,
) {
    if enable_cleanup {
        let cleanup_config = crate::db::CleanupTaskConfig::default();
        let pool_inner = (*pool).clone();
        let _cleanup_handle = crate::db::start_cleanup_task(pool_inner, cleanup_config);

        // Note: We don't drop the handle here; it runs for the lifetime of the server.
        // The tokio runtime will clean up the task when the server shuts down.
        log::info!("KeyPackage cleanup task started");
    } else {
        log::debug!("KeyPackage cleanup task disabled");
    }
}
```

**Modify: `server/src/main.rs`**

Update main() to start the cleanup task:

```rust
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_default_env()
        .format_timestamp_millis()
        .init();

    let config = Config::from_args();

    log::info!("Starting MLS Chat Server");
    log::info!("Database: {:?}", config.database);
    log::info!("Port: {}", config.port);

    // Write PID file if specified
    if let Some(pidfile) = &config.pidfile {
        let pid = process::id().to_string();
        fs::write(pidfile, pid).expect("Failed to write PID file");
        log::info!("PID file written to: {:?}", pidfile);
    }

    // Initialize database
    let pool =
        db::create_pool(config.database.to_str().unwrap()).expect("Failed to create database pool");

    log::info!("Database initialized");

    let pool_data = web::Data::new(pool.clone());
    let ws_server = web::Data::new(WsServer::new(Arc::new(pool_data.clone())));

    // Start cleanup task (only in production, not in tests)
    server::start_cleanup_task_if_enabled(pool_data.clone(), true);

    // Start HTTP server
    let bind_addr = format!("127.0.0.1:{}", config.port);
    log::info!("Starting HTTP server on {}", bind_addr);

    let http_server = server::create_http_server(pool_data, ws_server, &bind_addr)?;
    http_server.await
}
```

**Modify: `server/src/lib.rs`**

Export the function for potential use in integration tests:

```rust
pub use server::start_cleanup_task_if_enabled;
```

---

### Success Criteria

**Functional Requirements:**
- [ ] Cleanup task starts automatically when server runs from `main()`
- [ ] Cleanup task does NOT start in unit tests (no background task in test mode)
- [ ] Cleanup runs every 60 minutes (default interval)
- [ ] Each cleanup run deletes all KeyPackages where `not_after <= now`
- [ ] Cleanup logs count of deleted keys (when count > 0)
- [ ] Cleanup failures are logged but don't crash the server
- [ ] Cleanup can be disabled via configuration (for testing)
- [ ] Multiple cleanup runs don't cause race conditions (lock-based access)

**Code Quality:**
- [ ] All new code passes clippy linting
- [ ] All unit tests in cleanup_task.rs pass
- [ ] All existing server tests still pass (no regression)
- [ ] cleanup_task.rs module is well-documented (docstrings)
- [ ] Error handling is consistent with server logging patterns

**Integration:**
- [ ] Cleanup task compiles with rest of codebase
- [ ] Task persists for entire server lifetime (not dropped prematurely)
- [ ] Task gracefully stops when server shuts down
- [ ] Task doesn't interfere with active reserve/spend operations
- [ ] Concurrent keypackage operations and cleanup are safe

**Logging & Observability:**
- [ ] Startup log: "KeyPackage cleanup task started (interval: X seconds)"
- [ ] Success log: "KeyPackage cleanup completed: N expired keys removed"
- [ ] Debug log: "KeyPackage cleanup completed: no expired keys found" (when count = 0)
- [ ] Error log: "KeyPackage cleanup failed: [error message]"
- [ ] Disabled log: "KeyPackage cleanup task disabled"

**Test Isolation:**
- [ ] Unit tests do NOT spawn background cleanup tasks
- [ ] Integration tests CAN optionally call cleanup manually if needed
- [ ] Test code is not affected by main()'s cleanup task startup
- [ ] Configuration allows easy opt-out for test scenarios

---

### Implementation Roadmap

**Step 1: Create cleanup_task.rs module**
- Implement `CleanupTaskConfig` struct
- Implement `start_cleanup_task()` function
- Add comprehensive docstrings
- Add unit tests

**Step 2: Integrate with server initialization**
- Modify `server.rs` to add `start_cleanup_task_if_enabled()`
- Modify `main.rs` to call the function
- Modify `lib.rs` to export for test use

**Step 3: Testing**
- Run unit tests in cleanup_task.rs
- Verify no existing tests break
- Manual verification: Start server, check logs, verify cleanup runs

**Step 4: Documentation**
- Add comment to `main.rs` explaining cleanup task startup
- Update README if it mentions server features
- Add note to `docs/keypackage-pool-implementation.md`

---

### Estimated Time: 1-2 days

---

### Dependencies & Risks

**Dependencies:**
- `tokio` (already available)
- `tokio::time::sleep` and task spawning (stable)
- Existing `KeyPackageStore::cleanup_expired()` method

**Risks:**

1. **Task resource leaks**
   - *Mitigation:* Use Tokio's built-in task management; task is owned by runtime and cleaned up on shutdown
   - *Alternative:* Add explicit task cancellation token if server supports graceful shutdown

2. **Lock contention with active requests**
   - *Mitigation:* Cleanup uses same lock as other keypackage operations; SQLite serializes naturally
   - *Alternative:* Schedule cleanup during predictable low-traffic windows (future enhancement)

3. **Cleanup never runs in test mode**
   - *Mitigation:* This is intentional; tests call `cleanup_expired()` directly if needed
   - *Alternative:* Provide a test utility function to manually trigger cleanup

4. **Log spam if cleanup fails repeatedly**
   - *Mitigation:* Log at ERROR level; operators will notice and investigate
   - *Alternative:* Add exponential backoff for error logging (future enhancement)

---

### Future Enhancements (Not Phase 2.7)

- Phase 2.8: Add admin endpoint `POST /admin/cleanup` to manually trigger cleanup
- Phase 2.9: Make cleanup interval configurable via CLI args or config file
- Phase 3.0: Add metrics (e.g., Prometheus) to track cleanup latency and count
- Phase 3.1: Implement graceful shutdown that cancels cleanup task properly

---

## References

- `docs/keypackage-pool-strategy.md` - Strategy document
- `changelog/20251105-storageprovider-analysis.md` - OpenMLS StorageProvider analysis
