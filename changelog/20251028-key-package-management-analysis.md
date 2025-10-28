# Key Package Management Analysis

**Date:** 2025-10-28
**Task:** Research and analyze options for improving key package management in the MLS Rust client

## Task Specification

Research architectural improvements to key package management based on two identified problems:

1. **Problem 1:** Ambiguous server state on `get_user_key()` failure
   - Location: `src/mls/connection.rs:220`
   - Issue: Error handling assumes "user doesn't exist" but could be server/network error
   - Risk: Generates new key package when user already registered

2. **Problem 2:** Silent failure of key package registration
   - Location: `src/mls/connection.rs:247`
   - Issue: `register_user()` failure is silently ignored with `let _ = ...`
   - Risk: User is "orphaned" - cannot be invited to groups

## Research Questions

1. How to handle `get_user_key()` errors correctly (differentiate 404 from other errors)?
2. How to handle registration failures (strict vs graceful degradation)?
3. Integration with current architecture (where to track state, impact on group operations)
4. OpenMLS best practices for key package management

## Key Insights from OpenMLS Documentation

- Key packages are pre-published on Delivery Service (server)
- Used for asynchronous group additions (don't need user present)
- Private keys kept locally, fetched when key package is consumed
- Can generate multiple key packages ahead of time
- Essential for async aspect of MLS protocol

## Analysis Progress

- [x] Examine current code structure in `src/mls/connection.rs`
- [x] Review API error handling in client
- [x] Analyze OpenMLS key package lifecycle
- [x] Compare option architectures
- [x] Provide recommendations

## Files Examined

- `/home/kena/src/quintessence/mls-chat/client/rust/src/mls/connection.rs` (lines 178-251)
- `/home/kena/src/quintessence/mls-chat/client/rust/src/api.rs` (lines 51-125)
- `/home/kena/src/quintessence/mls-chat/client/rust/src/error.rs`
- `/home/kena/src/quintessence/mls-chat/client/rust/src/identity.rs`
- `/home/kena/src/quintessence/mls-chat/client/rust/tests/api_tests.rs`
- `/home/kena/src/quintessence/mls-chat/docs/mls-explanation.md`

## Current Implementation Analysis

### Error Handling in api.rs

1. `get_user_key()` returns:
   - `Ok(Vec<u8>)` - User found, key package returned
   - `Err(NetworkError::Server("User not found"))` - HTTP 404
   - `Err(NetworkError::Server(format!("Failed to get user key: {}")))` - Other HTTP errors
   - `Err(NetworkError::Http(reqwest::Error))` - Network/connection errors

2. `register_user()` has smart idempotent handling:
   - HTTP 200/201: Success
   - HTTP 409 Conflict: Validates remote key matches local key
     - Match: Returns Ok(())
     - Mismatch: Returns security error about identity compromise
   - Other errors: Returns Err

### Identity Persistence (identity.rs)

- `IdentityManager::load_or_create()` handles persistent identities
- Signature keys stored in OpenMLS provider storage
- Public key reference stored in LocalStore metadata
- Designed for identity reuse across sessions

### Current initialize() Flow (connection.rs:178-251)

```
1. Load or create identity via IdentityManager
2. Create MlsUser with identity material
3. Try get_user_key() from server:
   - Ok(remote_key) → Use remote key package (line 212-218)
   - Err(_) → Generate NEW key package (line 220-238)
4. Store user in connection (line 243)
5. Register with server (silently ignoring errors) (line 247)
```

## Key Insights

### OpenMLS Key Package Architecture

From OpenMLS design:
- Key packages are "pre-published" on Delivery Service
- Used for asynchronous group additions
- Can generate multiple ahead of time
- Private keys kept locally, fetched when package consumed
- Each key package is single-use (consumed when user added to group)

### Current Code Issues

**Problem 1 Context:**
- Line 220: `Err(_)` is too broad
- Could be 404 (user not registered), 500 (server error), timeout, etc.
- Generating new key package on server error is WRONG

**Problem 2 Context:**
- Line 247: `let _ = self.api.register_user(...)`
- Silently fails in all error cases
- User orphaned: exists locally but not on server
- Other users cannot invite this user (no key package available)

## Recommendations

### Problem 1: Differentiate Error Cases

**Recommended: Option A with Enhancement**

Differentiate 404 from other errors using error types:

```rust
match self.api.get_user_key(&self.username).await {
    Ok(remote_key_package) => {
        // User exists on server - use their key package
        remote_key_package
    }
    Err(ClientError::Network(NetworkError::Server(msg))) if msg.contains("User not found") => {
        // True 404: Generate new key package
        generate_new_key_package()
    }
    Err(e) => {
        // Server unavailable, network error, etc - propagate error
        return Err(e);
    }
}
```

**Why Option A:**
- Aligns with current error type structure (already differentiates 404)
- Clear separation: 404 = "proceed with registration", other = "fail fast"
- No local state tracking needed
- Idempotent: safe to retry on network failures

**Rejected Options:**
- Option B (local registration state): Adds complexity, doesn't solve server state mismatch
- Option C (try register first): Conflicts with current idempotent register_user design
- Option D (multi-step flow): Too complex, Option A is simpler

### Problem 2: Handle Registration Failures Properly

**Recommended: Hybrid of Option A + Option C**

Strict mode with limited retry capability:

```rust
// After generating/retrieving key package
self.user = Some(user);

// Attempt registration - fail if it fails
self.api.register_user(&self.username, &key_package_bytes).await?;

log::info!("User {} successfully registered with server", self.username);
Ok(())
```

**Why Strict Mode:**
- Strong guarantee: initialize() success means user is invitable
- Aligns with OpenMLS design: key packages must be on server for async invites
- Fail fast on server issues (better than silent degradation)
- register_user() already handles idempotency (409 Conflict with key match)

**Future Enhancement (Option C - multiple key packages):**
- Could add separate method: `refresh_key_package()` or `publish_additional_key_packages()`
- MLS best practice: publish multiple key packages ahead of time
- Current single key package is acceptable for MVP

**Rejected Options:**
- Option B (graceful degradation): User thinks they're registered but isn't invitable - bad UX
- Option D (post-initialization): Doesn't guarantee user is invitable after initialize()

## Architectural Changes Needed

### 1. Error Type Enhancement

Add variant to distinguish 404 specifically:

```rust
// In error.rs
#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Server error: {0}")]
    Server(String),

    #[error("Resource not found: {0}")]
    NotFound(String),  // NEW

    #[error("Connection timeout")]
    Timeout,
}
```

Update `api.rs`:

```rust
pub async fn get_user_key(&self, username: &str) -> Result<Vec<u8>> {
    let response = self.client
        .get(format!("{}/users/{}", self.base_url, username))
        .send()
        .await?;

    if response.status().is_success() {
        let user_key: UserKeyResponse = response.json().await?;
        Ok(user_key.key_package)
    } else if response.status() == StatusCode::NOT_FOUND {
        Err(NetworkError::NotFound(format!("User '{}' not found", username)).into())
    } else {
        Err(NetworkError::Server(format!("Failed to get user key: {}", response.status())).into())
    }
}
```

### 2. Update initialize() Method

```rust
pub async fn initialize(&mut self) -> Result<()> {
    log::info!("Initializing MlsConnection for {}", self.username);

    // Step 1: Load or create persistent identity
    let stored_identity = IdentityManager::load_or_create(
        &self.mls_provider,
        &self.metadata_store,
        &self.username,
    )?;

    let keypair_blob = stored_identity.signature_key.to_public_vec();

    // Step 2: Create MlsUser with identity material
    let identity = Identity {
        username: self.username.clone(),
        keypair_blob: keypair_blob.clone(),
        credential_blob: vec![],
    };

    let user = MlsUser::new(
        self.username.clone(),
        identity,
        stored_identity.signature_key,
        stored_identity.credential_with_key.clone(),
    );

    // Step 3: Generate or retrieve key package
    let key_package_bytes = match self.api.get_user_key(&self.username).await {
        Ok(remote_key_package) => {
            log::info!(
                "Found existing key package for {} on server",
                self.username
            );
            // TODO: Validate remote key package matches local identity
            remote_key_package
        }
        Err(ClientError::Network(NetworkError::NotFound(_))) => {
            // User genuinely doesn't exist - generate new key package
            log::info!("User {} not registered, generating new key package", self.username);

            let key_package_bundle = crypto::generate_key_package_bundle(
                user.get_credential_with_key(),
                user.get_signature_key(),
                &self.mls_provider,
            )?;

            use tls_codec::Serialize as TlsSerialize;
            key_package_bundle
                .key_package()
                .tls_serialize_detached()
                .map_err(|_e| ClientError::Mls(
                    crate::error::MlsError::OpenMls("Failed to serialize key package".to_string())
                ))?
        }
        Err(e) => {
            // Server unavailable, network error, etc - fail initialization
            log::error!("Failed to check server registration for {}: {}", self.username, e);
            return Err(e);
        }
    };

    // Step 4: Store user (before registration to maintain ordering)
    self.user = Some(user);

    // Step 5: Register with server - MUST succeed
    self.api.register_user(&self.username, &key_package_bytes).await?;

    log::info!("MlsConnection initialized and registered for {}", self.username);
    Ok(())
}
```

### 3. No Changes to MlsUser, MlsMembership

These components are unaffected by key package management changes.

### 4. New Error Type for Registration State (Future)

If implementing multiple key packages later:

```rust
pub struct KeyPackageManager {
    pending_packages: Vec<Vec<u8>>,
    published_count: usize,
    needs_refresh: bool,
}
```

## Impact Analysis

### Impact on initialize()

**Before:**
- Always succeeds even if server unavailable
- User may not be invitable

**After:**
- Fails fast if server unavailable (clear error message)
- Success guarantees user is registered and invitable
- Network errors propagated clearly

### Impact on Tests

**connection.rs tests (lines 622-646):**

```rust
#[tokio::test]
async fn test_connection_initialization_creates_user() {
    // ...
    let _ = connection.initialize().await;  // Currently ignores error

    // AFTER: Need to handle server unavailable
    // Option 1: Mock server
    // Option 2: Expect error when server unavailable
    assert!(
        connection.initialize().await.is_err(),
        "Initialize should fail without server"
    );
}
```

**api_tests.rs:**
- Already tests 404 handling (test_get_nonexistent_user)
- Add test for server unavailable during initialize

### Impact on Group Creation

**connect_to_group() and create_group():**
- No changes needed
- These already assume initialize() succeeded
- If initialize() fails, connection never reaches usable state

### Impact on Invitations

**invite_user() in membership.rs:**
- No changes needed
- Already calls `api.get_user_key(invitee)` which properly handles errors
- 404 = "user not registered" (clear error)
- Other errors = "server unavailable" (clear error)

## OpenMLS Best Practices

### Single vs Multiple Key Packages

**Current: Single Key Package**
- Acceptable for MVP
- Matches OpenMLS minimal usage pattern
- Users can be invited to multiple groups using same key package

**Future: Multiple Key Packages (OpenMLS Best Practice)**

From OpenMLS design:
- Publish multiple key packages ahead of time
- Each consumed when user added to group
- Reduces server round-trips
- Better privacy (different key per group)

**Implementation Path:**
1. Current: Single key package (sufficient)
2. Phase 2: Batch generation: `publish_key_packages(count: usize)`
3. Phase 3: Background refresh: `ensure_available_key_packages(min: usize)`

### Key Package Lifecycle

OpenMLS intended lifecycle:
1. **Generate**: Create multiple key packages offline
2. **Publish**: Upload to Delivery Service (server)
3. **Consume**: Server gives package to inviter, marks as used
4. **Refresh**: Client periodically uploads new packages

Current implementation simplifies to:
1. Generate single key package on demand
2. Publish during initialize()
3. Reuse for all group invitations

This is acceptable but not optimal for production.

## Test Strategy

### New Tests Needed

1. **Test server unavailable during initialize:**
   ```rust
   #[tokio::test]
   async fn test_initialize_fails_without_server() {
       // No server running
       let connection = MlsConnection::new(...);
       let result = connection.initialize().await;
       assert!(result.is_err());
       assert!(matches!(result.unwrap_err(), ClientError::Network(_)));
   }
   ```

2. **Test 404 triggers key generation:**
   ```rust
   #[tokio::test]
   async fn test_initialize_generates_key_for_new_user() {
       // Mock server returns 404
       // Verify new key package generated and registered
   }
   ```

3. **Test existing user uses remote key:**
   ```rust
   #[tokio::test]
   async fn test_initialize_uses_remote_key_for_existing_user() {
       // Register user first
       // Initialize again
       // Verify remote key used (not regenerated)
   }
   ```

4. **Test registration failure propagates:**
   ```rust
   #[tokio::test]
   async fn test_initialize_fails_on_registration_error() {
       // Mock server accepts get_user_key but rejects register_user
       // Verify error propagated
   }
   ```

## STRATEGIC CORRECTION - October 28, 2025

### Correction 1: Pool Strategy (From Strategy Doc Review)

**After Review of docs/keypackage-pool-strategy.md:**

The original recommendation for a single key package was **incorrect**. The proper MLS strategy is a **pool-based approach** with multiple key packages to support:

1. **Concurrency**: Multiple simultaneous invitations without blocking
2. **Asynchrony**: No need to wait for target device to come online
3. **Security**: One-time use, compromise containment, expiry-based rotation
4. **Reliability**: Reservation timeouts, exhaustion handling, clear error messages

Key strategic differences from original analysis:
- **NOT** a single key package per device
- **Pool of 32 KeyPackages** with 25% low-watermark replenishment
- **Each KeyPackage is single-use** (consumed when added to group)
- **Expiry & rotation** required (~7-14 days lifetime)
- **Reservation model** on server to prevent double-spend during concurrent adds

This alignment with MLS best practices is critical for production deployment.

### Correction 2: Storage Requirements (From OpenMLS Investigation)

**Critical Discovery from OpenMLS Analysis:**

The original implementation plan **drastically underestimated storage requirements**. The client must persistently store the complete `KeyPackageBundle` structure, not just metadata:

**What must be stored:**
1. **keypackage_bytes** (public part) - Serialized KeyPackage for server re-upload and validation
2. **private_init_key** (HPKE private key) - **CRITICAL**: Decrypts Welcome messages after restart
3. **private_encryption_key** (leaf private key) - **CRITICAL**: Enables group operations
4. **keypackage_ref** (BLOB hash) - Primary key, used by OpenMLS to look up private keys

**Why it matters:**
- Without `private_init_key` persisted, Welcome messages **cannot be decrypted** after client restart
- This breaks the entire async invitation feature
- Current code uses **in-memory storage** only, making async invitations impossible
- OpenMLS expects storage provider to persist complete KeyPackageBundle

**Immediate implications:**
- Phase 1 (MVP) must fix this **first** before any pool implementation
- Phase 2.1 becomes critical path
- Async invitations are currently broken - this must be fixed
- Storage layer is bottleneck for entire feature

See `changelog/20251028-keypackage-pool-implementation-plan.md` Phase 2.1 for detailed success criteria.

## Revised Implementation Strategy

**Phase 1 (MVP) - Current Focus:**
- Single KeyPackage per device on initialization (acceptable starting point)
- Fix error handling (Option A + strict mode)
- Ensure server registration is mandatory
- Clear error differentiation (404 vs server errors)

**Phase 2 (Production) - Pool Strategy:**
- Implement KeyPackage pool (32 target)
- Background replenishment when pool < 25% (8 keys)
- Batch generation and upload
- Expiry-aware selection
- Pool status monitoring

**Phase 3 (Advanced):**
- Server-side reservation tracking
- Spend log and equivocation detection
- Device alerts for exhaustion
- Transparent audit trails

## Summary

**Decision: Immediate Phase 1 (Option A + strict mode) + roadmap for Phase 2 (pool strategy)**

**Rationale:**
- Phase 1 fixes critical bugs and enables MVP deployment
- Phase 1 error handling aligns with Phase 2 pool implementation
- Pool strategy transition is natural progression (no rework needed)
- Matches industry MLS best practices

**Implementation Priority:**
1. Add NotFound error variant
2. Update get_user_key() to return NotFound for 404
3. Update initialize() to differentiate errors
4. Change register_user() call from `let _ =` to `.await?`
5. Update tests for new error behavior
6. Plan Phase 2 implementation in separate changelog
