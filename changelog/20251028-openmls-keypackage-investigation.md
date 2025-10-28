# OpenMLS KeyPackage Investigation

## Executive Summary

**Critical Discovery:** OpenMLS KeyPackageBundle contains **three essential components** that must all be stored persistently:

1. **KeyPackage** (public part) - for re-upload and validation
2. **private_init_key** (HpkePrivateKey) - for Welcome message decryption (MUST persist)
3. **private_encryption_key** (EncryptionPrivateKey) - for leaf node operations (MUST persist)

**Current Problem:** Client uses in-memory storage only. Private keys are lost on restart, preventing:
- Reception of delayed Welcome messages
- Joining groups when invited while offline
- Basic async MLS functionality

**Key Findings:**
- KeyPackageRef is computed as SHA256 hash of serialized KeyPackage (ciphersuite-dependent)
- Each KeyPackage gets unique HPKE key pair (never reuse HPKE keys)
- KeyPackages are single-use (deleted after Welcome consumption, unless last_resort)
- Credential/ciphersuite changes require new KeyPackages (cryptographically bound)

**Recommended Schema:**
- Minimal (Phase 1): 5 fields - ref, bytes, two private keys, created_at
- Full (Phase 2): Add status, credential_hash, ciphersuite, expiry, last_resort flag
- Storage: SQLite with encryption at rest (simpler than platform keystore)

**Implementation Phases:**
- Phase 1: Persistent storage of single KeyPackage (fixes broken async invites)
- Phase 2: Pool of 32 KeyPackages with replenishment (production-ready)
- Phase 3: Server-side reservation, equivocation detection (advanced)

## Task Specification
Investigate OpenMLS KeyPackage handling and storage requirements in detail:
1. Search openmls/ directory for KeyPackageBundle struct and related implementations
2. Read openmls-book.html for lifecycle and storage requirements
3. Analyze current crypto.rs usage patterns
4. Document findings with technical recommendations

## Investigation Progress

### Phase 1: OpenMLS Source Code Analysis
- [x] Find KeyPackageBundle struct definition
- [x] Find KeyPackageRef computation (hash algorithm)
- [x] Find HPKE key pair generation patterns
- [x] Find credential binding and ciphersuite storage
- [x] Find KeyPackage lifecycle examples/tests

### Phase 2: Documentation Review
- [x] Review openmls-book.html for lifecycle description
- [x] Document HPKE private key storage requirements
- [x] Document credential rotation implications
- [x] Document Welcome message consumption flow

### Phase 3: Current Implementation Analysis
- [x] Analyze client/rust/src/crypto.rs KeyPackageBundle usage
- [x] Document available fields and methods
- [x] Document credential/ciphersuite configuration
- [x] Document key storage patterns

### Phase 4: Synthesis
- [x] Document KeyPackageBundle fields and purposes
- [x] Define required client-side storage schema
- [x] Document update lifecycle
- [x] Document credential/ciphersuite rotation implications
- [x] Document HPKE private key handling requirements

## Findings

### KeyPackageBundle Structure

From /home/kena/src/quintessence/mls-chat/openmls/openmls/src/key_packages/mod.rs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPackageBundle {
    pub(crate) key_package: KeyPackage,
    pub(crate) private_init_key: HpkePrivateKey,
    pub(crate) private_encryption_key: EncryptionPrivateKey,
}
```

**Three critical components:**

1. **key_package: KeyPackage** - The public part containing:
   - protocol_version: ProtocolVersion
   - ciphersuite: Ciphersuite
   - init_key: InitKey (HpkePublicKey)
   - leaf_node: LeafNode (containing encryption public key, signature key, credential)
   - extensions: Extensions (including lifetime)
   - signature: Signature

2. **private_init_key: HpkePrivateKey** - Private HPKE key for Welcome message decryption
   - Used ONCE when processing Welcome message to decrypt group secrets
   - Must be retrievable by KeyPackageRef (hash of serialized KeyPackage)

3. **private_encryption_key: EncryptionPrivateKey** - Leaf node private key
   - Used for ongoing group message encryption/decryption
   - Part of the ratcheting tree state

### KeyPackageRef Computation

From /home/kena/src/quintessence/mls-chat/openmls/openmls/src/ciphersuite/hash_ref.rs:

```rust
pub fn make_key_package_ref(
    value: &[u8],
    ciphersuite: Ciphersuite,
    crypto: &impl OpenMlsCrypto,
) -> Result<KeyPackageRef, CryptoError> {
    HashReference::new(value, ciphersuite, crypto, KEY_PACKAGE_REF_LABEL)
}

