# KeyPackage Pool Strategy for MLS

## Overview

A KeyPackage in MLS is a **single-use** signed bundle containing a device's HPKE init public key, credential (binding to identity), ciphersuite/capabilities, and extensions. Unlike a static registration key, KeyPackages enable **asynchronous group membership** — someone can add your device to a group while you're offline, using a pre-published KeyPackage without waiting for you to respond.

This document specifies the recommended strategy for managing a **pool of fresh KeyPackages** rather than publishing a single mutable "latest" key. This aligns with MLS best practices and unlocks important security and concurrency properties.

## Critical Implementation Detail: Persistent Storage

**BREAKING CHANGE from previous analysis:** KeyPackages **cannot** be stored minimally. The client must persistently store the complete `KeyPackageBundle` structure (all three fields):

1. **KeyPackage** (public bytes) - The signed public bundle
2. **private_init_key** (HPKE private key) - Used to decrypt Welcome messages
3. **private_encryption_key** (leaf encryption key) - Used for group operations

**Why it matters:**
- Without persistent `private_init_key`, clients cannot decrypt Welcome messages after restart
- Welcome messages are encrypted to the HPKE init public key, and only the private key can decrypt them
- This breaks async invitations, the core value of KeyPackages
- Current in-memory storage is **broken for production use**

## OpenMLS Storage Backend Architecture

**Investigation Findings (2025-11-05):**

OpenMLS uses the `StorageProvider` trait to manage all persistent key material, including KeyPackageBundle private keys. Understanding this architecture is critical for correct implementation.

### Key Discovery: Automatic Storage

When calling `KeyPackage::builder().build()`:
1. OpenMLS generates the complete KeyPackageBundle (public + both private keys)
2. Automatically stores the entire bundle via `provider.storage().write_key_package()`
3. The StorageProvider receives the full bundle including `private_init_key` and `private_encryption_key`

**Source evidence:**
- `openmls/openmls/src/key_packages/mod.rs:547-550` shows auto-storage on build
- `openmls/traits/src/storage.rs:221-238` defines `write_key_package()` accepting KeyPackageBundle
- Documentation confirms: "Clients keep the private key material corresponding to a key package locally in the key store" (create_key_package.md:15)

### Storage Responsibility

**The StorageProvider trait is THE authoritative storage mechanism for:**
- KeyPackageBundle (all three components)
- Group state and secrets
- Encryption keys for epochs
- Message secrets for forward secrecy

**Important distinction:**
- `write_encryption_key_pair()` is ONLY for update leaf nodes, NOT for KeyPackages
- Comment states: "This is only be used for encryption key pairs that are generated for update leaf nodes. All other encryption key pairs are stored as part of the key package or the epoch encryption key pairs."

### Current Implementation: Dual Storage

Our current implementation has **duplicate storage**:
1. OpenMLS auto-stores KeyPackageBundle via its StorageProvider
2. We extract keys and store them AGAIN in our LocalStore

**Options for Phase 2.2+:**

A. **Implement StorageProvider trait for LocalStore** (recommended for Phase 3)
   - Make LocalStore the single source of truth
   - OpenMLS directly uses our database
   - Eliminates duplication
   - Major refactoring effort

B. **Continue dual storage** (current approach, adequate for Phase 2.2)
   - Keep both storage layers
   - LocalStore manages KeyPackage pool metadata
   - OpenMLS StorageProvider handles crypto operations
   - Fix: Extract `private_encryption_key` properly (currently placeholder)

C. **Use build_without_storage()** (test-only)
   - Available for testing but not production
   - Requires manual storage management
   - Loses OpenMLS forward-secrecy guarantees

### Forward-Secrecy Requirement

From `persistence.md:9-11`:
> "OpenMLS uses the `StorageProvider` to store sensitive key material. To achieve forward-secrecy (i.e. to prevent an adversary from decrypting messages sent in the past if a client is compromised), OpenMLS frequently deletes previously used key material through calls to the `StorageProvider`."

**Implication:** Any custom storage must implement proper deletion semantics. The StorageProvider's `delete_*` functions must irrevocably delete key material with no copies kept.

### Recommendation

For Phase 2.2, continue Option B (dual storage) with fixes:
- Accept that OpenMLS auto-stores KeyPackageBundle
- Keep LocalStore for pool management and metadata
- Fix the placeholder `private_encryption_key` storage
- Plan migration to Option A (implement StorageProvider trait) in Phase 3

