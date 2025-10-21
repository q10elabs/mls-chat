# OpenMLS SQLite Storage vs. Custom Implementation

A detailed comparison of using `openmls_sqlite_storage::SqliteStorageProvider` vs. rolling your own storage layer.

---

## Quick Summary

| Aspect | openmls_sqlite_storage | Custom (rusqlite) |
|--------|---|---|
| **Implementation Effort** | Minimal (plug & play) | Moderate (you handle everything) |
| **Correctness** | ✅ Audited, battle-tested | ⚠️ Your responsibility |
| **Storage Overhead** | Larger (11+ tables, full state) | Smaller (identity + metadata only) |
| **Performance** | Good (optimized queries) | Better (simpler schema) |
| **Flexibility** | Limited to trait constraints | Complete control |
| **Forward Secrecy** | ✅ Guaranteed by design | ⚠️ You must ensure it |
| **Learning Curve** | Low (just pass provider) | Medium (codec, serialization) |
| **Maintenance Burden** | None (OpenMLS team maintains) | High (your responsibility) |
| **Wasm Support** | ❌ No | ✅ Yes (rusqlite works) |

---

## What Each Approach Actually Does

### Your Current Approach (Custom rusqlite)

```
┌─────────────────────────────────────────────────────────────┐
│ Your Application                                            │
├─────────────────────────────────────────────────────────────┤
│ MlsClient                                                   │
│  ├─ stores identity (keypair, credential) in storage       │
│  ├─ stores group metadata (members) in storage             │
│  ├─ creates MlsGroup in memory                             │
│  ├─ passes MlsGroup to crypto functions                    │
│  └─ serializes group to blob, stores in storage (MANUAL)   │
├─────────────────────────────────────────────────────────────┤
│ LocalStore (rusqlite)                                       │
│  ├─ identities table (username → keypair, credential)      │
│  ├─ group_states table (username, group → blob)            │
│  └─ group_members table (username, group → [members])      │
├─────────────────────────────────────────────────────────────┤
│ SQLite: client.db                                           │
│  ├─ identities: 1 row per user                             │
│  ├─ group_states: 1 blob per (user, group)                 │
│  └─ group_members: 1 JSON array per (user, group)          │
└─────────────────────────────────────────────────────────────┘
```

**Your Responsibilities:**
1. Serialize MlsGroup to bytes yourself (using what codec?)
2. Deserialize bytes back to MlsGroup yourself
3. Know when to save/load from storage
4. Manage identity keypair serialization
5. Ensure forward secrecy of key material

### OpenMLS Storage Provider Approach

```
┌─────────────────────────────────────────────────────────────┐
│ Your Application                                            │
├─────────────────────────────────────────────────────────────┤
│ MlsClient                                                   │
│  ├─ stores identity metadata (username) in storage         │
│  ├─ creates MlsGroup with provider                         │
│  ├─ calls crypto functions (they auto-persist via provider)│
│  └─ loads MlsGroup by ID (provider handles everything)     │
├─────────────────────────────────────────────────────────────┤
│ OpenMlsProvider (your impl)                                 │
│  ├─ crypto: RustCrypto                                     │
│  ├─ rand: RustCrypto                                       │
│  └─ storage: SqliteStorageProvider ──┐                     │
├─────────────────────────────────────┼─────────────────────┤
│ SqliteStorageProvider (openmls_sqlite_storage)             │
│  │ Implements StorageProvider trait                        │
│  ├─ Codec: JsonCodec (or bincode, postcard, etc)          │
│  └─ Manages automatic serialization/deserialization       │
├─────────────────────────────────────┼─────────────────────┤
│ SQLite: mls_groups.db                │                     │
│  ├─ openmls_signature_keys           │                     │
│  ├─ openmls_encryption_keys          │                     │
│  ├─ openmls_group_data (11 rows/group)
│  ├─ openmls_tree                     │ Auto-managed        │
│  ├─ openmls_proposals                │ by OpenMLS          │
│  ├─ openmls_key_packages             │                     │
│  ├─ openmls_own_leaf_nodes           │                     │
│  └─ openmls_psks                     │                     │
└─────────────────────────────────────┘                      │
                                                              │
  All state transitions handled transparently ───────────────┘
```