// KEY_PACKAGE_REF_LABEL = b"MLS 1.0 KeyPackage Reference"
```

**Algorithm:**
1. Serialize KeyPackage using TLS encoding
2. Construct RefHashInput with label "MLS 1.0 KeyPackage Reference" and serialized KeyPackage
3. Hash using ciphersuite's hash algorithm (e.g., SHA256 for x25519 ciphersuite)
4. Result is the KeyPackageRef

**Hash size depends on ciphersuite:**
- SHA256: 32 bytes
- SHA384: 48 bytes
- SHA512: 64 bytes

### HPKE Key Generation

From KeyPackage::create() (lines 274-282):

```rust
// Create a new HPKE key pair
let ikm = Secret::random(ciphersuite, provider.rand())
    .map_err(LibraryError::unexpected_crypto_error)?;
let init_key = provider
    .crypto()
    .derive_hpke_keypair(ciphersuite.hpke_config(), ikm.as_slice())
```

**Key points:**
- Each KeyPackage gets a unique, randomly generated HPKE key pair
- Private key MUST be stored before KeyPackage is published
- Private key is needed to decrypt Welcome message when invited to group

### Credential and Ciphersuite Binding

From KeyPackageTbs struct (lines 150-157):

```rust
#[derive(Debug, Clone, PartialEq, TlsSize, TlsSerialize, Serialize, Deserialize)]
struct KeyPackageTbs {
    protocol_version: ProtocolVersion,
    ciphersuite: Ciphersuite,
    init_key: InitKey,
    leaf_node: LeafNode,
    extensions: Extensions,
}
```

**Credential is inside LeafNode:**
- LeafNode contains credential (BasicCredential with identity bytes)
- LeafNode contains signature key (public part)
- Entire KeyPackageTbs is signed with signature private key

**This means:**
- Changing credential requires new KeyPackage (signed with different key)
- Changing ciphersuite requires new KeyPackage
- KeyPackage is cryptographically bound to specific identity+ciphersuite

### Storage Requirements

From /home/kena/src/quintessence/mls-chat/openmls/traits/src/storage.rs:

```rust
fn write_key_package<
    HashReference: traits::HashReference<VERSION>,
    KeyPackage: traits::KeyPackage<VERSION>,
