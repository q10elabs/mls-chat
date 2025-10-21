# OpenMLS SQLite Storage Research - 2025-10-21

## Task Specification
Research the `openmls_sqlite_storage` crate to understand:
- What SqliteStorageProvider does
- How it integrates with OpenMLS group operations
- What data it stores (MLS group state, key packages, etc)
- How to serialize/deserialize MLS groups with it
- Performance characteristics
- Limitations and caveats

## Research Sources
- `/home/kena/src/quintessence/mls-chat/openmls/sqlite_storage/` directory (source code)
- `openmls-book.html` (documentation)
- Test files and examples in openmls repository

## Key Findings

### Core Purpose
- Codec-independent storage provider implementing the `StorageProvider` trait
- Uses rusqlite crate for SQLite database operations
- Provides persistence for all OpenMLS group state and cryptographic material
- Version-aware storage (STORAGE_PROVIDER_VERSION = 1)
- Uses refinery for database migrations

### Database Schema
Eight main tables:
1. `openmls_encryption_keys` - HPKE encryption key pairs
2. `openmls_epoch_keys_pairs` - Per-epoch encryption keys
3. `openmls_group_data` - Core group state (11+ data types)
4. `openmls_key_packages` - Key packages for joining groups
5. `openmls_own_leaf_nodes` - Client's leaf nodes in groups
6. `openmls_proposals` - Queued proposals
7. `openmls_psks` - Pre-shared keys
8. `openmls_signature_keys` - Signature key pairs

### Codec Design
- Generic over codec type implementing Codec trait
- Codec trait requires: `to_vec<T: Serialize>` and `from_slice<T: DeserializeOwned>`
- Common implementation: JsonCodec using serde_json
- Allows for alternative serialization formats (bincode, postcard, etc)

### Integration Pattern
- Create provider with crypto + storage + rand
- Initialize database with `run_migrations()`
- MLS groups automatically persist to storage
- Load groups with `MlsGroup::load(provider.storage(), &group_id)`
- No manual save/load operations needed

### Data Stored
Group data types include:
- JoinGroupConfig, Tree, InterimTranscriptHash, Context
- ConfirmationTag, GroupState, MessageSecrets
- ResumptionPskStore, OwnLeafIndex, GroupEpochSecrets
- ApplicationExportTree (draft-08 extension)

### Forward Secrecy
- Storage provider must irrevocably delete data when delete_ methods called
- Critical for forward secrecy guarantees
- No copies should be retained after deletion

## Limitations & Caveats

### Platform Support
- **Does NOT support wasm32 target** (documented in lib.rs)
- Only works on platforms with SQLite support

### Migration Management
- `initialize()` method deprecated since 0.2.0
- Use `run_migrations()` instead (sets custom migration table name)
- Version tracking ensures safe upgrades

### Thread Safety
- Uses `Borrow<Connection>` and `BorrowMut<Connection>` traits
- Single connection model - application must manage concurrency
- SQLite connection itself has thread safety constraints

### Error Handling
- StorageProvider::Error = rusqlite::Error
- Applications must handle database errors appropriately

## Best Practices

1. **Initialization**: Always call `run_migrations()` before first use
2. **Codec Choice**: Use efficient binary codecs (bincode) for production, JSON for debugging
3. **Connection Management**: Use persistent file-based DB, not in-memory for production
4. **Error Handling**: Wrap storage operations in proper error handling
5. **Deletion**: Ensure delete operations truly remove data (forward secrecy)
6. **Versioning**: Don't modify stored data structures without migration

## Example Usage Pattern

```rust
// 1. Create codec
#[derive(Default)]
pub struct JsonCodec;
impl Codec for JsonCodec {
    type Error = serde_json::Error;
    fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, Self::Error> {
        serde_json::to_vec(value)
    }
    fn from_slice<T: DeserializeOwned>(slice: &[u8]) -> Result<T, Self::Error> {
        serde_json::from_slice(slice)
    }
}

// 2. Setup storage provider
let connection = Connection::open("mls.db").unwrap();
let mut storage = SqliteStorageProvider::<JsonCodec, Connection>::new(connection);
storage.run_migrations().unwrap();

// 3. Create OpenMLS provider combining crypto + storage
struct MyProvider {
    crypto: RustCrypto,
    storage: SqliteStorageProvider<JsonCodec, Connection>,
}

// 4. Groups automatically persist
let group = MlsGroup::new(provider, &signer, &config, credential).unwrap();
// State is automatically written to storage

// 5. Load groups later
let group = MlsGroup::load(provider.storage(), &group_id)
    .expect("Error loading")
    .expect("Group not found");
```

## Status
Research complete. Comprehensive understanding of SqliteStorageProvider achieved.
