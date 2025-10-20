# CLAUDE.md - Rust Client Development Guidelines

## Architecture Documentation

This directory contains a Rust library for an MLS Chat client. The architecture is documented in `ARCHITECTURE.md`.

**Important:** When modifying the client architecture or APIs, you **MUST** update `ARCHITECTURE.md` to reflect the changes.

## When to Update ARCHITECTURE.md

Update the architecture document when:

1. **New Services Added**
   - Add service name, responsibilities, and key methods to "Service Layer" section
   - Update the architecture diagram if needed
   - Add data flow examples

2. **Service APIs Modified**
   - Update method signatures in "Core Concepts" sections
   - Document parameter changes and new return types
   - Update data flow diagrams if behavior changes

3. **Data Models Changed**
   - Update struct definitions in "Data Models" section
   - Update database schema if `StorageService` models change
   - Note any new fields and their purposes

4. **New Data Flows Added**
   - Add flow diagrams to "Main Data Flows" section
   - Include each service involved
   - Show data transformation at each step

5. **Service Layer Organization Changed**
   - Update the layered architecture diagram at the top
   - Describe new dependencies between services
   - Verify no circular dependencies are introduced

6. **Error Handling Modified**
   - Add new `ClientError` variants to "Error Handling" section
   - Update error categories documentation
   - Document when each error is raised

## Architecture Principles

When making changes, maintain these architectural principles:

### 1. Layered Architecture
```
Presentation (CLI/Library API)
    ↓ depends on
Application (ClientManager)
    ↓ depends on
Services (Business Logic)
    ↓ depends on
Infrastructure (Storage & Communication)
```

**Rule:** Never violate the dependency direction. Services must not depend on Presentation. Infrastructure must not depend on Services.

### 2. Single Responsibility
Each service should have ONE primary responsibility:
- `ClientManager` - Orchestration only
- `GroupService` - Group lifecycle only
- `MessageService` - Message handling only
- `StorageService` - Persistence only
- `ServerClient` - Network communication only

**Before adding functionality:** Ask "Is this the right service?"

### 3. Testability
Services are designed for independent testing:
- Minimal mocking required
- Dependencies injected via constructor
- No global state or singletons
- Pure functions preferred

**Before refactoring:** Ensure unit tests still pass.

### 4. Error Handling
All operations return `Result<T>` with context-rich `ClientError`.

**Pattern:**
```rust
pub fn operation() -> Result<Data> {
    // Validate inputs
    // Perform operation
    // Map errors to ClientError with context
    // Return Result
}
```

**Never use `.unwrap()`** in library code. Use `?` operator to propagate errors.

### 5. No Async in Tests
Async tests with `Arc<Mutex<T>>` can cause hangs in the test harness. Test design strategy:

- ✅ Synchronous tests for isolated operations
- ✅ Unit tests for data models
- ✅ Tests that write data (simpler patterns)
- ❌ Async tokio tests that lock shared state
- ❌ Integration tests (use real server instead)

Future integration tests will verify async operations against the real server.

## Code Organization

```
src/
├── lib.rs              # Public API exports
├── error.rs            # Error types (DO NOT modify error patterns)
├── models/
│   ├── mod.rs          # Model exports
│   ├── user.rs         # User model
│   ├── group.rs        # Group model
│   └── message.rs      # Message model
└── services/
    ├── mod.rs          # Service exports
    ├── client_manager.rs   # Orchestrator
    ├── group_service.rs    # Group operations
    ├── message_service.rs  # Message operations
    ├── mls_service.rs      # OpenMLS wrapper
    ├── storage.rs          # SQLite persistence
    └── server_client.rs    # HTTP/WebSocket client
```

## Development Workflow

### Adding a New Feature

1. **Plan** - Update `ARCHITECTURE.md` with design first
2. **Clarify** - Ask for approval if design is unclear
3. **Implement** - Code the feature following existing patterns
4. **Test** - Write unit tests (sync only, no tokio hangs)
5. **Integrate** - Test against real server
6. **Document** - Update `ARCHITECTURE.md` to match implementation