>(
    &self,
    hash_ref: &HashReference,
    key_package: &KeyPackage,
) -> Result<(), Self::Error>;
```

**OpenMLS stores KeyPackageBundle indexed by KeyPackageRef.**

From sqlite_storage implementation:
```sql
CREATE TABLE openmls_key_packages (
    key_package_ref BLOB PRIMARY KEY,
    key_package BLOB NOT NULL,
    provider_version INTEGER NOT NULL
)
```

**What gets stored:**
- Key: KeyPackageRef (hash)
- Value: Serialized KeyPackageBundle (including both private keys)

### Lifecycle Management

From /home/kena/src/quintessence/mls-chat/openmls/openmls/src/group/mls_group/creation.rs:572-575:

```rust
if !key_package_bundle.key_package().last_resort() {
    provider
        .storage()
        .delete_key_package(&key_package_bundle.key_package.hash_ref(provider.crypto())?)
```

**Key lifecycle stages:**

1. **Created** - KeyPackageBuilder.build() generates bundle and stores it
2. **Uploaded** - Serialized KeyPackage sent to server (private keys stay local)
3. **Reserved** - Server marks as reserved when fetched for Add proposal
4. **Consumed** - Welcome message processed, HPKE key used to decrypt
5. **Deleted** - After Welcome processing (unless marked last_resort)

**Last Resort Extension:**
- Special flag prevents deletion after consumption
- Allows KeyPackage to be reused if no other options
- Not recommended for production (security risk)

### Welcome Message Processing Flow

From KeyPackageBundle usage:

1. Adder fetches KeyPackage from server
2. Adder creates Add proposal with KeyPackageRef
3. Adder sends Welcome message encrypted to KeyPackage's init_key
4. Target client receives Welcome message
5. Target looks up KeyPackageBundle by KeyPackageRef in local storage
6. Target uses private_init_key to decrypt Welcome
7. Target extracts group secrets and encryption_key_pair
8. Target deletes KeyPackageBundle from storage (unless last_resort)

**Critical requirement:** private_init_key MUST be retrievable by KeyPackageRef

### Credential Rotation Implications

**Scenario:** User wants to change identity/credential

**Implications:**
1. Cannot update existing KeyPackages (signature would be invalid)
2. Must generate new KeyPackages with new credential
3. Old KeyPackages should be marked invalid/revoked on server
4. Any groups using old credential continue working (group state is separate)

**Best practice:**
- Upload new credential's KeyPackages before revoking old
- Grace period to allow pending invitations to complete
- Clear communication to users about identity change

### HPKE Private Key Handling

**Where stored:**
- Local device only (never sent to server)
- Stored in OpenMLS provider storage (KeyPackageBundle)
- Indexed by KeyPackageRef for retrieval

**Security requirements:**
- Must persist across sessions (to receive delayed invitations)
- Should use platform keystore if available (iOS Keychain, Android KeyStore)
- Encryption at rest recommended
- Deletion after use (except last_resort)

**Current client/rust implementation:**
- Uses OpenMLS in-memory storage (not persistent!)
- Private keys lost on restart
- Cannot receive Welcome messages after restart

### Recommendations

Based on OpenMLS design and best practices:

#### 1. Client Storage Schema

**Required table: keypackages**

```sql
CREATE TABLE keypackages (
    -- Primary key: hash of serialized KeyPackage
    keypackage_ref BLOB PRIMARY KEY,

    -- Serialized KeyPackage (public part) for validation
    keypackage_bytes BLOB NOT NULL,

    -- Private HPKE init key (for Welcome decryption)
    private_init_key BLOB NOT NULL,

    -- Private encryption key (for leaf node)
    private_encryption_key BLOB NOT NULL,

    -- Metadata for pool management
    created_at INTEGER NOT NULL,
    uploaded_at INTEGER,

    -- Lifecycle tracking
    status TEXT NOT NULL DEFAULT 'created', -- created|uploaded|reserved|spent

    -- Credential binding
    credential_hash BLOB NOT NULL,
    ciphersuite INTEGER NOT NULL,

    -- Expiry from lifetime extension
    not_before INTEGER NOT NULL,
    not_after INTEGER NOT NULL,

    -- Flag for last resort packages
    last_resort INTEGER NOT NULL DEFAULT 0,

    -- Index for pool queries
    INDEX idx_status ON keypackages(status),
    INDEX idx_expiry ON keypackages(not_after),
    INDEX idx_credential ON keypackages(credential_hash)
)
```

**Field rationales:**

- **keypackage_ref**: Must match OpenMLS KeyPackageRef computation (ciphersuite-specific hash)
- **keypackage_bytes**: Full KeyPackage for re-upload or validation
- **private_init_key**: Critical for Welcome decryption, must never be sent to server
- **private_encryption_key**: Needed for leaf node after joining group
- **created_at/uploaded_at**: Track upload status and age
- **status**: Enable pool management (query available keys)
- **credential_hash**: Support credential rotation (invalidate old keys)
- **ciphersuite**: Support multiple ciphersuites per user
- **not_before/not_after**: Enable expiry-based filtering and cleanup
- **last_resort**: Prevent deletion of backup keys

#### 2. Comparison: Current vs Should Be

**Currently stored (none - in-memory only):**
- KeyPackageBundle lost on restart
- Cannot receive delayed invitations
- Must regenerate KeyPackages every session

**Should be stored:**
- All KeyPackageBundle fields persistently
- Indexed by KeyPackageRef
- With lifecycle metadata
- With expiry tracking
- Associated with credential

#### 3. HPKE Private Key Handling

**Option A: Store in DB (current approach for MlsProvider)**
```rust
// Store encrypted at rest
private_init_key BLOB NOT NULL
```
- Pros: Simple, works across platforms
- Cons: Requires DB encryption, not using platform keystore

**Option B: Platform keystore + DB reference**
```rust
// Store keystore reference
private_init_key_ref TEXT NOT NULL,
```
- Pros: Better security (platform-managed)
- Cons: Complex, platform-specific, may not persist properly

**Recommendation: Option A with encryption at rest**
- OpenMLS already uses this pattern
- SQLCipher or similar for DB encryption
- Simpler to implement correctly
- Works consistently across platforms

#### 4. Pool Management Strategy

**Phase 1 (Current):**
- Single KeyPackage per credential
- Regenerate on each session
- Works but not optimal

**Phase 2 (Production):**
- Pool of 32 KeyPackages
- Generate in batch: `generate_key_packages(count: usize)`
- Upload in batch: `upload_key_packages(packages: Vec<KeyPackage>)`
- Background replenishment: Check pool size, upload when < 25%
- Expiry-based cleanup: Delete expired from DB
- Status tracking: created→uploaded→reserved→spent

**Phase 3 (Advanced):**
- Server-side reservation logic
- Equivocation detection
- Transparent audit logs

#### 5. Credential Lifecycle

**Rotation procedure:**
1. Generate new credential+signature key
2. Generate new KeyPackages with new credential
3. Upload new KeyPackages to server
4. Mark old KeyPackages as invalid (don't delete yet - grace period)
5. After grace period (e.g., 24h), delete old KeyPackages
6. Update local credential reference

**Invalidation triggers:**
- User explicitly rotates credential
- Credential expires (if using certificates)
- Security event (key compromise detected)

**Database support:**
```sql
-- Add credential tracking
ALTER TABLE keypackages ADD COLUMN credential_id TEXT;

-- Query keys by credential
SELECT * FROM keypackages WHERE credential_id = 'alice@example.com';

-- Invalidate old credential's keys
UPDATE keypackages
SET status = 'invalidated'
WHERE credential_id = 'old@example.com';
```

#### 6. Schema Recommendations Summary

**Minimal schema for Phase 1:**
```sql
CREATE TABLE keypackages (
    keypackage_ref BLOB PRIMARY KEY,
    keypackage_bytes BLOB NOT NULL,
    private_init_key BLOB NOT NULL,
    private_encryption_key BLOB NOT NULL,
    created_at INTEGER NOT NULL
)
```

**Full schema for Phase 2:**
```sql
CREATE TABLE keypackages (
    keypackage_ref BLOB PRIMARY KEY,
    keypackage_bytes BLOB NOT NULL,
    private_init_key BLOB NOT NULL,
    private_encryption_key BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    uploaded_at INTEGER,
    status TEXT NOT NULL DEFAULT 'created',
    credential_hash BLOB NOT NULL,
    ciphersuite INTEGER NOT NULL,
    not_before INTEGER NOT NULL,
    not_after INTEGER NOT NULL,
    last_resort INTEGER NOT NULL DEFAULT 0
)
```

### Code Examples from OpenMLS

**Creating KeyPackageBundle:**
```rust
// From openmls/openmls/src/key_packages/mod.rs:517-553
pub fn build(
    mut self,
    ciphersuite: Ciphersuite,
    provider: &impl OpenMlsProvider,
    signer: &impl Signer,
    credential_with_key: CredentialWithKey,
) -> Result<KeyPackageBundle, KeyPackageNewError> {
    self.ensure_last_resort();
    let KeyPackageCreationResult {
        key_package,
        encryption_keypair,
        init_private_key,
    } = KeyPackage::create(
        ciphersuite,
        provider,
        signer,
        credential_with_key,
        self.key_package_lifetime.unwrap_or_default(),
        self.key_package_extensions.unwrap_or_default(),
        self.leaf_node_capabilities.unwrap_or_default(),
        self.leaf_node_extensions.unwrap_or_default(),
    )?;

    // Store the key package in the key store with the hash reference as id
    let full_kp = KeyPackageBundle {
        key_package,
        private_init_key: init_private_key,
        private_encryption_key: encryption_keypair.private_key().clone(),
    };
    provider
        .storage()
        .write_key_package(&full_kp.key_package.hash_ref(provider.crypto())?, &full_kp)
        .map_err(|_| KeyPackageNewError::StorageError)?;

    Ok(full_kp)
}
```

**Computing KeyPackageRef:**
```rust
// From openmls/openmls/src/key_packages/mod.rs:374-383
pub fn hash_ref(&self, crypto: &impl OpenMlsCrypto) -> Result<KeyPackageRef, LibraryError> {
    make_key_package_ref(
        &self
            .tls_serialize_detached()
            .map_err(LibraryError::missing_bound_check)?,
        self.payload.ciphersuite,
        crypto,
    )
    .map_err(LibraryError::unexpected_crypto_error)
}
```

**Consuming KeyPackage on Welcome:**
```rust
// From openmls/openmls/src/group/mls_group/creation.rs:569-577
if !key_package_bundle.key_package().last_resort() {
    provider
        .storage()
        .delete_key_package(&key_package_bundle.key_package.hash_ref(provider.crypto())?)
        .map_err(|_| WelcomeError::StorageError)?;
}
```

## Decisions and Next Steps

### Critical Findings

1. **Storage is Mandatory:** OpenMLS expects KeyPackageBundle to persist. Current in-memory storage breaks async invitations.

2. **Three Fields Required:** Cannot store just KeyPackage bytes. Must store both private keys separately.

3. **KeyPackageRef is Authoritative:** Hash-based lookup is the only way OpenMLS finds private keys.

4. **Single-Use by Default:** KeyPackages are deleted after Welcome processing (security best practice).

5. **Pool Strategy is Production Requirement:** Single KeyPackage is acceptable for MVP but pool of 32 is needed for reliability.

### Immediate Recommendations

**For Phase 1 (Current Work):**
1. Implement minimal keypackages table in client/rust
2. Store KeyPackageBundle fields persistently
3. Update MlsProvider to use SQLite storage instead of in-memory
4. Test Welcome message reception after restart

**Schema to Implement Now:**
```sql
CREATE TABLE keypackages (
    keypackage_ref BLOB PRIMARY KEY,
    keypackage_bytes BLOB NOT NULL,
    private_init_key BLOB NOT NULL,
    private_encryption_key BLOB NOT NULL,
    created_at INTEGER NOT NULL
)
```

**For Phase 2 (Next Sprint):**
1. Expand schema with status, expiry, credential tracking
2. Implement batch generation (32 KeyPackages)
3. Add background replenishment task
4. Server-side reservation endpoint

**Testing Requirements:**
1. Generate KeyPackage, restart client, receive Welcome - must work
2. Multiple concurrent invitations - should not exhaust pool
3. Expired KeyPackage cleanup - verify deletion
4. Credential rotation - verify old keys invalidated

### Files to Modify

**Client Rust:**
- `/home/kena/src/quintessence/mls-chat/client/rust/src/provider.rs` - Add keypackages table
- `/home/kena/src/quintessence/mls-chat/client/rust/src/crypto.rs` - Update KeyPackageBundle handling
- `/home/kena/src/quintessence/mls-chat/client/rust/src/mls/connection.rs` - Update initialize() to persist

**Server:**
- Add KeyPackage pool management endpoint
- Add reservation/spend tracking

### Alignment with Previous Analysis

This investigation confirms the pool strategy documented in:
- `/home/kena/src/quintessence/mls-chat/docs/keypackage-pool-strategy.md`
- `/home/kena/src/quintessence/mls-chat/changelog/20251028-key-package-management-analysis.md`

The technical details now provide implementation guidance for both Phase 1 (single key) and Phase 2 (pool).

## Conclusion

OpenMLS KeyPackageBundle storage is **not optional** - it is required for basic MLS async functionality. The current in-memory approach must be replaced with persistent SQLite storage as the first priority. Pool management can follow as Phase 2, but persistent storage of private keys is a hard requirement for Phase 1.