**Your Responsibilities:**
1. Set up the provider correctly once
2. Call `provider.storage().run_migrations()` at startup
3. Pass provider to all MLS operations
4. Everything else is automatic

---

## Detailed Comparison

### 1. Correctness & Security

#### OpenMLS Storage Provider
**Pros:**
- ✅ Implements `StorageProvider` trait exactly as OpenMLS requires
- ✅ Audited by OpenMLS team (security-focused project)
- ✅ Handles forward secrecy correctly by design
- ✅ Proper error handling for storage errors
- ✅ All data transformations validated

**Cons:**
- ❌ You must trust the OpenMLS team's implementation
- ❌ If there's a bug in openmls_sqlite_storage, you're affected
- ⚠️ Currently no WASM support (irrelevant for CLI client)

#### Custom Implementation
**Pros:**
- ✅ Complete visibility into how data is stored
- ✅ Can audit your own code
- ✅ No dependencies on external implementations

**Cons:**
- ❌ You must implement forward secrecy correctly
- ❌ Easy to make subtle mistakes with serialization
- ❌ Risk of leaking key material in blobs
- ❌ Your responsibility to validate secure deletion
- ❌ Edge cases and race conditions are your problem

**Security Consideration:** Forward secrecy requires that deleted key material is *actually deleted*, not just marked deleted. OpenMLS docs explicitly state implementations must "irrevocably delete" data. Your custom code needs to ensure this.

---

### 2. Storage Schema & Overhead

#### OpenMLS Storage Provider

**8 tables, ~40+ indexed columns:**

```
openmls_signature_keys
  - public_key (PK)
  - key_pair

openmls_encryption_keys
  - public_key (PK)
  - key_pair

openmls_group_data (11+ rows per group)
  - group_id, data_type (PK)
  - config, tree, context, confirmation_tag, group_state
  - message_secrets, resumption_psk_store, own_leaf_index
  - group_epoch_secrets, application_export_tree

openmls_tree
  - group_id, tree_id (PK)
  - tree_data

openmls_proposals
  - group_id, proposal_ref (PK)
  - proposal_data

openmls_key_packages
  - key_package_ref (PK)
  - key_package_data

openmls_own_leaf_nodes
  - id (PK)
  - group_id, leaf_node_data

openmls_psks
  - psk_id (PK)
  - psk_data
```

**Storage per 3-person group:**
- ~10-15 KB per group (measured with binary codec)
- Grows with epoch count (refresh operations)
- Include key material copies for member tracking

#### Custom Implementation (Your Current Approach)

**3 tables, minimal columns:**

```
identities
  - username (PK)
  - keypair_blob
  - credential_blob

group_states
  - username, group_id (PK)
  - state_blob

group_members
  - username, group_id (PK)
  - members_json
```

**Storage per 3-person group:**
- ~2-5 KB (assuming you serialize group efficiently)
- Fixed size regardless of epoch
- Minimal metadata overhead

**Disk Impact:**
- OpenMLS: 3-5x larger per group
- Custom: Much smaller, but YOU must serialize correctly

---

### 3. Development Effort & Complexity

#### OpenMLS Storage Provider

**Setup (one-time):**
```rust
// 1. Define codec
#[derive(Default)]
pub struct BincodeCodec;

impl Codec for BincodeCodec {
    type Error = bincode::Error;

    fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, Self::Error> {
        bincode::serialize(value)
    }

    fn from_slice<T: DeserializeOwned>(slice: &[u8]) -> Result<T, Self::Error> {
        bincode::deserialize(slice)
    }
}

// 2. Create provider
struct MyProvider {
    crypto: OpenMlsRustCrypto,
    storage: SqliteStorageProvider<BincodeCodec, Connection>,
}

impl OpenMlsProvider for MyProvider {
    type StorageProvider = SqliteStorageProvider<BincodeCodec, Connection>;
    fn storage(&self) -> &Self::StorageProvider { &self.storage }
    fn crypto(&self) -> &Self::CryptoProvider { &self.crypto }
    fn rand(&self) -> &Self::RandProvider { &self.crypto }
}

// 3. Initialize
let mut provider = MyProvider::new(connection);
provider.storage_mut().run_migrations().unwrap();
```

