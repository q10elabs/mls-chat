# KeyPackage Pool Implementation Plan

**Date:** 2025-10-28
**Phase:** Phase 2 (Production-Ready Implementation)
**Status:** Planning

## Overview

This changelog documents the complete implementation plan for migrating from a single-key-package architecture to a **pool-based strategy** as specified in `docs/keypackage-pool-strategy.md`. This is the production-grade implementation that supports:

- Concurrent group additions
- Asynchronous invitations without blocking
- Single-use keys with compromise containment
- Automatic expiry and rotation

## Task Specification

**Goal:** Implement a KeyPackage pool management system that:

1. Generates and uploads multiple KeyPackages (32 target) during initialization
2. Tracks pool state (available, reserved, expired counts)
3. Maintains expiry bounds (~7-14 days)
4. Enables background replenishment when pool < 25% (8 keys)
5. Exposes pool health status to users
6. Integrates with server reservation/spend tracking

**Scope:** Rust client implementation + server API updates

**Depends On:** Phase 1 completion (error handling fixes must be done first)

## Architecture Overview

### Design Decisions (Updated from Codebase Review)

Based on `docs/codebase-overview.md`, the following architectural adjustments are made:

1. **Storage Layer:** Use `storage.rs` (LocalStore) to persist KeyPackage pool state
   - Add new table `keypackages` to store pool metadata
   - Reuse existing SQLite connection pattern
   - Maintains single metadata.db for all application-level data

2. **No Background Tasks:** Avoid spawning background tasks
   - MlsClient is invoked from CLI context (cli.rs::run_client_loop)
   - Add `refresh_key_packages()` method to MlsClient
   - Call from cli.rs main loop periodically or on-demand
   - Simpler lifecycle management, no task tracking needed

3. **Integration Points:**
   - MlsClient orchestrates all operations (maintains pattern from client.rs)
   - MlsConnection manages infrastructure and memberships
   - LocalStore persists pool state alongside identity metadata
   - CLI loop calls refresh periodically (e.g., every 10 messages or 30 seconds)

### New Components

#### 1. KeyPackagePool (New Struct in `src/mls/keypackage_pool.rs`)

Responsible for:
- Local pool state tracking (available, reserved, expired)
- KeyPackage generation and serialization
- Expiry lifecycle management
- Replenishment decision logic

```rust
pub struct KeyPackagePool {
    username: String,
    target_pool_size: usize,             // Target: 32
    low_watermark: usize,                // Trigger replenish: 8 (25%)
    hard_cap: usize,                     // Max allowed: 64
}

pub struct KeyPackageMetadata {
    id: u32,                             // LocalStore primary key
    ref_hash: String,                    // KeyPackage reference (hash)
    not_before: i64,                     // Unix timestamp
    not_after: i64,                      // Unix timestamp
    status: String,                      // "available" | "reserved" | "spent" | "expired"
    created_at: i64,                     // Unix timestamp
}

impl KeyPackagePool {
    // Core methods:
    pub async fn generate_and_store(&self, count: usize, store: &LocalStore) -> Result<()>;
    pub async fn get_available_count(&self, store: &LocalStore) -> Result<usize>;
    pub async fn should_replenish(&self, store: &LocalStore) -> Result<bool>;
    pub async fn get_replenishment_needed(&self, store: &LocalStore) -> Result<Option<usize>>;
    pub async fn mark_as_spent(&self, ref_hash: &str, store: &LocalStore) -> Result<()>;
}
```

#### 2. Storage Layer Updates (Update: `src/storage.rs`)

Add new table and methods to LocalStore. **CRITICAL:** Must store all three fields from KeyPackageBundle:

**Phase 1 Schema (Minimum for async invitations):**

```sql
CREATE TABLE IF NOT EXISTS keypackages (
    -- Hash of serialized KeyPackage (ciphersuite-dependent: 32/48/64 bytes)
    keypackage_ref BLOB PRIMARY KEY,

    -- Serialized KeyPackage bytes (public part)
    keypackage_bytes BLOB NOT NULL,

    -- CRITICAL: Private HPKE init key (decrypt Welcome messages)
    private_init_key BLOB NOT NULL,

    -- CRITICAL: Private encryption key (group operations)
    private_encryption_key BLOB NOT NULL,

    -- Creation timestamp
    created_at INTEGER NOT NULL
);
```

