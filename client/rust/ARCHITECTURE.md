# Rust MLS Chat Client - Architecture

## Overview

The Rust client is organized as a **library-first design** with an optional CLI wrapper. The architecture separates concerns into distinct layers:

```
┌─────────────────────────────────────────┐
│     Presentation Layer (CLI)            │
│   - User interaction                    │
│   - Terminal UI                         │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│     Application Layer                   │
│   ClientManager                         │
│   - Orchestrates all services           │
│   - Manages lifecycle                   │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│     Service Layer (Business Logic)      │
│   ┌──────────────────────────────────┐  │
│   │ GroupService                     │  │
│   │ - Group lifecycle management     │  │
│   │ - Member operations              │  │
│   └──────────────────────────────────┘  │
│   ┌──────────────────────────────────┐  │
│   │ MessageService                   │  │
│   │ - Send/receive messages          │  │
│   │ - Encryption/decryption          │  │
│   │ - Message storage                │  │
│   └──────────────────────────────────┘  │
│   ┌──────────────────────────────────┐  │
│   │ MlsService                       │  │
│   │ - OpenMLS operations             │  │
│   │ - Group state management         │  │
│   └──────────────────────────────────┘  │
└────────────────┬────────────────────────┘
                 │
┌────────────────┼────────────────────────┐
│  Infrastructure Layer                   │
│ ┌──────────────────────────────────┐   │
│ │ StorageService                   │   │
│ │ - SQLite persistence             │   │
│ │ - User/Group/Message storage     │   │
│ └──────────────────────────────────┘   │
│ ┌──────────────────────────────────┐   │
│ │ ServerClient                     │   │
│ │ - HTTP communication (REST)      │   │
│ │ - WebSocket communication        │   │
│ │ - Server API wrapper             │   │
│ └──────────────────────────────────┘   │
└─────────────────────────────────────────┘
```

## Core Concepts

### 1. ClientManager (Application Orchestrator)

The single entry point for all client operations. Coordinates between services and manages the overall client lifecycle.

**Responsibilities:**
- Initialize and wire up all services
- Provide high-level API for group and message operations
- Manage user registration and authentication
- Coordinate sync operations with server
- Handle graceful shutdown

**Key Methods:**
```rust
pub async fn new(username, server_url, config_dir) -> Result<Self>
pub async fn register_user(public_key) -> Result<UserId>
pub async fn create_group(name) -> Result<GroupId>
pub async fn send_message(content) -> Result<()>
pub async fn sync() -> Result<()>
pub async fn shutdown() -> Result<()>
```

### 2. Service Layer

#### GroupService
Manages group lifecycle and membership operations.

**Responsibilities:**
- Create and list groups
- Select active group
- Invite and manage members
- Handle group state

**Data Flow:**
```
User Input → GroupService → MlsService (group ops)
                          → StorageService (persist)
                          → ServerClient (upload)
```

#### MessageService
Handles message encryption, decryption, and storage.

**Responsibilities:**
- Encrypt messages before sending
- Decrypt received messages
- Store messages in local database
- Poll for new messages
- Search message history

**Data Flow:**
```
Send Message:
  User Content → MessageService → MlsService (encrypt)
                               → ServerClient (send)
                               → StorageService (store)

Receive Message:
  ServerClient (poll) → MessageService → MlsService (decrypt)
                                      → StorageService (store)
                                      → Return to UI
```

#### MlsService
OpenMLS integration for group cryptography.

**Responsibilities:**
- Create MLS groups
- Add members to groups
- Encrypt messages for groups
- Decrypt group messages
- Handle group state updates

**Note:** Currently a placeholder implementation. Full OpenMLS integration pending dependency availability.

### 3. Infrastructure Layer

#### StorageService
Persistent local storage using SQLite.

**Database Schema:**
```sql
users
  - id (UUID, primary key)
  - username (unique)
  - public_key
  - local_key_material (binary)
  - created_at

groups
  - id (UUID, primary key)
  - name
  - mls_state (binary, OpenMLS serialized state)
  - user_role (Member/Moderator/Admin)
  - created_at

members
  - id (auto)
  - group_id (foreign key → groups)
  - username
  - public_key
  - role (Member/Moderator/Admin)
  - joined_at

messages
  - id (UUID, primary key)
  - group_id (foreign key → groups)
  - sender
  - content (decrypted)
  - timestamp
  - local_only (true = awaiting server confirmation)
```

**Key Methods:**
```rust
pub fn save_user(&User) -> Result<()>
pub fn get_user(username) -> Result<Option<User>>
pub fn save_group(&Group) -> Result<()>
pub fn get_group(GroupId) -> Result<Option<Group>>
pub fn save_message(&Message) -> Result<()>
pub fn get_group_messages(GroupId, limit) -> Result<Vec<Message>>
```

#### ServerClient
HTTP and WebSocket wrapper for server communication.

**REST Endpoints Used:**
```
POST /users
  - Register user with public key

GET /users/{username}
  - Retrieve user's public key

POST /backup/{username}
  - Store encrypted client state

GET /backup/{username}
  - Retrieve encrypted client state

POST /groups/{group_id}/messages
  - Send message to group

GET /groups/{group_id}/messages
  - Poll for messages in group
```

