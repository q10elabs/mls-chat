# Phase 2.2 Implementation

## Task Specification
- Implement phase 2.2 of the keypackage pool plan per changelog/20251105-keypackage-pool-implementation-plan-openmls-aligned.md
- Follow two-sub-agent workflow: Agent A implements changes/tests; Agent B reviews and writes feedback to `feedback.md`
- After each agent completes, seek user confirmation before proceeding

## High-Level Decisions
- Introduced `KeyPackagePool` and configuration layer in `client/rust/src/mls/keypackage_pool.rs` to encapsulate pool lifecycle operations while keeping storage responsibilities in `LocalStore`.
- Keep freshly generated KeyPackages in the initial `created` state while leaving replenishment thresholds tied strictly to the `available` count, ensuring Phase 2.3 drives server uploads before the pool is considered healthy.
- Added a custom `StoredKeyPackageRef` helper implementing OpenMLS storage traits to allow deletion of expired bundles using the persisted hash reference bytes without enabling additional OpenMLS features.

## Requirements Changes
- None noted yet

## Files Modified
- `client/rust/src/mls/keypackage_pool.rs` (NEW): Added pool management implementation leveraging OpenMLS for persistence and LocalStore for metadata, plus helper struct for cleanup operations.
- `client/rust/src/mls/mod.rs`: Registered the new module and re-exported pool types.
- `client/rust/src/error.rs`: Expanded `MlsError` with a pool capacity variant used by the new logic.
- `client/rust/tests/keypackage_pool_tests.rs` (NEW): Added integration-style tests covering generation, capacity enforcement, replenishment logic, status updates, expiry cleanup, and invariants.

## Rationales and Alternatives
- Enforced hard caps using a helper that sums `created + available`, but kept low-watermark/target calculations keyed on the `available` count to reflect what the server can actually allocate; promotion to `available` remains part of Phase 2.3.
- Added a regression test ensuring OpenMLS storage releases expired bundles to guard against future serialization drift in the custom hash-reference wrapper.
- Opted for an internal helper struct for hash references rather than enabling `test-utils` features in OpenMLS, preserving production build parity while still permitting cleanup against the StorageProvider.
- Reused the existing credential generation utility to avoid duplicating crypto setup code and keep signature material management consistent with the rest of the client.

## Obstacles and Solutions
- OpenMLS `KeyPackageRef` lacked a public constructor from raw bytes → Wrapped the bytes in a `StoredKeyPackageRef` implementing the necessary storage traits so deletions can proceed.
- OpenMLS storage API requires trait-bound wrapper types in integration tests → Re-used the custom reference pattern within tests to assert pre/post cleanup state.
- `cargo test keypackage_pool` filtered out the new cases unexpectedly → Re-ran the suite with `cargo test --test keypackage_pool_tests` to verify coverage.

## Current Status
- Agent A iteration 3 complete after reconciling availability thresholds; refreshed tests (`cargo test --test keypackage_pool_tests`) pass locally.
- Awaiting updated Agent B review based on the latest changes.
