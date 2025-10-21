# Client Prototype Implementation

## Task Specification

Prototype the MLS chat client as described in README.md with the following requirements:

- Create/connect to MLS groups
- Support commands: `/invite`, `/list`, and regular messages
- Display messages in format: `#groupname <username> message...`
- Display control messages: `#groupname action...`
- Store client state in ~/.mlschat directory
- Reuse existing server code where possible

## Implementation Plan

### Architecture Decisions

1. **Authentication**: Username-only authentication (no OIDC, no complex identity)
2. **Key Management**: Private keys stored locally on client, server stores public keys only
3. **No Backup System**: Remove all backup-related functionality
4. **Simplified Identity**: Username is the only identity mechanism
5. **Testing**: Use server as library in client tests for integration testing

### Success Criteria

✅ **Functional Requirements:**

- Real MLS encryption/decryption
- Group creation and management
- Username-based invitations
- Real-time messaging
- Simple terminal interface

✅ **Security Requirements:**

- Forward secrecy maintained
- Post-compromise security
- Secure local key storage
- Message authentication

✅ **Quality Requirements:**

- Comprehensive test coverage (80+ tests)
- Server library integration for testing
- Clean architecture with separation of concerns
- Error handling and recovery
- Documentation and examples

## Implementation Decisions

### Technology Stack

1. **MLS Library**: Using `openmls` Rust crate (most mature implementation)
2. **Networking**: Hybrid approach - REST for commands, WebSocket for real-time messages
3. **Local Storage**: SQLite database in `~/.mlschat` for client state
4. **Invitation System**: Simple "invite by username" - inviter creates Add proposal, Welcome stored on server
5. **Terminal UI**: Basic stdout/stdin (no TUI library) for simplicity
6. **Group Support**: Single group per client instance (scope limited for prototype)

### Architecture Approach

- Client as standalone binary in `client/rust/`
- Reuse server's models and REST types where possible
- MLS group state stored locally with SQLite
- KeyPackages managed locally, uploaded to server on registration
- Welcome messages fetched from server's invitation inbox

## Current Status

### Planning Phase

- ✅ Reviewed server implementation
- ✅ Clarified technical decisions with user
- ⏳ Creating implementation plan
