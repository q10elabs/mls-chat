# StorageProvider Trait Analysis

## Task Specification
Analyze the OpenMLS StorageProvider trait and its implementation to understand:
1. What StorageProvider does internally for KeyPackages
2. What key material it manages
3. How it compares to our Phase 2.2 implementation plan
4. Identify duplication vs. remaining work

## High-Level Findings

StorageProvider is OpenMLS's **abstraction layer for persisting all group and key material**. It defines what data must be stored, but NOT how. Implementations (like sqlite_storage) provide concrete persistence.

### What StorageProvider Does For KeyPackages

**Core Operations:**
1. **Stores** complete KeyPackageBundle (line 549 in openmls/src/key_packages/mod.rs)
   - Called automatically during `KeyPackageBuilder::build()`
   - Stores by hash reference (deterministic identifier)
   
2. **Retrieves** KeyPackageBundle by hash reference (line 567 in creation.rs)
   - Used during Welcome message processing
   - Returns: Complete KeyPackageBundle with all private keys
   
3. **Deletes** non-last-resort key packages after use (lines 573-576 in creation.rs)
   - Single-use model enforced at OpenMLS level
   - Last-resort keys preserved for fallback

**File Locations:**
- Trait definition: `/openmls/traits/src/storage.rs` (lines 221-238, 412-419, 561-568)
- KeyPackageBundle struct: `/openmls/openmls/src/key_packages/mod.rs` (lines 556-587)
- KeyPackage generation: `/openmls/openmls/src/key_packages/mod.rs` (lines 260-300, 517-553)
- Storage write call: `/openmls/openmls/src/key_packages/mod.rs` (line 549)
- Storage read call: `/openmls/openmls/src/group/mls_group/creation.rs` (lines 567)
- Storage delete call: `/openmls/openmls/src/group/mls_group/creation.rs` (lines 575)
- SQLite implementation: `/openmls/sqlite_storage/src/key_packages.rs` (entire file)
- SQLite schema: `/openmls/sqlite_storage/src/key_packages.rs` (lines 26-28, 51-52, 73-74)

---

## What Key Material Does StorageProvider Manage?

### Three Critical Keys Per KeyPackage

**1. Private Init Key (HPKE Private Key)**
- **Purpose:** Decrypt Welcome messages when joining a group
- **Type:** `HpkePrivateKey` 
- **Stored in:** `KeyPackageBundle::private_init_key` (line 564)
- **Used in:** `GroupSecrets::try_from_ciphertext()` at line 172 in creation.rs
- **Critical:** WITHOUT this, Welcome messages cannot be decrypted (asymmetric encryption)

**2. Private Encryption Key (HPKE Private Key)**
- **Purpose:** Decrypt and sign messages in group after joining
- **Type:** `EncryptionPrivateKey` (aka encryption_keypair.private_key)
- **Stored in:** `KeyPackageBundle::private_encryption_key` (line 565)
- **Used in:** `EncryptionKeyPair::from()` at line 582 in key_packages.rs
- **Critical:** Required for all group operations after welcome decryption

**3. Public Key Package Bytes**
- **Purpose:** Serialized public key package (the "advertisement" sent to server)
- **Stored in:** `KeyPackageBundle::key_package` field
- **Serialization:** TLS-encoded binary format
- **Used in:** Hash reference computation and Welcome decryption verification

### StorageProvider Interface For KeyPackages

```rust
// WRITE - Line 230-237 in traits/src/storage.rs
fn write_key_package<HashReference, KeyPackage>(
    &self,
    hash_ref: &HashReference,        // Key: deterministic hash
    key_package: &KeyPackage,        // Value: KeyPackageBundle
) -> Result<(), Self::Error>;

// READ - Line 412-419 in traits/src/storage.rs
fn key_package<KeyPackageRef, KeyPackage>(
    &self,
    hash_ref: &KeyPackageRef,
) -> Result<Option<KeyPackage>, Self::Error>;

// DELETE - Line 561-568 in traits/src/storage.rs
fn delete_key_package<KeyPackageRef: traits::HashReference<VERSION>>(
    &self,
    hash_ref: &KeyPackageRef,
) -> Result<(), Self::Error>;
```

### Associated Encryption Key Storage

StorageProvider ALSO manages the **encryption key pair** separately:

```rust
// Line 198-205 (write)
fn write_encryption_key_pair<EncryptionKey, HpkeKeyPair>(
    &self,
    public_key: &EncryptionKey,
    key_pair: &HpkeKeyPair,
) -> Result<(), Self::Error>;

// Line 386-397 (read)
fn encryption_key_pair<HpkeKeyPair, EncryptionKey>(
    &self,
    public_key: &EncryptionKey,
) -> Result<Option<HpkeKeyPair>, Self::Error>;

// Line 539-547 (delete)
fn delete_encryption_key_pair<EncryptionKey>(
    &self,
    public_key: &EncryptionKey,
) -> Result<(), Self::Error>;
```

**Note:** In current OpenMLS implementation, the encryption private key is stored INSIDE the KeyPackageBundle (line 545 in key_packages.rs), so it's automatically stored/deleted with the KeyPackage.

---

## SQLite Storage Implementation Details

**Schema** (`openmls/sqlite_storage/src/key_packages.rs` lines 26-28, 51-52):
```sql
CREATE TABLE openmls_key_packages (
    key_package_ref BLOB,          -- Hash reference (variable length)
    key_package BLOB,              -- Entire KeyPackageBundle serialized
    provider_version INTEGER       -- Schema version (currently 1)
);
```

**Implementation:**
- `StorableKeyPackage::load()` - Line 20-37: Queries by hash_ref, deserializes
- `StorableKeyPackageRef::store()` - Line 45-60: Inserts hash_ref and serialized bundle
- `StorableHashRef::delete_key_package()` - Line 68-82: Deletes by hash_ref

**Key Point:** The entire KeyPackageBundle (all three keys + public data) is serialized as a single BLOB. No separate tables for private keys.

---

## Integration With KeyPackage Generation Flow

**When `KeyPackageBuilder::build()` is called (line 517 in key_packages.rs):**

1. Calls `KeyPackage::create()` (line 529) which:
   - Generates random HPKE init key pair (line 275)
   - Generates leaf node encryption key pair (line 336)
   - Returns `KeyPackageCreationResult` with both private keys (lines 295-299)

2. Wraps in `KeyPackageBundle` (line 542)
   - Bundles: key_package + private_init_key + private_encryption_key

3. **Calls StorageProvider::write_key_package()** (line 549)
   - Persists entire bundle keyed by hash_ref
   - **This is AUTOMATIC and MANDATORY** - happens in the builder itself

4. Returns `KeyPackageBundle` to caller
   - Caller never manually stores - OpenMLS does it

**When Welcome message arrives (creation.rs line 551-580):**

1. Iterates through Welcome.secrets to find matching key package
2. **Calls StorageProvider::key_package()** (line 567)
   - Retrieves complete bundle from storage
3. Uses `init_private_key()` to decrypt welcome (line 172)
4. Deletes from storage if not last-resort (line 575)
   - **Single-use enforcement:** Non-last-resort keys destroyed after use

---

## Comparison To Phase 2.2 Plan

### What StorageProvider Already Handles (DON'T DUPLICATE)

1. **KeyPackageBundle persistence** 
   - ✓ Write complete bundle with all private keys
   - ✓ Read by hash reference
   - ✓ Delete by hash reference
   - ✓ Already encoded/decoded as serialized BLOB
   - **Status:** COMPLETE IN OPENMLS
   - **Our plan duplication:** Lines 161-180 in changelog/20251028-keypackage-pool-implementation-plan.md propose exact same fields (private_init_key, private_encryption_key)

2. **Hash reference computation**
   - ✓ OpenMLS computes hash_ref automatically (key_packages.rs line 374-383)
   - ✓ Deterministic - same bundle = same hash
   - **Status:** COMPLETE IN OPENMLS
   - **Our plan issue:** We proposed storing this separately; OpenMLS already does it

3. **Encryption key pair management**
   - ✓ StorageProvider has write_encryption_key_pair() method (line 198)
   - ✓ StorageProvider has delete_encryption_key_pair() method (line 539)
   - **Status:** AVAILABLE BUT OPTIONAL (encryption key also in bundle)

4. **Welcome message decryption**
   - ✓ OpenMLS looks up key package by hash_ref
   - ✓ Extracts private_init_key from bundle
   - ✓ Decrypts Welcome using that key
   - **Status:** COMPLETE IN OPENMLS