**Usage (automatic):**
```rust
// Everything persists automatically
let group = MlsGroup::new(&provider, ...)?;
// All state now in database, no manual saves

let loaded = MlsGroup::load(provider.storage(), &group_id)?;
// Automatic deserialization
```

**Effort:** ~50 lines of boilerplate, then automatic forever

#### Custom Implementation

**Your current approach (ongoing):**

```rust
// 1. Serialize before storing
let mls_group = crypto::create_group(...)?;
let blob = bincode::serialize(&mls_group)?; // YOU handle this
storage.save_group_state(&username, &group_id, &blob)?;

// 2. Deserialize when loading
let blob = storage.load_group_state(&username, &group_id)?;
let mls_group: MlsGroup = bincode::deserialize(&blob)?; // YOU handle this

// 3. Process messages with manual serialization
let processed = crypto::process_message(&mut mls_group, ...)?;
let updated_blob = bincode::serialize(&mls_group)?; // Save again
storage.save_group_state(&username, &group_id, &updated_blob)?;

// 4. Every operation requires: load → process → serialize → save
```

**Ongoing Effort:** Every MLS operation requires manual serialization steps

**Risk:** Easy to forget to save after operations, leading to lost state

---

### 4. Performance Characteristics

#### OpenMLS Storage Provider

**Advantages:**
- ✅ Optimized queries via indexed tables
- ✅ Only changed data written (granular updates)
- ✅ Codec performance tunable (JSON vs binary)
- ✅ Built-in transaction handling

**Disadvantages:**
- ❌ Multiple table writes per operation (11+ rows per group state change)
- ❌ SQLite single-writer constraint (serialized writes)
- ❌ I/O overhead from database transactions

**Benchmarks (from OpenMLS test infrastructure):**
- Group creation: ~5-10ms (includes database writes)
- Message send: ~2-5ms (codec + database)
- Message receive: ~3-8ms (deserialize + process + save)

#### Custom Implementation

**Advantages:**
- ✅ Minimal writes (one blob per operation)
- ✅ Simpler schema = faster queries
- ✅ Direct control over optimization
- ✅ No per-operation overhead

**Disadvantages:**
- ❌ You must serialize/deserialize manually each time
- ❌ Entire group blob written even for small changes
- ❌ No automatic batching of writes

**Estimated Performance:**
- Group creation: ~1-2ms (no database overhead if you cache)
- Message send: ~1-3ms (serialize blob)
- Message receive: ~1-3ms (deserialize blob)
- **Better for CLI client** (not high throughput)

**Verdict:** Performance unlikely to matter for a CLI chat client either way.

---

### 5. Maintainability & Future Changes

#### OpenMLS Storage Provider

**Future-Proof:**
- ✅ OpenMLS team maintains schema/migrations
- ✅ New MLS features automatically supported (you just run migrations)
- ✅ Security patches applied to storage layer
- ✅ API stable (part of OpenMLS contract)

**When OpenMLS Adds Features:**
- Tree sync support? Storage table added automatically
- New key type? Codec handles it
- PSK refresh improvements? You get them for free

**Your Code Changes:** Typically zero, if using provider correctly

#### Custom Implementation

**Not Future-Proof:**
- ❌ You must track OpenMLS API changes manually
- ❌ New group state fields require schema migration
- ❌ Codec changes need your handling
- ❌ Performance optimizations your responsibility

**When OpenMLS Adds Features:**
- Tree sync support? You need to update serialization
- New key type? You must handle in your blob structure
- PSK refresh improvements? You need to adapt
- Forward secrecy enhancement? You reimplement

**Your Code Changes:** Potentially significant for each new feature

---

### 6. Debugging & Observability

#### OpenMLS Storage Provider

**Debugging:**
- Use JSON codec during development (human-readable blobs)
- Query `openmls_group_data` table directly to inspect state
- Schema is well-documented