This preserves our LocalStore API while ensuring correct KeyPackage functionality.

## What a KeyPackage Is (in Practice)

- **Signed bundle**: Contains device's HPKE init public key, Credential, ciphersuite/capabilities, and extensions
- **Single-use**: After an Add+Welcome consumes it, never reuse it
- **For async adds**: Lets someone invite/add your device while you're offline
- **Lifetime-bound**: Includes a `not_before` and `not_after` to limit validity window

## Recommended Operating Model

### Pool

- **Anonymous list**: Upload to the directory/DS as a list keyed by identity (or account), not as a single mutable "latest"
- **Target size**: 32 KeyPackages
- **Low-watermark**: 25% of target (~8 KeyPackages) → trigger background replenishment
- **Hard cap**: Prevent storage abuse (e.g., 64 per account)

**Rationale:**
- Multiple concurrent invitations can consume several KeyPackages simultaneously
- Asynchronous operation requires fresh keys available at any time
- Pool guarantees that failed or retried invitations don't exhaust keys

### Expiry & Rotation

- **Lifetime extension**: Every KeyPackage includes `not_before = now` and `not_after = now + 7–14 days`
- **Replenishment**: Use new HPKE keypairs; **never reuse an HPKE init key** across KeyPackages
- **Garbage collection**:
  - Server: Automatically remove expired/consumed KeyPackages
  - Client: Prune locally-cached metadata for expired keys

**Rationale:**
- If a KeyPackage is compromised but never used, it expires silently
- Rotating HPKE keys limits cross-group linkability
- Confinement: HPKE private-key compromise affects only the specific groups/joins using those keys

### Consumption Semantics

- **Reservation model**: When an adder requests a KeyPackage for a target:
  1. Directory marks it as "reserved" (not yet spent)
  2. Adder includes the KeyPackageRef (hash) in the Add proposal
  3. Adder posts the Commit; directory validates KeyPackageRef was valid at reservation time
  4. After Commit is persisted, directory marks it as "spent"

- **Timeout on reservation**: If the adder crashes or times out before posting Commit, the reservation expires (e.g., 60s TTL) and the KeyPackage returns to the available pool

- **Verification**: Directory verifies that the KeyPackageRef belonged to the target identity and was unspent at reservation time

**Rationale:**
- Prevents double-spend races (two concurrent Adds using the same KeyPackage)
- Avoids burning keys on failed or abandoned attempts
- Reservation timeout ensures pool doesn't get permanently blocked by crashed clients

### Error Handling & User Experience

**If a device runs out of valid KeyPackages:**
- Directory returns distinct error: `"No valid KeyPackage available for target user"`
- Adder is informed of the error and can retry later
- Target device (when online) is prompted/alerted to replenish the pool

**Health metrics exposed to device:**
- Pool size (available + reserved + expired)
- Count soon-to-expire (< 2 days)
- Last upload timestamp
- Replenishment status/health

**Rationale:**
- Users understand why an invitation failed
- Device gets proactive feedback to replenish keys
- Transparent health monitoring prevents silent exhaustion

## Why Multiple KeyPackages Matter

### Concurrency
A single KeyPackage cannot service multiple concurrent invitations. With a pool of 32, several groups can add the device simultaneously without blocking each other.

### Asynchrony
Adders cannot wait for you to come online; they need a fresh, valid KeyPackage in the directory at the moment of invitation. A pool ensures this.

### Safety
- **One-time use**: Eliminates cross-group linkability through HPKE key reuse
- **Compromise containment**: If an HPKE private key is harvested, only the specific groups/joins using that key are exposed, not the whole device
- **Expiry**: Unused keys die automatically; no lingering keys that could leak later

## Server/Directory Rules

### Spend Log
Maintain (optionally) an immutable audit log:
```
{
  keypackage_ref: hash,
  added_by: user_id,
  group_id: bytes,
  timestamp: iso8601,
  status: "reserved" | "spent" | "expired" | "failed"
}
```
**Purpose**: Audit trail; enable transparency logs (e.g., detect equivocation); never store private keys

### TTL Enforcement
- **Reservation TTL**: ~60 seconds (if Commit not posted, release the reservation)
- **Lifetime enforcement**: Server drops KeyPackages after `not_after` is reached
- **Automatic cleanup**: Garbage collect spent/expired entries periodically

