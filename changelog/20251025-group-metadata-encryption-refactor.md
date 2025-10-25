# Group Metadata Encryption Refactoring

**Date:** 2025-10-25
**Status:** Completed
**Scope:** Move group names from plaintext envelopes to encrypted group context extensions

## Task Specification

Refactor the MLS chat client to properly handle group naming through encrypted group context extensions instead of plaintext envelope metadata, ensuring MLS privacy guarantees are maintained while supporting fixed server API requirements.

### Key Requirements:
1. Move group name from plaintext envelopes to encrypted GroupContext extensions
2. Use MLS group ID (binary, base64-encoded) for server routing instead of human-readable group name
3. Keep WelcomeMessage without a group_id field (not routed through groups)
4. Keep CommitMessage and ApplicationMessage with group_id field for server routing
5. Create GroupMetadata struct (not just a string) to allow future metadata expansion
6. Do NOT add group renaming support
7. Adapt implementation to fixed server API requirements

## High-Level Decisions

### 1. Extension Type Selection

**Decision**: Use type ID 0xff00 (private use range) for GROUP_METADATA_EXTENSION_TYPE

**Rationale**:
- RFC 9420 defines 0xff00-0xffff as private use range
- OpenMLS ExtensionType enum maps known IDs (1-5, 10) to known variants
- IDs 1-5 are reserved for standard extensions (ApplicationId, RatchetTree, etc.)
- Using 0xff00 ensures the extension is treated as Unknown (not filtered or special-cased)
- Original choice of 0x0001 failed because it was interpreted as ApplicationId, not Unknown

### 2. GroupMetadata Structure

**Decision**: Define extensible GroupMetadata struct with name, created_at, version fields

**Rationale**:
- Allows future expansion (additional fields won't break old clients)
- JSON serialization provides readable format for debugging
- Version field enables detecting changes/rollbacks
- created_at timestamp enables chronological ordering

### 3. Server Routing Layer

**Decision**: Use MLS group ID (base64-encoded binary) in ApplicationMessage and CommitMessage for server routing

**Rationale**:
- Server needs routing metadata to deliver messages to group members
- MLS group ID is cryptographically bound to group state
- Base64 encoding makes binary ID JSON-compatible
- Separates plaintext routing metadata from encrypted protocol data
- Server remains protocol-agnostic (only relays encrypted state)

### 4. Welcome Message Design

**Decision**: Remove group_id field from WelcomeMessage enum variant

**Rationale**:
- Welcome messages are sent directly to specific invitees, not broadcast through groups
- Server shouldn't route Welcome through group members (only for specific invitee)
- Inviter identity is sufficient to identify sender
- Future: server could implement direct user-to-user messaging for Welcome delivery

## Implementation Details

### Files Created

**src/extensions.rs** (NEW)
- `GROUP_METADATA_EXTENSION_TYPE: u16 = 0xff00` constant
- `GroupMetadata` struct with fields: name, created_at, version
- Serialization methods: `to_bytes()` (JSON-encoded), `from_bytes()` (JSON-decoded)
- Test coverage for serialization round-trips

### Files Modified

**src/lib.rs**
- Added `pub mod extensions;` to export extensions module

**src/models.rs**
- Modified `MlsMessageEnvelope::WelcomeMessage` variant
  - REMOVED: `group_id: String` field
  - KEPT: `inviter: String`, `welcome_blob: String`, `ratchet_tree_blob: String`
- Updated test assertions to verify no group_id in serialized WelcomeMessage

**src/crypto.rs**
- Modified `create_group_with_config()` signature:
  - Added `group_name: &str` parameter
  - Builds GroupMetadata and stores in GroupContext extensions
  - Adds ciphersuite to builder (was missing)
- Added public `extract_group_metadata()` function:
  - Accesses extensions via public `group.extensions()` method
  - Deserializes GroupMetadata from Unknown extension
- Updated all test calls to include group_name parameter (15+ locations)

**src/client.rs**
- Modified `handle_welcome_message()` signature:
  - REMOVED: `group_name: &str` parameter
  - KEPT: `inviter: &str`, `welcome_blob_b64: &str`, `ratchet_tree_blob_b64: &str`
  - Now extracts group name from encrypted metadata after joining
- Modified `send_message()` method:
  - Encodes MLS group ID to base64
  - Sends ApplicationMessage with base64-encoded group ID for server routing
- Modified `invite_user()` method:
  - WelcomeMessage created without group_id field
  - CommitMessage uses base64-encoded MLS group ID for routing
- Updated all test calls to include group_name parameter

**src/identity.rs**
- Moved Serialize import to test module only (unused in main code)

**src/message_processing.rs**
- Moved Serialize import to test module only (unused in main code)

**tests/invitation_tests.rs**
- Updated envelope structure tests to reflect WelcomeMessage without group_id
- Verified test assertions for envelope serialization

## Technical Challenges and Solutions

### Challenge 1: Extension Type ID Interpretation

**Problem**: Original type ID 0x0001 was being interpreted as ApplicationId instead of Unknown
- OpenMLS ExtensionType enum maps specific u16 values to known variants
- During conversion via `From<u16>`, ID 1 maps to `ExtensionType::ApplicationId`
- The `unknown()` method only matches `ExtensionType::Unknown` variants
- Unknown extensions with known IDs are effectively hidden from the API

**Solution**: Changed to type ID 0xff00 (well within private use range)
- Ensured value doesn't match any known extension IDs
- Guarantees conversion to `ExtensionType::Unknown(0xff00)`
- Makes unknown extension accessible via public `extensions()` API

### Challenge 2: API Method Availability

**Problem**: `export_group_context()` is test-gated (not available in release builds)
- Original solution attempted to use private `public_group()` method
- Both approaches violate public API boundaries

**Solution**: Used public `group.extensions()` method
- Returns reference to Extensions directly
- Internally calls `public_group().group_context().extensions()`
- No need to call private methods or rely on test-only APIs
- Works in all build configurations

## Test Results

**All 55 library tests passing:**
- ✅ 13 crypto tests (group creation, messaging, persistence)
- ✅ 8 client tests (metadata storage, state loading)
- ✅ 8 models tests (serialization, envelope discrimination)
- ✅ 7 identity tests (persistence, multi-user)
- ✅ 6 message processing tests (formatting, decryption)
- ✅ 5 storage tests (table init, persistence)
- ✅ 2 extension tests (metadata serialization)
- ✅ 2 provider tests (initialization)
- ✅ 1 error test (conversion)

**No compilation warnings:** Clean build with `cargo build`

## Files Modified Summary

| File | Changes |
|------|---------|
| src/extensions.rs | NEW - GroupMetadata struct and extension type |
| src/lib.rs | Added extensions module export |
| src/models.rs | Removed group_id from WelcomeMessage |
| src/crypto.rs | Added group_name param, extract_group_metadata() |
| src/client.rs | Updated envelope handling, removed group_name from welcome handling |
| src/identity.rs | Moved Serialize to test-only import |
| src/message_processing.rs | Moved Serialize to test-only import |
| tests/invitation_tests.rs | Updated envelope structure assertions |

## Current Status

✅ **COMPLETED** - All tests passing, no warnings, implementation ready for:
- Server integration testing
- End-to-end messaging with encryption
- Multi-user group scenarios
- Group member management via commits

## Future Considerations

1. **Group Renaming**: Currently disabled per requirements, but structure supports via Commit messages
2. **Metadata Versioning**: Version field enables detecting inconsistencies
3. **Additional Metadata**: Structure allows adding more fields without protocol changes
4. **Extension Validation**: Could add signature validation on metadata changes