**Phase 2 Schema (Complete pool management):**

```sql
CREATE TABLE IF NOT EXISTS keypackages (
    keypackage_ref BLOB PRIMARY KEY,
    keypackage_bytes BLOB NOT NULL,
    private_init_key BLOB NOT NULL,
    private_encryption_key BLOB NOT NULL,

    -- Timestamps
    created_at INTEGER NOT NULL,
    uploaded_at INTEGER,
    reserved_at INTEGER,
    spent_at INTEGER,

    -- Lifecycle status
    status TEXT NOT NULL DEFAULT 'created',

    -- Expiry tracking
    not_before INTEGER NOT NULL,
    not_after INTEGER NOT NULL,

    -- Credential and ciphersuite binding
    credential_hash BLOB NOT NULL,
    credential_type TEXT,
    ciphersuite INTEGER NOT NULL,

    -- Flags and hints
    last_resort INTEGER NOT NULL DEFAULT 0,
    reservation_id TEXT,
    reservation_expires_at INTEGER,
    reserved_by TEXT,
    spent_group_id BLOB,
    spent_by TEXT,

    -- Indexes
    INDEX idx_status ON keypackages(status),
    INDEX idx_credential ON keypackages(credential_hash),
    INDEX idx_expiry ON keypackages(not_after),
    INDEX idx_ciphersuite ON keypackages(ciphersuite)
);
```

**Methods to implement (Phase 1):**
```rust
pub fn save_key_package_bundle(
    &self,
    keypackage_ref: &[u8],
    keypackage_bytes: &[u8],
    private_init_key: &[u8],
    private_encryption_key: &[u8],
) -> Result<()>;

pub fn load_key_package_bundle(
    &self,
    keypackage_ref: &[u8],
) -> Result<Option<KeyPackageBundleData>>;

pub fn get_key_package_bundle(
    &self,
    keypackage_ref: &[u8],
) -> Result<KeyPackageBundleData>;

pub fn delete_key_package_bundle(&self, keypackage_ref: &[u8]) -> Result<()>;
```

**Methods to add (Phase 2):**
```rust
pub fn update_key_package_status(&self, keypackage_ref: &[u8], status: &str) -> Result<()>;
pub fn count_key_packages_by_status(&self, status: &str) -> Result<usize>;
pub fn get_expired_keys(&self) -> Result<Vec<Vec<u8>>>;
pub fn get_available_keys(&self, not_after_min: i64) -> Result<Vec<KeyPackageMetadata>>;
pub fn update_reservation_info(
    &self,
    keypackage_ref: &[u8],
    reservation_id: &str,
    expires_at: i64,
) -> Result<()>;
pub fn invalidate_credential_keys(&self, credential_hash: &[u8]) -> Result<()>;
```

#### 3. MlsClient API (Add to `src/client.rs`)

```rust
impl MlsClient {
    /// Refresh key package pool (check expiry, replenish if needed)
    /// Called periodically from CLI loop
    pub async fn refresh_key_packages(&mut self) -> Result<()> {
        self.connection.refresh_key_packages().await
    }
}
```

#### 4. MlsConnection Updates (Update: `src/mls/connection.rs`)

```rust
impl MlsConnection {
    /// Refresh key package pool state and replenish if needed
    pub async fn refresh_key_packages(&mut self) -> Result<()> {
        let pool = KeyPackagePool::new(self.username.clone());

        // Remove expired keys
        let expired = self.metadata_store.get_expired_keys(&self.username)?;
        for ref_hash in expired {
            self.metadata_store.delete_key_package(&ref_hash)?;
            log::debug!("Removed expired key package: {}", ref_hash);
        }

        // Check if replenishment needed
        if pool.should_replenish(&self.metadata_store).await? {
            if let Some(count_needed) = pool.get_replenishment_needed(&self.metadata_store).await? {
                log::info!("Replenishing key packages: {} needed", count_needed);
                pool.generate_and_store(count_needed, &self.metadata_store).await?;
                // Upload to server
                self.upload_key_packages(&self.metadata_store).await?;
            }
        }

        Ok(())
    }

    async fn upload_key_packages(&self, store: &LocalStore) -> Result<()> {
        // Call api.upload_key_packages() with pending keys
        // Remove from local storage after successful upload
    }
}
```

#### 5. Server API Updates (Update: `client/rust/src/api.rs`)

New endpoints:

```rust
// Upload a batch of KeyPackages
pub async fn upload_key_packages(
    &self,
    username: &str,
    keypackages: Vec<KeyPackageBundle>,
) -> Result<KeyPackageUploadResponse>;

// Reserve a KeyPackage (with TTL) - replaces get_user_key() for invitations
pub async fn reserve_key_package(
    &self,
    target_username: &str,
    group_id: &[u8],
) -> Result<ReservedKeyPackage>;

// Spend a KeyPackage (after Commit posted)
pub async fn spend_key_package(
    &self,
    keypackage_ref: &str,
    group_id: &[u8],
    added_by: &str,
) -> Result<()>;

// Get pool status (health monitoring)
pub async fn get_key_package_status(
    &self,
    username: &str,
) -> Result<KeyPackageStatusResponse>;
```

#### 6. MlsMembership Updates (Update: `src/mls/membership.rs`)

```rust
// Updated to use reserve/spend model instead of get_user_key()
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

## Implementation Roadmap

### Phase 2.0: Server-Side KeyPackage Storage (FOUNDATIONAL - Enables Client Tests)

**Rationale:** Implement server storage first so client unit tests can use the server library to create realistic test data without needing to mock the database layer.

**Files to create/modify:**
- `server/src/db/keypackage_store.rs` (NEW - KeyPackage storage module)
- `server/src/db/mod.rs` (MODIFY - add keypackage_store module)
- `server/tests/keypackage_store_tests.rs` (NEW - unit tests)

**Tasks:**
1. Design KeyPackage data structures for server-side storage
2. Implement SQLite schema for server keypackages table
3. Implement KeyPackageStore struct with methods:
   - `save_key_package()` - Store bundle and metadata
   - `get_key_package()` - Retrieve by ref
   - `list_available_for_user()` - Pool queries
   - `reserve_key_package()` - Mark as reserved with TTL
   - `spend_key_package()` - Mark as spent
   - `cleanup_expired()` - Garbage collect
4. Add double-spend prevention logic
5. Add TTL enforcement for reservations (60s timeout)

**Server Schema (Phase 2.0):**
```sql
CREATE TABLE keypackages (
    keypackage_ref BLOB NOT NULL,
    username TEXT NOT NULL,

    keypackage_bytes BLOB NOT NULL,

    -- Server-side tracking
    uploaded_at INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'available',  -- available|reserved|spent

    -- Reservation details
    reservation_id TEXT UNIQUE,
    reservation_expires_at INTEGER,
    reserved_by TEXT,

    -- Spend details
    spent_at INTEGER,
    spent_by TEXT,
    group_id BLOB,

    -- Metadata (from client upload)
    not_after INTEGER NOT NULL,
    credential_hash BLOB,
    ciphersuite INTEGER,

    PRIMARY KEY (username, keypackage_ref),
    INDEX idx_user_status ON keypackages(username, status),
    INDEX idx_user_expiry ON keypackages(username, not_after),
    INDEX idx_reservation ON keypackages(reservation_id)
);
```

**Success Criteria:**
- [ ] Unit test: Save and retrieve KeyPackage by ref
- [ ] Unit test: Double-spend prevention (reject reserve of already-spent key)
- [ ] Unit test: TTL enforcement (reservation timeout)
- [ ] Unit test: Expiry cleanup removes expired keys
- [ ] Unit test: List available keys filters correctly (status, expiry)
- [ ] Integration test: Multiple clients can reserve different keys concurrently
- [ ] Integration test: Reservation timeout releases key for reuse
- [ ] Unit test: Spend updates status and logs details
- [ ] All tests pass with in-memory SQLite (for speed)

**Estimate:** 2-3 days

---

### Phase 2.1: Client Storage Layer Enhancement (CRITICAL - Fixes Async Invitations)

**Files to modify/create:**
- `client/rust/src/storage.rs` (MODIFY - add keypackages table with Phase 1 schema)
- `client/rust/tests/storage_tests.rs` (MODIFY - add comprehensive keypackage tests)

**Tasks:**
1. Add Phase 1 `keypackages` table to LocalStore schema (5 fields: ref, bytes, init_key, enc_key, created_at)
2. Implement `save_key_package_bundle()` - stores complete KeyPackageBundle
3. Implement `load_key_package_bundle()` - retrieves by keypackage_ref (handles not found)
4. Implement `get_key_package_bundle()` - retrieves or errors if not found
5. Implement `delete_key_package_bundle()` - removes after consumption

**CRITICAL FIELDS:**
- **keypackage_ref**: BLOB PRIMARY KEY (hash, ciphersuite-dependent: 32/48/64 bytes)
- **keypackage_bytes**: BLOB (public part for re-upload/validation)
- **private_init_key**: BLOB (required for Welcome decryption)
- **private_encryption_key**: BLOB (required for group operations)
- **created_at**: INTEGER (timestamp for tracking)

**Success Criteria:**
- [ ] Unit test: Save and load KeyPackageBundle preserves all 3 private fields
- [ ] Unit test: Load with missing ref returns None (error handling)
- [ ] Unit test: Get with missing ref returns error
- [ ] Unit test: Delete removes bundle and all private keys
- [ ] Unit test: Private keys can be retrieved by keypackage_ref (not spoofable)
- [ ] **Integration test:** Generate real KeyPackageBundle, upload to server (via server library), client retrieves from server, stores, verifies fields match
- [ ] **Integration test:** Welcome message decrypts using persisted private_init_key (uses server::keypackage_store to simulate server state)
- [ ] **Integration test:** Multiple bundles can coexist without key collisions (uses server to manage state)
- [ ] Unit test: keypackage_ref is deterministic (same input = same hash)
- [ ] No regression in existing identity storage tests
- [ ] No dangling private keys after delete
- [ ] **Property test:** Can round-trip KeyPackageBundle through server→client→storage without data loss

**Testing approach (using server library):**
```rust
// In client tests
use mls_chat_server::db::KeyPackageStore;  // Import server lib
use tempfile::TempDir;