### Double-Spend Prevention
- Check at spend time: verify the KeyPackageRef has not already been marked spent
- Log the spend; reject re-use attempts with error "KeyPackage already spent"

## Test Scenarios

### 1. Concurrent Adds
**Setup**: Two adders attempt to add the same device to different groups simultaneously.
**Expected**:
- Both invitations succeed
- Two KeyPackages consumed from the pool
- Pool decremented by 2
- No double-spend error

### 2. Reservation Timeout
**Setup**: Adder reserves a KeyPackage, then crashes before posting Commit. TTL is 60s.
**Expected**:
- After 60s, reservation expires and KeyPackage returns to the pool
- Next Add can use that KeyPackage successfully
- No blocking or permanent reservation loss

### 3. Pool Exhaustion
**Setup**: Device has an empty or depleted pool. Adder requests a KeyPackage.
**Expected**:
- Directory returns error: `"No valid KeyPackage available for target user"`
- Adder receives clear feedback
- Target device (when online) is alerted to replenish
- After device uploads new KeyPackages, invitation can be retried and succeeds

### 4. Expiry
**Setup**: Adder attempts to add device with an expired KeyPackage (past `not_after`).
**Expected**:
- Directory rejects: `"KeyPackage has expired"`
- Server has garbage-collected the expired key
- Adder fetches a fresh, valid KeyPackage from the pool and retries
- Retry succeeds

### 5. Re-use Defense
**Setup**: Adder attempts to use a KeyPackageRef that was already marked spent.
**Expected**:
- Server rejects: `"KeyPackage already spent"`
- Client logs the attempted re-use as a potential attack/misconfiguration
- Adder fetches a fresh key and retries

### 6. Equivocation Detection (Transparency Log)
**Setup**: Two monitors/auditors fetch the directory state for the same device at the same moment.
**Expected** (if transparency logs enabled):
- Both monitors see the same KeyPackage list and hashes
- If an adversary tries to serve different lists to different clients, mismatch is detectable
- Clients can compare logs and alert on inconsistencies

## API Sketch (Minimal)

### Upload KeyPackages
```
POST /keypackages/upload
Content-Type: application/json

{
  "keypackages": [
    {
      "id": "<keypackage_ref>",
      "keypackage": "<base64-encoded-bytes>",
      "credential": "<credential>",
      "lifetime_ms": 604800000
    },
    ...
  ]
}

Response 200:
{
  "accepted": 32,
  "rejected": [],
  "pool_size": 32
}
```

### Reserve KeyPackage
```
POST /keypackages/reserve
Content-Type: application/json

{
  "target_user": "alice",
  "group_id": "<base64-encoded-group-id>"
}

Response 200:
{
  "keypackage_ref": "<hash>",
  "keypackage": "<base64-encoded-bytes>",
  "expires_at": "2025-11-04T12:34:56Z"
}

Response 404: { "error": "No valid KeyPackage available for target user" }
```

### Spend KeyPackage
```
POST /keypackages/spend
Content-Type: application/json

{
  "keypackage_ref": "<hash>",
  "group_id": "<base64-encoded-group-id>",
  "added_by": "bob"
}

Response 200: { "spent": true }
Response 409: { "error": "KeyPackage already spent" }
Response 404: { "error": "KeyPackage not found or expired" }
```

### Get Pool Status (auth'd as device)
```
GET /keypackages/status
Authorization: Bearer <device_auth_token>

Response 200:
{
  "available": 24,
  "reserved": 5,
  "expiring_soon": 3,
  "total": 32,
  "pool_health": "good",
  "recommended_action": null,
  "last_upload": "2025-10-28T14:30:00Z"
}
```

### Get Spent Log (audit, optional)
```
GET /keypackages/audit?user_id=alice&limit=100
Authorization: Bearer <admin_token>

Response 200:
{
  "log": [
    {
      "keypackage_ref": "<hash>",
      "added_by": "bob",
      "group_id": "<group-id>",
      "timestamp": "2025-10-28T14:35:00Z",
      "status": "spent"
    },
    ...
  ]
}
```

## Implementation Roadmap

### Phase 1 (MVP - Current)
- Single KeyPackage per device on initialization
- Fallback to strict registration (fail fast if server unavailable)
- Error handling for missing/expired keys
- **Scope**: 3 files, ~50 lines