### What OUR Implementation Must Still Do

1. **Pool Management (Phase 2.2 core)**
   - Generate N key packages at once (multiple calls to KeyPackageBuilder::build)
   - Track pool state: available/reserved/spent/expired counts
   - Implement replenishment logic (when < 25%, generate more)
   - **Status:** NOT IN OPENMLS - This is application-level
   - **Required:** KeyPackagePool struct with tracking

2. **Pool Persistence Beyond StorageProvider**
   - StorageProvider stores individual KeyPackages
   - WE MUST add application-level metadata tracking:
     - Pool statistics (available count, reserved count, etc.)
     - Expiry timestamps for cleanup
     - Reservation state (reserved_by, expires_at)
     - Spend tracking (spent_by, group_id)
   - **Status:** NOT IN OPENMLS - Requires extra table
   - **Required:** Phase 2.1 LocalStore enhancements (lines 119-157 in our plan)

3. **Reservation Model (Phase 2.3)**
   - Server-side reservation with TTL (60-second timeout)
   - Prevent double-spend (check status before allowing spend)
   - **Status:** NOT IN OPENMLS - Server application concern
   - **Required:** KeyPackageStore in server

4. **Periodic Refresh (Phase 2.2)**
   - Background/periodic task to call replenishment
   - Cleanup expired keys
   - **Status:** NOT IN OPENMLS - Caller responsibility
   - **Required:** MlsConnection::refresh_key_packages() method

5. **CLI Integration (Phase 2.5)**
   - Call refresh() periodically from CLI loop
   - **Status:** NOT IN OPENMLS
   - **Required:** CLI.rs integration

---

## Architectural Implications For Phase 2.2

### 1. Storage Layer Strategy

**Current Plan (from changelog/20251028):**
- Add `keypackages` table to LocalStore for pool metadata
- Duplicate storage of KeyPackageBundle

**BETTER APPROACH (OpenMLS-aligned):**
- **Use StorageProvider for individual KeyPackages** 
  - Let OpenMLS handle write_key_package/delete_key_package
  - KeyPackages automatically persisted by OpenMLS
  - Cost: Zero additional code
  
- **Add ONLY pool metadata to LocalStore**
  - Table: `keypackage_pool_metadata` with:
    - `keypackage_ref` (BLOB) - reference to lookup in OpenMLS storage
    - `status` (TEXT) - "available" | "reserved" | "spent" | "expired"
    - `created_at` (INTEGER) - timestamp
    - `not_after` (INTEGER) - expiry time
    - `reserved_at` (INTEGER) - when reserved
    - `spent_at` (INTEGER) - when spent
  
  - **Do NOT store:** keypackage_bytes, private_init_key, private_encryption_key
    - OpenMLS already stores these via StorageProvider
    - We only track lifecycle state

**Benefit:** Avoids data duplication, leverages OpenMLS guarantees

### 2. KeyPackagePool Implementation Strategy

**Current Plan:** Pool tracks available/reserved/spent counts by querying LocalStore

**Better Approach:** Pool queries BOTH storage systems:
```rust
impl KeyPackagePool {
    pub fn get_available_count(&self, 
        openmls_storage: &StorageProvider,
        local_store: &LocalStore
    ) -> Result<usize> {
        // Query OpenMLS storage for existence
        let kp = openmls_storage.key_package(&ref)?;
        
        // Query LocalStore for status
        let metadata = local_store.get_pool_metadata(&ref)?;
        
        // Only count if exists in BOTH and status == "available"
        if kp.is_some() && metadata.status == "available" {
            count += 1;
        }
        Ok(count)
    }
}
```

### 3. Deletion Safety

**Critical:** When spending a key package:
1. Mark as spent in LocalStore metadata (status = "spent")
2. Call OpenMLS storage::delete_key_package()
3. Both must succeed (transactional concern)

**Risk:** If step 2 fails, we've lost the private keys but marked spent
- Mitigation: Wrap in transaction, use Result<>, handle errors

### 4. Last-Resort Keys

**OpenMLS Behavior:** Never deletes keys marked with LastResortExtension (line 578 in creation.rs)

