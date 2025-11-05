# Task Specification
- Implement Phase 2.3 of the keypackage-pool plan: integrate refresh logic into `MlsConnection`/`MlsClient`, ensure expiry cleanup, replenishment, and server upload functionality plus adequate tests, following the two-agent workflow.

# High-Level Decisions
- Implemented `MlsConnection::refresh_key_packages` to coordinate expiry cleanup, replenishment, and uploads via the real server API while preserving local identity state.
- Added configurable `KeyPackagePoolConfig` plumbing (setters on connection/client) so tests can shrink pool sizes without impacting production defaults.
- Extended the server crate with a `POST /keypackages/upload` handler backed by `KeyPackageStore`, keeping persisted data in sync with client expectations.
- Ensured metadata transitions (`created → uploaded → available`) occur immediately after successful uploads to maintain accurate pool counts.
- Updated the client API layer to batch-upload KeyPackages, encoding payloads in base64 to match the new server contract.

# Requirements Changes
- Clarified that KeyPackage uploads use real server endpoints during refresh and tests leverage server library helpers.

# Files Modified
- client/rust/src/mls/connection.rs
- client/rust/src/client.rs
- client/rust/src/api.rs
- client/rust/tests/client_tests.rs
- server/Cargo.toml
- server/src/db/init.rs
- server/src/handlers/rest.rs
- server/src/server.rs
- server/src/db/keypackage_store.rs

# Rationales and Alternatives
- Exercising the real server endpoint keeps serialization and schema in sync; building local stubs was rejected to satisfy the project requirement for “real endpoint” coverage.
- Injecting the keypackages schema during database initialization avoided introducing async setup hooks inside `create_pool`.
- Exposing pool configuration via explicit setters keeps configurability inside the client API surface instead of relying on environment flags or feature toggles.

# Obstacles and Solutions
- Sandbox initially blocked local sockets for integration tests; reran `cargo test` commands with elevated permissions to allow loopback bindings.
- Existing documentation snippets are non-executable, so doc tests fail; limited automated runs to `cargo test --lib --tests --bins` and recorded the gap for future cleanup.

# Current Status
- Phase 2.3 implementation for Agent A is complete and ready for Agent B review.
- Tests executed: `cargo test --lib --tests --bins` (client, escalated for sockets) and `cargo test` (server, escalated); doc tests remain pending pending documentation clean-up.