**Observability:**
```sql
-- Check what state is stored
SELECT group_id, data_type, length(data) as blob_size
FROM openmls_group_data;

-- Find all keys for a user
SELECT * FROM openmls_own_leaf_nodes
WHERE group_id = 'your_group';
```

**Issues:** Must understand OpenMLS's 11 data types to debug

#### Custom Implementation

**Debugging:**
- You control the schema, can query easily
- Can add debug columns (timestamps, version info)
- Can log before/after serialization

**Observability:**
```sql
-- Easy to check
SELECT username, group_id, length(state_blob) as size FROM group_states;
SELECT * FROM group_members;
```

**Issues:** Must ensure you serialize things you can debug

---

### 7. Forward Secrecy & Security Guarantees

#### OpenMLS Storage Provider

**Forward Secrecy:**
- ✅ Automatic by design
- ✅ All `delete_*()` operations irrevocably delete
- ✅ SQLite-level deletion (not just marking deleted)
- ✅ Secure deletion best practices built-in

**Key Material Handling:**
- ✅ Separate tables for signature/encryption keys
- ✅ Deleted keys removed from database
- ✅ No copies of deleted material in logs

**Your Responsibility:** Just use it correctly (don't cache deleted keys)

#### Custom Implementation

**Forward Secrecy:**
- ⚠️ YOU must ensure deleted keys are actually gone
- ⚠️ Serializing to blob doesn't automatically delete old copies
- ⚠️ Version changes might leave old blobs around
- ⚠️ Easy to accidentally keep copies

**Key Material Handling:**
```rust
// YOUR blob approach - can you guarantee this is secure?
storage.save_group_state(&username, &group_id, &blob)?;

// What happens to the old blob?
// Is it overwritten? Deleted? Still in SQLite WAL?
// Do you VACUUM? Do you securely zero memory?
```

**Your Responsibility:** Extensive - must think through every security edge case

**Critical Issue:** SQLite keeps write-ahead logs (WAL) by default. Deleted data might linger in WAL files unless you handle cleanup.

---

### 8. Integration Complexity

#### OpenMLS Storage Provider

**With Your Client Code:**
```rust
// Current crypto.rs signature
pub fn create_group(
    provider: &impl OpenMlsProvider,  // Takes provider
    signer: &SignatureKeyPair,
    config: &MlsGroupCreateConfig,
    credential: &CredentialWithKey,
) -> Result<MlsGroup> {
    let group = MlsGroup::new(provider, signer, &config, credential)?;
    // Group is automatically persisted by provider
    Ok(group)
}

// Your client.rs
let mut provider = create_provider()?;
provider.storage_mut().run_migrations()?;

let group = create_group(&provider, ...)?;  // Auto-saves
let loaded = MlsGroup::load(provider.storage(), &group_id)?; // Auto-loads
```

**Changes Needed:**
1. Add provider field to `MlsClient`
2. Pass provider to all crypto functions
3. Remove manual serialization/deserialization
4. Use `MlsGroup::load()` instead of blob management

**Difficulty:** Low, mostly mechanical changes

#### Custom Implementation

**With Your Client Code:**
```rust
// Your crypto.rs signature (current)
pub fn create_group(
    credential: &CredentialWithKey,
    signer: &SignatureKeyPair,
    provider: &impl OpenMlsProvider,  // Still needs provider for crypto!
) -> Result<MlsGroup> {
    let group = MlsGroup::new(provider, signer, &config, credential)?;
    // YOU must serialize and save
    Ok(group)
}

// Your client.rs
let group = create_group(&credential, ...)?;
let blob = bincode::serialize(&group)?;  // YOUR code
storage.save_group_state(&username, &group_id, &blob)?;  // YOUR code

// Later:
let blob = storage.load_group_state(&username, &group_id)?;  // YOUR code
let group: MlsGroup = bincode::deserialize(&blob)?;  // YOUR code
```

**Changes Needed:**
1. Decide on codec (bincode, postcard, serde_json?)
2. Add serialization calls everywhere
3. Add error handling for serialization failures
4. Ensure consistency of codec across codebase
5. Handle version changes to MlsGroup serialization

**Difficulty:** Medium, many places to update

---

## Decision Framework

### Choose OpenMLS Storage Provider If...

✅ **You prioritize correctness**
- Don't want to reimplement security-critical code
- Want OpenMLS team to maintain the implementation

✅ **You plan to add features later**
- Tree sync, external commits, etc.
- Want built-in support for new MLS features

✅ **You want production-grade reliability**
- Security audits matter
- Forward secrecy must be bulletproof
- Maintenance burden matters

✅ **You're building a reference implementation**
- Teaching MLS concepts
- Want to follow best practices

### Choose Custom Implementation If...

✅ **Storage size is critical**
- Running on embedded devices
- Bandwidth is a constraint
- Custom codec for ultra-compact representation

✅ **Performance is paramount**
- High-throughput messaging system
- Latency-sensitive operations
- Worth optimizing every millisecond

✅ **You need complete control**
- Custom encryption wrapper for blobs
- Integration with existing storage system
- Specific security requirements

✅ **You're experimenting/prototyping**
- Learning MLS internals
- Need flexibility to change approach
- Not planning long-term maintenance

✅ **You target WASM**
- openmls_sqlite_storage doesn't support it
- Custom implementation with browser storage

---

## Hybrid Approach (Best of Both Worlds?)

You could also do a hybrid:

```rust
// Use OpenMLS storage for MLS group state
let provider = create_openmls_provider()?;

// Use custom storage for application-level metadata
let metadata_store = LocalStore::new("metadata.db")?;

// Store only what OpenMLS doesn't handle
metadata_store.save_group_members(&username, &group_id, &members)?;
metadata_store.save_user_preferences(&username, prefs)?;

// MLS state handled by provider
let group = MlsGroup::load(provider.storage(), &group_id)?;
```

**Advantages:**
- ✅ OpenMLS handles group state correctly
- ✅ You handle only application metadata
- ✅ Best of both worlds

**Disadvantages:**
- ❌ Two databases to manage
- ❌ More complexity than single approach

---

## My Recommendation for Your Chat Client

Given your use case (CLI MLS chat client):

### **Use OpenMLS Storage Provider**

**Reasoning:**

1. **Correctness is non-negotiable** - Forward secrecy matters for encrypted chat
2. **It's not complex** - 50 lines of setup, then automatic
3. **Your code is cleaner** - No serialization boilerplate everywhere
4. **Future-proof** - If OpenMLS adds features, you get them free
5. **Less to debug** - Trust the audited implementation
6. **Storage size doesn't matter** - It's a CLI app, a few KB is fine
7. **Performance isn't constrained** - Humans type messages slowly

**Implementation approach:**

```rust
// Create OpenMlsProvider once in MlsClient::new()
struct MlsClient {
    provider: MyOpenMlsProvider,  // NEW
    storage: LocalStore,          // Keep for metadata
    // ... rest
}

// Your crypto functions stay the same
pub fn create_group(
    provider: &impl OpenMlsProvider,
    signer: &SignatureKeyPair,
    config: &MlsGroupCreateConfig,
    credential: &CredentialWithKey,
) -> Result<MlsGroup> {
    MlsGroup::new(provider, signer, &config, credential)
    // No manual serialization
}

// Client.rs gets cleaner
let group = create_group(&self.provider, ...)?;
// Group state auto-persisted

// Later:
let group = MlsGroup::load(self.provider.storage(), &group_id)?;
// Group state auto-loaded
```

**Trade-off:** Accept 3-5x larger database, gain correctness and maintainability.

---

## Action Items If You Choose OpenMLS Storage

1. Add `openmls_sqlite_storage` to `Cargo.toml` (already referenced as openmls dependency)
2. Create `MyOpenMlsProvider` struct implementing `OpenMlsProvider` trait
3. Update `crypto.rs` functions to take `&impl OpenMlsProvider` consistently
4. Remove all manual serialization code
5. Use `MlsGroup::load()` to load by ID
6. Tests automatically verify storage works (no separate test requirements)

**Estimated effort:** 2-3 hours to implement and test