**Our Plan:** Must account for this
- Don't force delete last-resort keys
- Mark as "expired" in metadata instead
- Only delete manually when user initiates cleanup

---

## Files Summary

### OpenMLS Core (Read-Only Reference)
| File | Purpose | Key Lines |
|------|---------|-----------|
| `/openmls/traits/src/storage.rs` | StorageProvider trait definition | 29-585 |
| `/openmls/openmls/src/key_packages/mod.rs` | KeyPackage generation & bundling | 260-587 |
| `/openmls/openmls/src/group/mls_group/creation.rs` | Welcome processing & key lookup | 550-581 |
| `/openmls/sqlite_storage/src/key_packages.rs` | SQLite implementation | 1-84 |

### Our Implementation (Phase 2.2 Focus)
| File | Purpose | Dependent On |
|------|---------|--------------|
| `client/rust/src/storage.rs` | LocalStore pool metadata table | None (new) |
| `client/rust/src/mls/keypackage_pool.rs` | Pool management logic | StorageProvider + LocalStore |
| `client/rust/src/mls/connection.rs` | refresh_key_packages() orchestration | KeyPackagePool + StorageProvider |
| `server/src/db/keypackage_store.rs` | Server reservation tracking | None (new) |

---

## Critical Insights For Implementation

### 1. OpenMLS Stores Everything Automatically
- When KeyPackageBuilder::build() completes, the bundle is ALREADY in storage
- We don't need to manually serialize/deserialize KeyPackageBundle
- The StorageProvider abstraction handles this

### 2. Welcome Decryption Requires Both Keys
- **private_init_key** - decrypts Welcome message envelope
- **private_encryption_key** - not used for Welcome, but needed for group operations
- Both are required for the bundle to be usable

### 3. Hash Reference is Deterministic
- Same KeyPackage input = same hash_ref always
- Hash computed from serialized KeyPackage bytes
- Can be re-computed if needed (though not typically done)

### 4. Single-Use Model is Enforced by OpenMLS
- After joining a group via Welcome, OpenMLS calls delete_key_package()
- Application cannot "reuse" a key package that was already used
- Last-resort keys bypass this deletion

### 5. StorageProvider is NOT Just Persistence
- It's the CONTRACT between OpenMLS and application
- OpenMLS assumes certain data will be present/absent
- If we override default behavior, we must maintain invariants

---

## Next Steps For Phase 2.2 Implementation

### Before Coding
1. Decide: Use OpenMLS StorageProvider for KeyPackages, or duplicate storage?
   - **Recommendation:** Use StorageProvider (avoid duplication)
   
2. Design transaction handling:
   - What if mark-spent succeeds but delete fails?
   - What if network failure during spend?
   
3. Plan expiry cleanup:
   - Iterate all refs in LocalStore metadata
   - Check expiry times
   - Delete expired from both LocalStore AND OpenMLS storage

### During Coding
1. Phase 2.1: Implement LocalStore keypackage_pool_metadata table (NOT full bundle storage)
2. Phase 2.2: Implement KeyPackagePool using OpenMLS StorageProvider + LocalStore metadata
3. Phase 2.3: Add refresh_key_packages() to MlsConnection calling KeyPackagePool methods

### Testing Considerations
- Generate KeyPackage via OpenMLS
- Verify it appears in StorageProvider (it will, automatically)
- Retrieve via StorageProvider.key_package()
- Verify private keys are intact
- Test Welcome decryption with retrieved key
- Test deletion removes from StorageProvider

---

## Summary

**StorageProvider does a lot for KeyPackages:**
- Automatic persistence of complete bundle (public + 3 private keys)
- Retrieval by hash reference
- Single-use enforcement via deletion
- Special handling for last-resort keys

**We must NOT duplicate this.** Instead:
1. **Leverage OpenMLS StorageProvider** for KeyPackageBundle storage
2. **Add only metadata tracking** in LocalStore (status, timestamps)
3. **Implement pool logic** in KeyPackagePool (counting, replenishment decisions)
4. **Handle coordination** between OpenMLS storage and our metadata

This avoids data duplication and aligns with OpenMLS architecture.

**Status:** Ready for Phase 2.1 implementation with updated approach

---

**Created:** 2025-11-05
**Last Updated:** 2025-11-05
**Phase:** Analysis Complete - Ready for Implementation Planning