### Phase 2 (Production - Multiple KeyPackages)
- Batch KeyPackage generation and upload on initialize
- Background replenishment task (when pool falls below 25% of target)
- Pool status endpoint for monitoring
- Expiry-aware selection (prefer keys with longer lifetime)
- **Scope**: New module `src/keypackage_pool/`, ~200 lines

### Phase 3 (Advanced - Consumption Tracking)
- Server-side reservation and spend tracking
- Equivocation detection via transparency logs
- Device prompt/alert when pool is exhausted
- Audit log queries and analysis
- **Scope**: Server changes, advanced error scenarios

## Client-Side Storage Schema

### Phase 1 (Minimal - Fixes Welcome Reception)

The absolute minimum to make async invitations work:

```sql
CREATE TABLE keypackages (
    -- Hash of serialized KeyPackage (computed by OpenMLS)
    -- Ciphersuite-dependent size: SHA256=32B, SHA384=48B, SHA512=64B
    keypackage_ref BLOB PRIMARY KEY,

    -- Serialized KeyPackage bytes (public part)
    -- Used for re-upload to server, validation, or audit
    keypackage_bytes BLOB NOT NULL,

    -- Private HPKE init key
    -- REQUIRED: Used to decrypt Welcome messages
    -- Must be stored encrypted at rest (SQLCipher recommended)
    private_init_key BLOB NOT NULL,

    -- Private encryption key for leaf node
    -- REQUIRED: Used for group message operations
    -- Must be stored encrypted at rest
    private_encryption_key BLOB NOT NULL,

    -- Creation timestamp for age tracking
    created_at INTEGER NOT NULL
);
```

**What this enables:**
- ✅ Clients can receive Welcome messages after restart
- ✅ Async invitations work correctly
- ✅ Basic KeyPackage persistence

**What this does NOT enable:**
- ❌ Pool management (no status tracking)
- ❌ Expiry-based cleanup
- ❌ Credential rotation support
- ❌ Per-message debugging

### Phase 2 (Complete - Production Ready)

Full schema supporting pool management and auditing:

```sql
CREATE TABLE keypackages (
    -- Primary key: Hash of serialized KeyPackage
    keypackage_ref BLOB PRIMARY KEY,

    -- Public bundle bytes
    keypackage_bytes BLOB NOT NULL,

    -- Private keys (stored encrypted at rest)
    private_init_key BLOB NOT NULL,
    private_encryption_key BLOB NOT NULL,

    -- Timestamps
    created_at INTEGER NOT NULL,
    uploaded_at INTEGER,
    reserved_at INTEGER,
    spent_at INTEGER,

    -- Lifecycle status
    status TEXT NOT NULL DEFAULT 'created',
    -- Values: created | uploaded | reserved | spent | expired | failed

    -- Expiry tracking (from lifetime extension)
    not_before INTEGER NOT NULL,
    not_after INTEGER NOT NULL,

    -- Credential binding (support rotation)
    credential_hash BLOB NOT NULL,
    credential_type TEXT,  -- e.g., "username", "certificate", "oidc"

    -- Ciphersuite (support migration)
    ciphersuite INTEGER NOT NULL,  -- e.g., 0x0001 for MLS_128_DHKEMX25519_...

    -- Flags
    last_resort INTEGER NOT NULL DEFAULT 0,  -- Prevent deletion after consume

    -- Server-side hints (updated from reserve/spend endpoints)
    reservation_id TEXT,
    reservation_expires_at INTEGER,
    reserved_by TEXT,  -- Adder identity
    spent_group_id BLOB,
    spent_by TEXT,  -- Adder identity who spent it

    -- Indexes for efficient queries
    INDEX idx_status ON keypackages(status),
    INDEX idx_credential ON keypackages(credential_hash),
    INDEX idx_expiry ON keypackages(not_after),
    INDEX idx_ciphersuite ON keypackages(ciphersuite)
);
```

**Additional capabilities:**
- ✅ Pool management (query by status)
- ✅ Automatic expiry cleanup
- ✅ Credential rotation support
- ✅ Multi-ciphersuite support
- ✅ Audit trail (timestamps, spent info)
- ✅ Double-spend prevention (check status)

## Key Field Rationales

### private_init_key and private_encryption_key