**WebSocket Protocol:**
```
Connect: WS /ws/{username}

Subscribe to group:
  {"action": "subscribe", "group_id": "..."}

Send message:
  {"action": "message", "group_id": "...", "encrypted_content": "..."}

Receive broadcast:
  {"type": "message", "sender": "...", "group_id": "...", "encrypted_content": "..."}

Unsubscribe:
  {"action": "unsubscribe", "group_id": "..."}
```

### 4. Data Models

#### User
```rust
pub struct User {
    pub id: UserId,
    pub username: String,
    pub public_key: String,
    pub local_key_material: Vec<u8>,
    pub created_at: DateTime<Utc>,
}
```

#### Group
```rust
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub members: Vec<Member>,
    pub mls_state: Vec<u8>,
    pub user_role: MemberRole,
    pub created_at: DateTime<Utc>,
}
```

#### Member
```rust
pub struct Member {
    pub username: String,
    pub public_key: String,
    pub role: MemberRole,  // Member | Moderator | Admin
    pub joined_at: DateTime<Utc>,
}
```

#### Message
```rust
pub struct Message {
    pub id: MessageId,
    pub group_id: GroupId,
    pub sender: String,
    pub content: String,  // Decrypted
    pub timestamp: DateTime<Utc>,
    pub local_only: bool,  // Not yet confirmed by server
}
```

### 5. Error Handling

Centralized error type: `ClientError`

**Error Categories:**
- `StorageError` - Database or local persistence issues
- `ServerError` - Server communication failures
- `MlsError` - OpenMLS operation failures
- `InvalidGroup` / `InvalidUser` - Resource not found
- `StateError` - Invalid client state for operation
- `AuthError` - Authentication failures
- `MessageError` - Message processing failures

**Pattern:**
```rust
pub type Result<T> = std::result::Result<T, ClientError>;

// All fallible operations return Result<T>
// Errors propagate up for handling at UI layer
```

## Main Data Flows

### 1. User Registration Flow

```
CLI User Input
    ↓
ClientManager::register_user(public_key)
    ↓
ServerClient::register_user(username, public_key)  [HTTP POST /users]
    ↓
StorageService::save_user(user)  [SQLite]
    ↓
Return UserId to UI
```

### 2. Create Group Flow

```
CLI User Input: /create #groupname
    ↓
ClientManager::create_group(name)
    ↓
GroupService::create_group(name)
    ├→ MlsService::create_group()  [Generate MLS state]
    └→ StorageService::save_group(group)  [SQLite]
    ↓
Return GroupId, automatically select as current group
```

### 3. Send Message Flow

```
CLI User Input: message text
    ↓
ClientManager::send_message(content)
    ↓
MessageService::send_message(content)
    ├→ Get current group from GroupService
    ├→ MlsService::encrypt_message(content)  [Encrypt with group key]
    ├→ ServerClient::send_message(encrypted)  [HTTP POST /groups/{id}/messages]
    └→ StorageService::save_message(message)  [SQLite, mark as local_only=true]
    ↓
Update UI with message sent
```

### 4. Receive Message Flow

```
ServerClient polls OR WebSocket broadcasts message
    ↓
MessageService::process_incoming_message(sender, encrypted_content)
    ├→ MlsService::decrypt_message(encrypted)  [Decrypt with group key]
    ├→ StorageService::save_message(message)  [SQLite, mark as local_only=false]
    └→ Return decrypted Message
    ↓
Update UI with new message
```

### 5. Invite User Flow

```
CLI User Input: /invite @username
    ↓
ClientManager::invite_user(username)
    ↓
GroupService::invite_user(username, current_group_id)
    ├→ ServerClient::get_user_key(username)  [Fetch from server]
    ├→ MlsService::add_member(username, public_key)  [Update MLS group]
    └→ StorageService::save_group(updated_group)  [SQLite]
    ↓
Update UI with member added
```

## Dependency Directions

```
Presentation (CLI)
    ↓
ClientManager
    ↓
Services (Group, Message, Mls)
    ↓
Infrastructure (Storage, ServerClient)
```

**Key Rule:** No circular dependencies. Each layer only depends on layers below it.

## Testing Strategy

### Unit Tests
- **Models**: Serialization, validation, basic operations (✅ 18 tests)
- **Error handling**: Error construction and propagation (✅ 3 tests)
- **Services**: Isolated service operations with mocked dependencies (✅ 17 tests)
- **Utilities**: Base64 encoding/decoding (✅ 2 tests)

**Total: 38 passing tests**

### Integration Tests (To be implemented)
- Full workflows against real MLS Chat server
- WebSocket message streaming
- Group membership operations
- Message encryption/decryption verification

## Configuration & Initialization

```rust
// Typical initialization flow
let config_dir = PathBuf::from("~/.mlschat");
let client = ClientManager::new(
    "alice".to_string(),
    "http://localhost:4000".to_string(),
    config_dir,
).await?;

// Register if new user
if !client.is_registered() {
    let user_id = client.register_user(public_key).await?;
}

// Load existing groups from local storage
let groups = client.list_groups().await?;

// Application ready for CLI commands
```

## Future Enhancements

1. **Full OpenMLS Integration** - When dependencies become available
2. **WebSocket Streaming** - Real-time message reception
3. **Group Admin Features** - Kick, mod, unmod operations
4. **Backup/Restore** - Client state persistence to server
5. **Offline Support** - Queue operations when disconnected
6. **Multi-device Sync** - Synchronize state across devices
7. **Message Search** - Full-text search in local messages
8. **End-to-End Encryption** - Device-to-device verification