#[test]
fn test_client_receives_keypackage_from_server() {
    let server_db = TempDir::new().unwrap();
    let server_store = KeyPackageStore::new(server_db.path()).unwrap();

    // Create a KeyPackageBundle and upload to server
    let bundle = generate_test_keypackage();
    let ref_hash = compute_keypackage_ref(&bundle);
    server_store.save_key_package("alice", &bundle, &ref_hash).unwrap();

    // Client retrieves from server
    let retrieved = server_store.get_key_package(&ref_hash).unwrap();

    // Client stores in LocalStore
    let client_store = LocalStore::new(client_db_path).unwrap();
    client_store.save_key_package_bundle(
        &ref_hash,
        &retrieved.keypackage_bytes,
        &retrieved.private_init_key,
        &retrieved.private_encryption_key,
    ).unwrap();

    // Verify client can retrieve and use private keys
    let loaded = client_store.load_key_package_bundle(&ref_hash).unwrap();
    assert_eq!(loaded.private_init_key, bundle.private_init_key);
}
```

**Estimate:** 2-3 days (critical path - blocks all async features)

### Phase 2.2: Client KeyPackagePool Core Implementation

**Files to modify/create:**
- `client/rust/src/mls/keypackage_pool.rs` (NEW)
- `client/rust/tests/keypackage_pool_tests.rs` (NEW)

**Tasks:**
1. Create KeyPackagePool struct with configuration
2. Implement `generate_and_store()` - creates N key packages, stores in LocalStore
3. Implement `get_available_count()` - counts available keys in storage
4. Implement `should_replenish()` - checks if available < low_watermark
5. Implement `get_replenishment_needed()` - returns count to generate
6. Implement `mark_as_spent()` - updates status in storage
7. Implement expiry checking logic

**Success Criteria:**
- [ ] Unit tests for pool generation (verify N keys created)
- [ ] Unit tests for replenishment threshold logic
- [ ] Unit tests verify expiry detection and cleanup
- [ ] Unit tests verify status transitions work correctly
- [ ] All methods tested with LocalStore integration (not mocked)
- [ ] Tests verify OpenMLS KeyPackageBundle integration
- [ ] Property tests: available count + reserved count + spent count + expired count = total

**Estimate:** 2-3 days

### Phase 2.3: Client MlsConnection & MlsClient Integration

**Files to modify/create:**
- `client/rust/src/mls/connection.rs` (MODIFY - add refresh_key_packages())
- `client/rust/src/client.rs` (MODIFY - expose refresh_key_packages())
- `client/rust/tests/client_tests.rs` (MODIFY - add refresh tests)

**Tasks:**
1. Add `refresh_key_packages()` method to MlsConnection
2. Add `refresh_key_packages()` method to MlsClient
3. Implement expiry cleanup during refresh
4. Implement replenishment decision during refresh
5. Update error handling for pool operations

**Success Criteria:**
- [ ] Unit test: refresh removes expired keys from storage
- [ ] Unit test: refresh triggers replenishment when needed
- [ ] Unit test: refresh succeeds when no replenishment needed
- [ ] **Integration test (using server lib):** refresh works with real LocalStore and can query server state
- [ ] No background tasks spawned (synchronous only)

**Estimate:** 1-2 days

### Phase 2.4: Server REST API Endpoints

**Files to modify/create:**
- `server/src/routes/keypackages.rs` (NEW - REST endpoints)
- `server/src/main.rs` (MODIFY - register routes)
- `server/tests/keypackage_api_tests.rs` (NEW - integration tests)

**Tasks:**
1. Create REST endpoints using KeyPackageStore from Phase 2.0
2. Endpoint: `POST /keypackages/upload` - Client uploads batch
3. Endpoint: `POST /keypackages/reserve` - Reserve key for invite
4. Endpoint: `POST /keypackages/spend` - Mark key as spent
5. Endpoint: `GET /keypackages/status` - Pool health info
6. Add error handling and validation

**Success Criteria:**
- [ ] Integration test: Upload batch of KeyPackages
- [ ] Integration test: Reserve returns available key
- [ ] Integration test: Double-spend prevented
- [ ] Integration test: Expired keys rejected
- [ ] Integration test: TTL timeout releases reservation
- [ ] HTTP status codes correct (200/400/404/409)
- [ ] Clear error messages for all failure cases

**Estimate:** 2 days

### Phase 2.5: Client CLI Integration & Periodic Refresh

**Files to modify/create:**
- `client/rust/src/cli.rs` (MODIFY - call refresh_key_packages() periodically)
- `client/rust/src/client.rs` (MODIFY - track message count or add timer)

**Tasks:**
1. Add message counter to MlsClient (or use tokio timer)
2. Call `client.refresh_key_packages()` every N messages (e.g., 10)
3. Log refresh results (keys added, removed, etc.)
4. Handle refresh errors gracefully (log but don't break CLI)

**Success Criteria:**
- [ ] Unit test: refresh is called on correct message interval
- [ ] Integration test: CLI loop with refresh works end-to-end
- [ ] E2E test: multiple users can send messages and triggers refresh
- [ ] Refresh errors are logged but don't crash the client
- [ ] Property test: refresh is idempotent (multiple calls safe)

**Estimate:** 1-2 days

### Phase 2.6: Client Invitation & Spend Integration

**Files to modify/create:**
- `client/rust/src/mls/membership.rs` (MODIFY - use reserve/spend)
- `client/rust/src/api.rs` (MODIFY - add reserve/spend endpoints)
- `client/rust/tests/invitation_tests.rs` (MODIFY - test reservation)

**Tasks:**
1. Update `invite_user()` to reserve key package before invite
2. Update `add_to_group()` to spend key after commit posted
3. Handle reservation timeout errors (clear user message)
4. Handle pool exhaustion errors (suggest refresh or retry later)
5. Update error types with new pool-specific errors

**Success Criteria:**
- [ ] Unit test: reservation succeeds with available keys (uses server lib)
- [ ] Unit test: reservation fails when pool exhausted (uses server lib)
- [ ] Unit test: spend marks key as spent in storage (uses server lib)
- [ ] Integration test: invite → reserve → spend flow works
- [ ] E2E test: multiple concurrent invites use different keys
- [ ] E2E test: pool exhaustion error is clear and actionable

**Estimate:** 2-3 days

### Phase 2.7: Documentation & End-to-End Testing

**Files to modify/create:**
- `docs/keypackage-implementation.md` (NEW - operation guide)
- `client/rust/e2e_tests/test_pool_replenishment.expect` (NEW - E2E test)

**Test scenarios to cover (in addition to above):**
1. Concurrent invitations consume different keys from pool
2. Expiry and rotation (old keys cleaned up)
3. Server pool exhaustion error propagates clearly
4. Pool refresh maintains pool health
5. Pool state survives client restart (persistent storage)

**Success Criteria:**
- [ ] All test scenarios pass
- [ ] E2E test: pool starts at 1 key, grows to 32 via refresh
- [ ] E2E test: multiple users can invite simultaneously without conflicts
- [ ] Documentation describes pool lifecycle and refresh mechanism
- [ ] README updated with pool limitations (32-key max, expiry windows)

**Estimate:** 2-3 days

## Technical Details

### KeyPackage Expiry Strategy

```rust
const KEYPACKAGE_LIFETIME_DAYS: i64 = 14;
const EXPIRY_WARNING_THRESHOLD_DAYS: i64 = 2;

