# IdentityManager Quick Reference

## What It Does

The `IdentityManager` handles persistent storage and recovery of user identities (credentials and signature keys) across sessions using OpenMLS's built-in storage provider.

## Usage

### Basic Usage in Client Code

```rust
use mls_chat_client::{
    identity::IdentityManager,
    provider::MlsProvider,
    storage::LocalStore,
};

// Create/get provider and metadata store
let provider = MlsProvider::new("~/.mlschat/mls.db")?;
let metadata_store = LocalStore::new("~/.mlschat/metadata.db")?;

// Load or create identity
let stored_identity = IdentityManager::load_or_create(
    &provider,
    &metadata_store,
    "alice",  // username
)?;

// Use the identity
let signature_key = stored_identity.signature_key;
let credential = stored_identity.credential_with_key;

// Now use signature_key to sign messages
// The key is PERSISTENT - same key on next session!
```

### Verifying Identity Storage

```rust
// Check that identity is properly stored in both layers
let is_stored = IdentityManager::verify_stored(
    &provider,
    &metadata_store,
    &stored_identity,
)?;

if is_stored {
    println!("Identity persisted successfully!");
}
```

## Storage Architecture

### Metadata Store (LocalStore)
- **Database:** `~/.mlschat/metadata.db`
- **Stores:** Application metadata
- **Key Data:**
  - `username` → `public_key_blob` mapping
  - Group member lists
  - User preferences

```sql
CREATE TABLE identities (
    username TEXT PRIMARY KEY,
    keypair_blob BLOB,           -- Currently unused (regenerated from username)
    credential_blob BLOB,        -- Currently unused (regenerated from username)
    public_key_blob BLOB,        -- CRITICAL: Used for key lookup
    created_at TEXT
);
```

### MLS Provider Storage (OpenMLS)
- **Database:** `~/.mlschat/mls.db`
- **Stores:** OpenMLS group state and signature keys
- **Key Data:**
  - Signature keys indexed by public key hash
  - Group states
  - Key packages

**OpenMLS stores keys by public key**, so we need the LocalStore to map username → public_key.

## Flow Diagram

### First Time (New User)

```
load_or_create(provider, metadata_store, "alice")
    ↓
LocalStore.load_public_key("alice") → None
    ↓
Generate new SignatureKeyPair
    ↓
signature_key.store(provider.storage()) → Save to OpenMLS storage
    ↓
metadata_store.save_identity("alice", ..., public_key) → Save to metadata
    ↓
Return StoredIdentity with new key
```

### Subsequent Sessions (Returning User)

```
load_or_create(provider, metadata_store, "alice")
    ↓
LocalStore.load_public_key("alice") → "public_key_blob"
    ↓
SignatureKeyPair::read(provider.storage(), public_key) → Load from OpenMLS storage
    ↓
Return StoredIdentity with SAME key
```

## Key Design Decisions

### Why Two Storage Systems?

1. **OpenMLS Storage** (`SqliteStorageProvider`)
   - ✅ Handles complex group state
   - ✅ Built-in signature key persistence
   - ❌ Keys indexed by public_key_hash, not username

2. **Application Metadata** (`LocalStore`)
   - ✅ Provides username → public_key mapping
   - ✅ Stores application-level data (members, preferences)
   - ✅ Enables efficient lookups

**Together:** They provide a complete persistent identity system where any user can restart their client and resume with their original cryptographic identity.

### Why SignatureKeyPair Cannot Be Cloned?

`SignatureKeyPair` contains private key material and doesn't implement `Clone` for security:
- Private keys must not be accidentally duplicated
- `StoredIdentity` is non-cloneable
- Must be used immediately in operations

This is intentional - use it to sign messages, don't store it!

## Testing

### Run Identity Tests
```bash
cargo test identity:: --lib
```

### Expected Output
```
running 7 tests
test identity::tests::test_create_new_identity ... ok
test identity::tests::test_identity_persistence_across_instances ... ok
test identity::tests::test_different_users_different_identities ... ok
test identity::tests::test_identity_storage_verification ... ok
test identity::tests::test_signature_key_preserved_across_sessions ... ok
test identity::tests::test_credential_with_key_structure ... ok
test identity::tests::test_multiple_identities_in_same_db ... ok

test result: ok. 7 passed
```