**Absolutely required for:**
- Welcome message decryption (private_init_key only, one-time use)
- Group state operations (private_encryption_key)

**Storage considerations:**
- Use SQLCipher (or similar) for encryption at rest
- Access control: Only decryption, never export to logs
- Backup: Include in user backups (encrypted)
- Compromise: Delete all keys immediately

### keypackage_ref

**Why it's the primary key:**
- OpenMLS uses KeyPackageRef (hash) to index everything
- It is **the only way** to correlate server reservations with local keys
- Computed deterministically from KeyPackage bytes
- Cannot be spoofed or altered

**Ciphersuite dependency:**
- SHA256-based ciphersuite: 32-byte ref
- SHA384-based ciphersuite: 48-byte ref
- SHA512-based ciphersuite: 64-byte ref

### credential_hash and ciphersuite

**Why needed for production:**
- **Credential changes**: All outstanding KeyPackages for old credential must be invalidated
  - Client generates new keys when credential rotates
  - Old KeyPackages remain available for pending invites (grace period)
  - After grace, delete keys with old credential_hash

- **Ciphersuite changes**: Cannot mix ciphersuites in same group
  - Track which ciphersuite generated each key
  - Policy changes (e.g., upgrade to stronger cipher) require new keys
  - Enforce: Only use keys with current ciphersuite

### status and timestamps

**Lifecycle tracking:**
```
created ──upload──> uploaded ──reserve──> reserved ──spend──> spent
   ↓                                                              ↓
expired (automatic, not_after passed)                    deleted or archived
   ↓                                                              ↓
failed (upload error)                              consumed (Welcome processed)
```

**Query patterns enabled:**
```sql
-- Count available keys (pool health)
SELECT COUNT(*) FROM keypackages
WHERE status = 'uploaded'
  AND not_after > current_timestamp;

-- Find expired keys for cleanup
SELECT keypackage_ref FROM keypackages
WHERE status IN ('created', 'uploaded')
  AND not_after <= current_timestamp;

-- Prevent double-spend
SELECT * FROM keypackages
WHERE keypackage_ref = ?
  AND status = 'spent';
```

### reservation_id and reservation_expires_at

**Purpose:**
- Server returns reservation details when key is reserved
- Client stores to coordinate with server
- Enables timeout logic: if reservation expires without being spent, key can be reused

**Usage:**
```sql
-- Check if reservation timed out
SELECT * FROM keypackages
WHERE reservation_id = ?
  AND reservation_expires_at < current_timestamp;
-- If found: Server released the key, status can revert to 'uploaded'
```

## Credential Rotation Procedure

When a user rotates their credential (e.g., certificate expires, security policy):

1. **Generate new keys:**
   ```
   New credential (new public cert, new signature key)
   Generate 32 KeyPackageBundle with new credential
   Store in DB with credential_hash = hash(new_credential)
   ```

2. **Upload new keys:**
   ```
   POST /keypackages/upload with new bundles
   Server returns: 32 uploaded, 0 rejected
   Update status: 'created' → 'uploaded' for all 32
   ```

3. **Mark old keys as invalid:**
   ```
   UPDATE keypackages
   SET status = 'invalidated'
   WHERE credential_hash = hash(old_credential);

   -- But don't delete yet! Grace period for pending adds to complete
   ```

4. **Grace period (24 hours):**
   - Old keys remain in DB but unavailable for new adds
   - Server rejects reserves on invalidated keys
   - Pending invitations using old keys can still complete

5. **Cleanup after grace:**
   ```
   DELETE FROM keypackages
   WHERE status = 'invalidated'
     AND created_at < ? (24 hours ago)
   ```

## Conclusion

Managing a **pool of fresh, time-bound KeyPackages** instead of a single "latest" key is the standard MLS practice. It unlocks:

1. **Concurrency**: Multiple simultaneous invitations without blocking
2. **Asynchrony**: No need to wait for the target device to come online
3. **Security**: One-time use, compromise containment, expiry-based rotation
4. **Reliability**: Reservation timeouts, exhaustion handling, clear error messages

**Critical requirement:** Persistent storage of complete KeyPackageBundle (both private keys) is **mandatory**, not optional. Without it:
- Welcome messages cannot be decrypted
- Async invitations are impossible
- Clients cannot work offline

Implementing Phase 1 schema is the **immediate priority** to fix broken async invitations. Phase 2 adds production-grade pool management.