fn generate_keypackage_with_lifetime(
    credential: &CredentialWithKey,
    signature_key: &SignatureKeyPair,
    provider: &OpenMlsProvider,
) -> Result<KeyPackageBundle> {
    let now = SystemTime::now();
    let not_before = now;
    let not_after = now + Duration::days(KEYPACKAGE_LIFETIME_DAYS);

    // Generate with Extensions containing validity bounds
    // OpenMLS KeyPackage includes lifetime_extension
    let bundle = KeyPackageBundle::generate(
        &provider,
        credential,
        signature_key,
        lifetime: not_after - not_before,
    )?;

    Ok(bundle)
}

fn is_expired(keypackage: &KeyPackageMetadata) -> bool {
    SystemTime::now() > keypackage.not_after
}

fn days_until_expiry(keypackage: &KeyPackageMetadata) -> i64 {
    (keypackage.not_after - SystemTime::now()).as_secs() / 86400
}
```

### Pool Replenishment Logic

```rust
impl KeyPackageManager {
    pub async fn should_replenish(&self) -> bool {
        let available_count = self.keypackages
            .iter()
            .filter(|kp| matches!(kp.status, KeyPackageStatus::Available))
            .count();

        // Trigger when available keys < low_watermark
        available_count < self.low_watermark && !self.replenishment_in_progress
    }