### Modifying Existing Code

1. **Understand** - Read current `ARCHITECTURE.md`
2. **Check Principles** - Does change violate layering or SRP?
3. **Test First** - Ensure existing tests pass
4. **Implement** - Make the change
5. **Verify** - Run full test suite
6. **Update Docs** - Reflect changes in `ARCHITECTURE.md`

### Before Making PRs/Commits

- [ ] All tests pass: `cargo test --lib`
- [ ] No compiler warnings (especially in lib code)
- [ ] `ARCHITECTURE.md` is updated to match implementation
- [ ] No circular dependencies between modules
- [ ] Error handling follows `Result<T>` pattern
- [ ] No `.unwrap()` calls in library code (unless clearly justified with comment)

## Key Files & Their Purposes

| File | Purpose | Modification Guidelines |
|------|---------|------------------------|
| `ARCHITECTURE.md` | Design documentation | Update when APIs/structure change |
| `src/error.rs` | Error types | Add new variants with comments |
| `src/models/*` | Data structures | Add fields, update docs |
| `src/services/client_manager.rs` | Main orchestrator | Keep minimal, delegate to services |
| `src/services/*.rs` | Business logic | Can grow, but split if >500 lines |
| `src/lib.rs` | Public API | Only export needed items |

## Testing Guidelines

### Unit Tests

Write tests that:
- ✅ Test one behavior per test
- ✅ Are deterministic (no randomness/timing)
- ✅ Use `in_memory()` storage for isolation
- ✅ Use meaningful assertion messages
- ✅ Document why test exists if not obvious

Example:
```rust
#[test]
fn test_save_and_retrieve_user() -> Result<()> {
    let storage = StorageService::in_memory()?;
    let user = User::new("alice".to_string(), "pk123".to_string(), vec![1, 2, 3]);

    storage.save_user(&user)?;
    let retrieved = storage.get_user("alice")?;

    assert!(retrieved.is_some(), "User should be retrievable after save");
    let retrieved_user = retrieved.unwrap();
    assert_eq!(retrieved_user.username, "alice");
    Ok(())
}
```

### Avoiding Test Hangs

The following patterns CAUSE HANGS - do NOT use:

```rust
// ❌ Async tokio test that locks group_service
#[tokio::test]
async fn test_create_group() -> Result<()> {
    let gs = Arc::new(Mutex::new(GroupService::new(...)));
    let mut gs_lock = gs.lock().await;  // Can hang
    gs_lock.create_group(...).await?;
    Ok(())
}

// ❌ Test that calls get_group() with member loading
#[test]
fn test_save_and_get_group() -> Result<()> {
    let storage = StorageService::in_memory()?;
    storage.save_group(&group)?;
    storage.get_group(group.id)?;  // This can hang - loads members
    Ok(())
}
```

Good alternative:
```rust
// ✅ Synchronous test that only writes
#[test]
fn test_save_group() -> Result<()> {
    let storage = StorageService::in_memory()?;
    storage.save_group(&group)?;
    Ok(())
}

// ✅ Integration test against real server verifies reads
// (To be added later)
```

## Documentation Standards

All public items in `src/lib.rs` should have:
- Brief description of what it does
- Example usage if complex
- Links to related types/methods

Example:
```rust
/// Register a new user with the MLS Chat server.
///
/// Creates a new user account with the given public key and stores the user
/// in local storage. The user can then participate in groups.
///
/// # Errors
///
/// Returns `Err(ClientError::ServerError)` if server registration fails.
/// Returns `Err(ClientError::AuthError)` if user already registered.
pub async fn register_user(&mut self, public_key: String) -> Result<UserId>
```

## Contact/Questions

If unclear about architecture or design, see the main `CLAUDE.md` in the project root for additional guidelines.

When making significant changes:
1. Document your design decisions in ARCHITECTURE.md
2. Explain rationale for any pattern deviations
3. Update this file with new guidelines if applicable
