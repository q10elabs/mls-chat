# Plan Transition Summary

**Date:** 2025-11-05
**Action:** Transitioned from duplicative design to OpenMLS-aligned design

---

## What Changed

### Old Plan (20251028-keypackage-pool-implementation-plan.md)
- ❌ LocalStore stored complete KeyPackageBundle (bytes + private keys)
- ❌ OpenMLS StorageProvider also stored the same bundle
- ❌ ~400-500 bytes per key duplicated
- ✅ But phases 2.0 (server) was correct

### New Plan (20251105-keypackage-pool-implementation-plan-openmls-aligned.md)
- ✅ OpenMLS StorageProvider stores key material (automatic, we don't touch it)
- ✅ LocalStore stores only metadata (status, timestamps, not_after)
- ✅ ~100-150 bytes per key (40% reduction)
- ✅ Clear separation of concerns
- ✅ Phases 2.0 (server) unchanged

---

## What to Keep vs. Revert

### KEEP
- ✅ Phase 1: Error handling fixes (pre-existing)
- ✅ Phase 2.0: Server KeyPackageStore and API (completed, working)
  - `server/src/db/keypackage_store.rs`
  - `server/src/routes/keypackages.rs`
  - `server/tests/keypackage_store_tests.rs`

### REVERT
- ❌ Phase 2.1: Client storage layer (remove)
  - Remove: keypackages table from `client/rust/src/storage.rs`
  - Remove: save_key_package_bundle(), load_key_package_bundle(), etc.
  - Remove: storage tests for bundle persistence

- ❌ Phase 2.2: Client KeyPackagePool (remove)
  - Remove: `client/rust/src/mls/keypackage_pool.rs`
  - Remove: `client/rust/tests/keypackage_pool_tests.rs`
  - Remove: keypackage_pool module from `client/rust/src/mls/mod.rs`

### REIMPLEMENT (Following New Plan)
- 🔄 Phase 2.1: LocalStore metadata table (NEW)
  - Add: keypackage_pool_metadata table (9 fields, metadata only)
  - Add: CRUD methods for metadata
  - Keep LocalStore identities table (unchanged)

- 🔄 Phase 2.2: KeyPackagePool (NEW)
  - Use OpenMLS StorageProvider directly (no LocalStore bundle storage)
  - Track state in LocalStore metadata only
  - All 49 tests can be re-used with updated expectations

- 🔄 Phases 2.3-2.6: Client integration, server coordination, CLI, docs (NEW)

---

## Key Architectural Insights

### Why This Is Better

1. **OpenMLS owns key material**
   - When you call `KeyPackageBuilder::build()`, OpenMLS automatically calls `StorageProvider::write_key_package()`
   - We get this for free - no manual storage needed
   - OpenMLS handles forward-secrecy guarantees

2. **Single source of truth**
   - Previously: Bundle in LocalStore AND in OpenMLS storage (duplicate)
   - Now: Bundle ONLY in OpenMLS, metadata tracking ONLY in LocalStore
   - Easier to reason about, less sync problems

3. **Proper separation of concerns**
   - Crypto storage (keys, serialization, deletion) → OpenMLS StorageProvider
   - State tracking (status, timestamps, server hints) → LocalStore metadata
   - Each layer does what it's designed for

4. **Storage efficiency**
   - Save ~40% storage per pool (7-11 KB for 32 keys)
   - No redundant copying of key material

---

## Code Size Comparison

### Phase 2.1 (LocalStore Enhancement)

**Old approach:**
```
Lines of code: ~200
- Full bundle table (14 fields)
- 4 persistence methods (save, load, get, delete)
- Serialization/deserialization logic
```

**New approach:**
```
Lines of code: ~100
- Metadata-only table (9 fields)
- 8 simple methods (create, update, count, query, etc.)
- No cryptography, just SQL
```

### Phase 2.2 (KeyPackagePool)

**Logic unchanged:**
- generate_and_update_pool() - same algorithm, different storage backend
- get_available_count() - query metadata instead of full bundle table
- should_replenish() - same threshold logic
- cleanup_expired() - same cleanup, but calls provider.storage().delete_key_package()

**Net result:** Logic from old Phase 2.2 (275 lines) is preserved, but storage integration is simplified

---

## Testing Strategy

### Existing Phase 2.2 Tests (49 tests)

**All tests can be reused:**
- ✅ Pool generation - now validates OpenMLS + LocalStore metadata
- ✅ Replenishment logic - queries metadata, not bundle table
- ✅ Expiry detection - scans metadata timestamps
- ✅ Mark as spent - updates metadata status
- ✅ Hard cap enforcement - counts metadata entries

**Test updates:**
- Remove: Tests that verify private keys are stored in LocalStore
- Keep: Tests that verify pool mechanics work (counts, thresholds, state)
- Add: Tests that verify both storages are kept in sync

---

## Migration Path (Your Task)

1. **Revert commits:**
   ```bash
   git revert <commit-for-phase-2.2>  # Remove keypackage_pool.rs, tests
   git revert <commit-for-phase-2.1>  # Remove bundle storage from storage.rs
   ```
   (Or manually delete the files/changes)

2. **Verify:**
   - `client/rust/src/storage.rs` has only identities table
   - `client/rust/src/mls/mod.rs` has no keypackage_pool module
   - Server code unchanged (Phase 2.0 still there)

3. **Next:**
   - Follow new plan `20251105-keypackage-pool-implementation-plan-openmls-aligned.md`
   - Start with Phase 2.1 (metadata table only)
   - Proceed through phases 2.2-2.6

---

## Documents Created

1. **`20251105-storageprovider-analysis.md`**
   - Deep analysis of OpenMLS StorageProvider
   - What it does, how it works, code locations

2. **`20251105-plan-openmls-redundancy-analysis.md`**
   - Side-by-side comparison of old vs. new approach
   - Shows what's redundant and why
   - Recommendation for reverting and reimplementing

3. **`20251105-phase2.2-refactor-recommendation.md`**
   - Decision framework for Option A vs. Option B
   - You chose Option B (revert and reimplement)

4. **`20251105-keypackage-pool-implementation-plan-openmls-aligned.md`** (THIS IS YOUR NEW PLAN)
   - Complete revised implementation plan
   - Keeps Phase 1 and Phase 2.0
   - Replaces Phase 2.1-2.2 with new design

5. **`20251105-plan-transition-summary.md`** (this document)
   - Quick reference for what changed

---

## Success Metrics for New Implementation

When you're done with Phase 2.6, you should have:

- ✅ No duplicate bundle storage
- ✅ OpenMLS StorageProvider used properly
- ✅ ~40% storage savings (15-21 KB per pool instead of 24-32 KB)
- ✅ Clear separation: OpenMLS handles crypto, we handle state
- ✅ All 49 tests passing (rewritten with correct expectations)
- ✅ Cleaner codebase (fewer total lines)
- ✅ Proper architecture that downstream phases (2.3-2.6) can build on

---

## Questions to Clarify

Before you start reimplementing, confirm:

1. **Storage backend:** Do you want to keep using `rusqlite::Connection` with `conn.execute()` pattern?
   - (Assuming yes, based on existing code)

2. **Error handling:** Should pool errors be `ClientError`, `MlsError`, or new `PoolError`?
   - (Recommend: Use existing `MlsError` enum)

3. **OpenMLS provider access:** In KeyPackagePool, how will we get `provider.storage()`?
   - (Recommend: Pass as `&impl OpenMlsProvider` to cleanup_expired())

4. **Refresh frequency:** How often should CLI call refresh?
   - (Recommend: Every N messages or every T seconds)

---

**Status:** Ready for you to revert code and follow the new plan
**Estimated Timeline:** 10-14 days (Phases 2.1-2.6)