    pub async fn replenishment_needed(&self) -> Option<usize> {
        let available = self.available_count();
        let reserved = self.reserved_count();

        if available + reserved < self.target_pool_size {
            let needed = self.target_pool_size - (available + reserved);
            Some(needed)
        } else {
            None
        }
    }
}
```

### Server Reservation Model (Pseudocode)

```rust
// Server-side reservation tracking
pub struct KeyPackageReservation {
    keypackage_ref: String,
    reserved_by: String,           // Adder username
    reserved_at: SystemTime,
    reservation_ttl: Duration,     // ~60 seconds
    group_id: Vec<u8>,
}

pub async fn reserve_key_package(
    &self,
    target_user: &str,
    group_id: &[u8],
) -> Result<ReservedKeyPackage> {
    let mut pool = self.get_user_pool(target_user)?;

    // Find first available, non-expired key
    let key = pool.keypackages
        .iter_mut()
        .find(|kp| kp.status == KeyPackageStatus::Available && !is_expired(kp))?
        .ok_or(ClientError::NoAvailableKeyPackages)?;

    // Mark as reserved with TTL
    key.status = KeyPackageStatus::Reserved {
        reserved_at: SystemTime::now(),
        reserved_by: self.username.clone(),
    };

    self.db.save_pool(&pool)?;

    Ok(ReservedKeyPackage {
        keypackage_ref: key.ref_hash.clone(),
        keypackage: key.bytes.clone(),
        expires_at: key.not_after,
    })
}