## Troubleshooting

### "Public key found in metadata but not in OpenMLS storage"
- **Cause:** Metadata and MLS databases are out of sync
- **Solution:** Will automatically regenerate identity
- **Prevention:** Keep both databases in same directory

### Different key on reload
- **Cause:** Not using `IdentityManager` consistently
- **Solution:** Always use `IdentityManager::load_or_create()`
- **Check:** Use `verify_stored()` to validate persistence

### "Identity not found"
- **Cause:** Database corrupted or different path
- **Solution:** Check database paths match
- **Debug:** Look at `~/.mlschat/metadata.db` and `~/.mlschat/mls.db`

## Integration Checklist

When integrating IdentityManager into your code:

- [ ] Import: `use mls_chat_client::identity::IdentityManager`
- [ ] Create MlsProvider with file path
- [ ] Create LocalStore with file path (different database)
- [ ] Call `IdentityManager::load_or_create()` in initialization
- [ ] Store returned `signature_key` in client state
- [ ] Use for all message signing operations
- [ ] Run `cargo test identity::` to verify integration

## Security Notes

### ✅ What This Achieves
- **Identity Continuity:** Same user maintains same signing identity across sessions
- **Message Verification:** Receivers can verify messages came from same user
- **Forward Secrecy:** Each message still encrypted with fresh per-epoch keys (MLS handles)
- **Isolation:** Different usernames have completely different key material

### ⚠️  What This Doesn't Cover (Future Work)
- Key rotation for user-initiated identity refresh
- Backup/restore of identities
- Key recovery if database lost
- Multi-device support

## Example: Complete Usage

```rust
use mls_chat_client::{
    client::MlsClient,
    identity::IdentityManager,
};
use std::path::PathBuf;

async fn setup_client(username: &str) -> Result<MlsClient> {
    // Create client with persistent storage
    let client = MlsClient::new(
        "http://localhost:4000",
        username,
        "mygroup",
    ).await?;

    Ok(client)
}

async fn main() -> Result<()> {
    // Session 1: Create alice's identity
    let mut client = setup_client("alice").await?;
    client.initialize().await?;
    println!("Alice initialized with identity");

    // At this point:
    // - alice's signature key is stored in OpenMLS provider storage
    // - alice's public key is stored in metadata store

    drop(client); // Simulate client closing

    // Session 2: Reopen client
    let mut client = setup_client("alice").await?;
    client.initialize().await?;
    println!("Alice restored with SAME identity");

    // Behind the scenes:
    // - IdentityManager loaded alice's public key from metadata
    // - OpenMLS returned alice's ORIGINAL signature key
    // - All messages signed with this key match previous session

    Ok(())
}
```

## API Reference

### `IdentityManager::load_or_create()`

```rust
pub fn load_or_create(
    provider: &MlsProvider,
    metadata_store: &LocalStore,
    username: &str,
) -> Result<StoredIdentity>
```

**Behavior:**
- Returns existing identity if found, OR
- Creates new identity and stores in both storage layers

**Returns:** `StoredIdentity` with credential and signature key ready to use

### `IdentityManager::verify_stored()`

```rust
pub fn verify_stored(
    provider: &MlsProvider,
    metadata_store: &LocalStore,
    identity: &StoredIdentity,
) -> Result<bool>
```

**Behavior:**
- Checks metadata store has public key
- Checks OpenMLS storage has signature key
- Verifies they match

**Returns:** `true` if identity properly persisted in both layers

### `StoredIdentity`

```rust
pub struct StoredIdentity {
    pub username: String,
    pub credential_with_key: CredentialWithKey,
    pub signature_key: SignatureKeyPair,
}
```

**Note:** `SignatureKeyPair` is not cloneable. Use it immediately for signing operations.

---

**For detailed implementation information, see:** `SIGNATURE_KEY_PERSISTENCE_IMPLEMENTATION.md`
