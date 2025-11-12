# Task: Use Configurable Reservation Timeout in KeyPackageStore

**Date:** 2025-11-11

## Task Specification

The server has a command-line configurable setting `--reservation-timeout-seconds` that allows users to set the KeyPackage reservation timeout. However, the implementation in `keypackage_store.rs` uses a hardcoded package-level constant `RESERVATION_TTL_SECONDS = 60` instead of using the configurable value.

**Objective:** Remove the hardcoded constant and make the code use the configurable timeout from `ServerConfig`.

## Current State

- `server/src/config.rs`: Defines `reservation_timeout_seconds: i64` config field (line 24)
- `server/src/main.rs`: Passes config value to `ServerConfig` (line 51)
- `server/src/handlers/rest.rs`: Uses `config.reservation_timeout_seconds` when calling `reserve_key_package_with_timeout()` (line 306)
- `server/src/db/keypackage_store.rs`: Has hardcoded `RESERVATION_TTL_SECONDS = 60` (line 19)
  - `reserve_key_package()` method calls `reserve_key_package_with_timeout()` with hardcoded constant (line 245)
  - `reserve_key_package_with_timeout()` already accepts timeout parameter but `reserve_key_package()` doesn't expose it

## Implementation Plan

1. **Remove hardcoded constant:** Delete line 19 (`const RESERVATION_TTL_SECONDS: i64 = 60`)

2. **Update `reserve_key_package()` method:**
   - Change signature from `pub async fn reserve_key_package(...)` to accept `timeout_seconds: i64` parameter
   - Pass this parameter to `reserve_key_package_with_timeout()` instead of the constant

3. **Update all callers of `reserve_key_package()`:**
   - Search for all calls to `reserve_key_package()` that don't use timeout parameter
   - Replace with calls to `reserve_key_package_with_timeout()` or pass timeout to `reserve_key_package()`
   - The handler in `rest.rs:301` already uses `reserve_key_package_with_timeout()` with config timeout, so no change needed there

4. **Update tests:**
   - Tests that call `reserve_key_package()` need to be checked
   - Some tests may need to pass timeout parameter or switch to `reserve_key_package_with_timeout()`
   - Tests like `test_reservation_timeout_releases_key()` (line 793) use `reserve_key_package()` and should continue to work with a default or explicit timeout

5. **Update module documentation:**
   - Update the module-level doc comment (line 6) that currently states "Reservation system with TTL (60s timeout)" since this is now configurable

## Files to Modify

- `server/src/db/keypackage_store.rs`

## Rationale

- **Consistency:** The configuration is already defined and passed through the system; it makes sense to use it
- **Flexibility:** Allows runtime control of reservation timeout without code recompilation
- **Alignment:** Makes the code match the intended architecture where `ServerConfig` is the source of truth for this setting

## Status

✅ **Completed**

### Changes Made

1. **Removed hardcoded constant:** Deleted `const RESERVATION_TTL_SECONDS: i64 = 60` from line 18
2. **Removed `reserve_key_package()` method:** Deleted the wrapper method (lines 224-233) that exposed the hardcoded constant
3. **Updated all test calls:** Updated 5 test cases to call `reserve_key_package_with_timeout()` directly with explicit 60-second timeout:
   - `test_reservation_ttl_enforcement()` - line 526
   - `test_concurrent_reservations()` - lines 707, 712
   - `test_reservation_timeout_releases_key()` - lines 788, 808
4. **Updated module documentation:** Changed "Reservation system with TTL (60s timeout)" to "Reservation system with configurable TTL timeout" (line 6)

### Verification

- All 40 server unit tests pass ✅
- The REST handler at `server/src/handlers/rest.rs:301` was already using `reserve_key_package_with_timeout()` with `config.reservation_timeout_seconds`, so no changes needed there
- Code now consistently uses the configurable timeout from `ServerConfig`

### Impact

- **REST API:** Unchanged - already used configurable timeout
- **Internal API:** Callers must now explicitly specify timeout, forcing awareness of the configurable setting
- **Architecture:** Cleaner - removes duplicate method that hid the configuration option