pub async fn spend_key_package(
    &self,
    keypackage_ref: &str,
    group_id: &[u8],
    added_by: &str,
) -> Result<()> {
    let mut pool = self.find_pool_with_key(keypackage_ref)?;
    let key = pool.keypackages
        .iter_mut()
        .find(|kp| kp.ref_hash == keypackage_ref)?
        .ok_or(ClientError::KeyPackageNotFound)?;

    // Prevent double-spend
    if matches!(key.status, KeyPackageStatus::Spent { .. }) {
        return Err(ClientError::KeyPackageAlreadySpent);
    }

    key.status = KeyPackageStatus::Spent {
        spent_at: SystemTime::now(),
    };

    // Optional: Log to audit trail
    self.db.log_spend(SpendRecord {
        keypackage_ref: keypackage_ref.to_string(),
        added_by: added_by.to_string(),
        group_id: group_id.to_vec(),
        timestamp: SystemTime::now(),
        status: "spent",
    })?;

    self.db.save_pool(&pool)?;
    Ok(())
}
```

## Files Modified/Created Summary

### Client (Rust)

**New:**
- `client/rust/src/mls/keypackage_pool.rs` - KeyPackagePool struct and core logic
- `client/rust/tests/keypackage_pool_tests.rs` - Unit tests for pool operations

**Modified:**
- `client/rust/src/storage.rs` - Add keypackages table and CRUD methods
- `client/rust/tests/storage_tests.rs` - Add storage layer tests
- `client/rust/src/mls/connection.rs` - Add `refresh_key_packages()` method
- `client/rust/src/client.rs` - Expose `refresh_key_packages()` method
- `client/rust/src/cli.rs` - Call refresh periodically in main loop
- `client/rust/src/mls/membership.rs` - Use reserve/spend model for invitations
- `client/rust/src/api.rs` - Add upload, reserve, spend, status endpoints
- `client/rust/src/error.rs` - Add pool-specific error types
- `client/rust/tests/client_tests.rs` - Add refresh tests
- `client/rust/tests/invitation_tests.rs` - Add reservation tests

### Server (Rust)

**New:**
- `server/src/routes/keypackages.rs` - REST endpoints for pool operations
- `server/src/db/keypackage_store.rs` - KeyPackage pool storage and TTL logic
- `server/tests/keypackage_api_tests.rs` - Server-side tests

**Modified:**
- `server/src/main.rs` - Register keypackage routes
- `server/src/db/mod.rs` - Add keypackage_store module

### Documentation

**New:**
- `docs/keypackage-implementation.md` - Pool operation guide and architecture
- `client/rust/e2e_tests/test_pool_replenishment.expect` - E2E test for pool refresh

## Dependencies & Risks

### Dependencies
- **Phase 1 completion** (error handling fixes) must be done first
- **Server API changes** require coordination with server implementation
- **No external dependencies**: Uses existing SQLite (LocalStore), OpenMLS, tokio, reqwest

### Risks

1. **Synchronous Refresh Latency:** CLI refresh blocks user input while generating/uploading keys
   - *Mitigation:* Keep refresh fast (generate once, not on every message); cache results
   - *Alternative:* Use tokio select! in CLI loop to make refresh non-blocking

2. **Server Synchronization:** Client pool state may drift from server (e.g., after network failure)
   - *Mitigation:* Periodic status sync, fallback to on-demand generation; skip spend if key not found

3. **Storage Overhead:** Storing 32+ KeyPackages locally requires ~32KB per user
   - *Mitigation:* Prune expired keys immediately, hard cap of 64 keys enforced

4. **Timing Sensitivity:** Reservation TTL requires synchronized clocks (client/server)
   - *Mitigation:* Server uses lenient TTL (e.g., 120s instead of 60s), skip spend on 404

5. **Refresh Call Frequency:** If refresh is called too often, it may generate excess keys
   - *Mitigation:* Implement cooldown between refresh calls (e.g., 1 minute minimum)

## Rollout Strategy

1. **Code Review:** Full review of all new modules and modifications
2. **Unit Tests:** Verify each phase passes its success criteria tests
3. **Integration Tests:** Verify client-server pool operations end-to-end
4. **Load Testing:** Test with concurrent invitations, pool exhaustion scenarios
5. **Gradual Rollout:** Deploy server changes first (backward-compatible), then client
6. **Monitoring:** Log pool health metrics, replenishment events, errors
7. **Fallback:** Maintain single-key-package fallback for degradation scenarios

## Overall Success Criteria (Phase 2 Complete)

**Functional Requirements:**
- [ ] All 7 phases implemented and tested
- [ ] Client can generate and store multiple KeyPackages (target 32)
- [ ] KeyPackage pool automatically replenishes when < 25% available
- [ ] Expired KeyPackages are cleaned up automatically
- [ ] Multiple concurrent invitations consume different KeyPackages from pool
- [ ] Server prevents double-spend of KeyPackages
- [ ] Pool state persists across client restarts
- [ ] Clear error messages for pool exhaustion, expiry, other failures

**Code Quality:**
- [ ] All new code passes clippy linting
- [ ] All unit tests pass with 100% coverage of new modules
- [ ] All integration tests pass with real client/server
- [ ] E2E test (test_pool_replenishment.expect) passes
- [ ] No background tasks spawned (synchronous only)
- [ ] No regression in Phase 1 or existing functionality
- [ ] All CLAUDE.md guidelines followed (file headers, service layer separation, etc.)

**Architecture Requirements:**
- [ ] KeyPackagePool in `src/mls/keypackage_pool.rs` (single module, not nested package)
- [ ] All pool state persisted in LocalStore (storage.rs)
- [ ] MlsClient exposes `refresh_key_packages()` method
- [ ] CLI calls refresh from main loop (every N messages or timer)
- [ ] Reservation model used for invitations (not single get_user_key)
- [ ] Server API: upload, reserve, spend, status endpoints implemented
- [ ] All error types properly defined in error.rs

**Documentation:**
- [ ] Implementation guide: `docs/keypackage-implementation.md`
- [ ] README updated with pool features and limitations
- [ ] Code comments explain pool lifecycle and refresh strategy
- [ ] Test documentation explains each scenario covered

## Implementation Checklist

Phase 2.0 (Server Storage - Foundational):
- [ ] KeyPackageStore struct implemented
- [ ] Keypackages table created
- [ ] All CRUD methods working
- [ ] Double-spend prevention tested
- [ ] TTL enforcement tested
- [ ] Unit tests pass
- **Exports:** `server::db::KeyPackageStore` available for import in client tests

Phase 2.1 (Client Storage - Fixes Async):
- [ ] Keypackages table created
- [ ] All CRUD methods implemented
- [ ] Uses server lib in integration tests
- [ ] Welcome decryption test passes
- [ ] No dangling keys after delete

Phase 2.2 (Client Pool Core):
- [ ] KeyPackagePool struct implemented
- [ ] Generation, counting, replenishment logic tested
- [ ] Expiry cleanup tested
- [ ] Property tests for pool invariants

Phase 2.3 (Client Integration):
- [ ] refresh_key_packages() added to Connection and Client
- [ ] Refresh tested with LocalStore and server lib
- [ ] No background tasks

Phase 2.4 (Server REST API):
- [ ] 4 endpoints implemented
- [ ] All tested with KeyPackageStore
- [ ] Error handling complete

Phase 2.5 (CLI Integration):
- [ ] Periodic refresh in CLI loop
- [ ] Error handling tested
- [ ] Idempotency verified

Phase 2.6 (Client Invitation Integration):
- [ ] Invite uses reserve/spend model
- [ ] Uses server lib in tests
- [ ] Concurrent invites tested
- [ ] Pool exhaustion errors tested

Phase 2.7 (Testing & Docs):
- [ ] All test scenarios covered
- [ ] E2E test passes
- [ ] Documentation complete

## Next Steps

1. **Immediate:** Get user approval for this reorganized plan (Phase 2.0 first)

2. **Phase 2.0 - START HERE:** Server-side KeyPackage storage
   - Implement KeyPackageStore with SQLite backend
   - Tests use in-memory SQLite for speed
   - Export as public library for client tests to import
   - Estimated: 2-3 days

3. **Phase 2.1 - UNBLOCKS ASYNC:** Client-side KeyPackage storage
   - Implement LocalStore.keypackages table
   - Integration tests use `server::db::KeyPackageStore` to simulate server
   - Verify Welcome messages decrypt using persisted private_init_key
   - Estimated: 2-3 days

4. **During Implementation:**
   - Update this changelog with progress and obstacles
   - Mark each phase complete when success criteria met
   - Add any learnings or architectural adjustments discovered
   - Each phase blocks until success criteria pass

5. **Post-Phase 2:** Consider Phase 3 enhancements (transparency logs, advanced audit, credential rotation flows)

## Obstacles & Solutions

### Phase 2.0 Implementation (2025-10-28)

**Started:** 2025-10-28
**Status:** In Progress

**Implementation Notes:**
- Reviewed existing server patterns in `server/src/db/mod.rs`, `models.rs`, `init.rs`
- Server uses `rusqlite` with async via `DbPool = Arc<Mutex<Connection>>`
- Timestamps stored as TEXT in RFC3339 format (e.g., using `chrono::Utc::now().to_rfc3339()`)
- Standard pattern: DbPool-based methods, optional() for Option results, tests use in-memory DB
- Creating KeyPackageStore as separate module following existing architecture

---

**Created:** 2025-10-28
**Last Updated:** 2025-10-28 (Phase 2.0 implementation started)
**Status:** Implementation - Phase 2.0
**Phase:** 2 (Production-ready pool implementation)
