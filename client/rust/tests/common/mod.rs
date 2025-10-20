/// Common test utilities and helpers for integration tests
/// Provides setup, teardown, and assertion helpers for testing the MLS chat client

use mls_chat_client::error::Result;
use mls_chat_client::models::{Group, GroupId, Member, MemberRole, Message, User};
use mls_chat_client::services::StorageService;
use std::sync::Arc;
use tempfile::TempDir;

/// Test context holding isolated storage and temporary directory
pub struct TestContext {
    pub storage: Arc<StorageService>,
    pub temp_dir: TempDir,
}

impl TestContext {
    /// Create a new test context with in-memory storage
    pub fn new_in_memory() -> Result<Self> {
        let storage = Arc::new(StorageService::in_memory()?);
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        Ok(TestContext { storage, temp_dir })
    }

    /// Create a new test context with file-based storage
    pub fn new_with_file_storage() -> Result<Self> {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test_client.db");
        let storage = Arc::new(StorageService::new(db_path)?);

        Ok(TestContext { storage, temp_dir })
    }
}

/// Helper for creating test users
pub struct TestUserBuilder {
    username: String,
    public_key: String,
}

impl Default for TestUserBuilder {
    fn default() -> Self {
        TestUserBuilder {
            username: "test_user".to_string(),
            public_key: "test_pk_123".to_string(),
        }
    }
}

impl TestUserBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn username(mut self, username: &str) -> Self {
        self.username = username.to_string();
        self
    }

    pub fn public_key(mut self, pk: &str) -> Self {
        self.public_key = pk.to_string();
        self
    }

    pub fn build(self) -> User {
        User::new(self.username, self.public_key, Vec::new())
    }
}

/// Helper for creating test groups
pub struct TestGroupBuilder {
    name: String,
    members: Vec<String>,
}

impl Default for TestGroupBuilder {
    fn default() -> Self {
        TestGroupBuilder {
            name: "test_group".to_string(),
            members: vec!["test_user".to_string()],
        }
    }
}

impl TestGroupBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    pub fn members(mut self, members: Vec<&str>) -> Self {
        self.members = members.iter().map(|m| m.to_string()).collect();
        self
    }

    pub fn build(self) -> Group {
        // Create group with empty mls_state (32 bytes of zeros)
        let mut group = Group::new(self.name, vec![0u8; 32]);

        // Add members
        for member_name in self.members {
            let member = Member::new(member_name.clone(), format!("pk_{}", member_name));
            group.add_member(member);
        }

        group
    }
}

/// Helper for creating test messages
pub struct TestMessageBuilder {
    sender: String,
    group_id: GroupId,
    content: String,
}

impl Default for TestMessageBuilder {
    fn default() -> Self {
        TestMessageBuilder {
            sender: "test_user".to_string(),
            group_id: GroupId::from_string("test_group_id"),
            content: "test message".to_string(),
        }
    }
}

impl TestMessageBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sender(mut self, sender: &str) -> Self {
        self.sender = sender.to_string();
        self
    }

    pub fn group_id(mut self, group_id: &str) -> Self {
        self.group_id = GroupId::from_string(group_id);
        self
    }

    pub fn content(mut self, content: &str) -> Self {
        self.content = content.to_string();
        self
    }

    pub fn build(self) -> Message {
        Message::new(self.group_id, self.sender, self.content)
    }
}

/// Custom assertions for test validation
pub struct Assertions;

impl Assertions {
    /// Assert that a group exists and has expected member count
    pub fn assert_group_has_members(
        group: &Group,
        expected_count: usize,
        message: &str,
    ) {
        assert_eq!(
            group.members.len(),
            expected_count,
            "Group member count mismatch: {}",
            message
        );
    }

    /// Assert that a specific member exists in group
    pub fn assert_member_in_group(group: &Group, member_name: &str, message: &str) {
        let exists = group.members.iter().any(|m| m.username == member_name);
        assert!(exists, "Member {} not found in group: {}", member_name, message);
    }

    /// Assert that a member has specific role
    pub fn assert_member_has_role(group: &Group, member_name: &str, role_str: &str, message: &str) {
        let member = group.members.iter().find(|m| m.username == member_name);
        assert!(member.is_some(), "Member {} not found: {}", member_name, message);

        let member = member.unwrap();
        let expected_role = match role_str {
            "Member" => MemberRole::Member,
            "Moderator" => MemberRole::Moderator,
            "Admin" => MemberRole::Admin,
            _ => panic!("Unknown role: {}", role_str),
        };
        assert_eq!(
            member.role, expected_role,
            "Member {} role mismatch: {}",
            member_name, message
        );
    }

    /// Assert that pending members count matches expected
    pub fn assert_pending_members_count(
        group: &Group,
        expected_count: usize,
        message: &str,
    ) {
        assert_eq!(
            group.pending_members.len(),
            expected_count,
            "Pending members count mismatch: {}",
            message
        );
    }

    /// Assert that a user exists in pending list
    pub fn assert_user_pending(group: &Group, member_name: &str, message: &str) {
        let exists = group.pending_members.iter().any(|m| m.username == member_name);
        assert!(exists, "User {} not in pending list: {}", member_name, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_context_in_memory() -> Result<()> {
        let _ctx = TestContext::new_in_memory()?;
        Ok(())
    }

    #[test]
    fn test_test_user_builder() {
        let user = TestUserBuilder::new()
            .username("alice")
            .public_key("pk_alice")
            .build();

        assert_eq!(user.username, "alice");
        assert_eq!(user.public_key, "pk_alice");
    }

    #[test]
    fn test_test_group_builder() {
        let group = TestGroupBuilder::new()
            .name("test_group")
            .members(vec!["alice", "bob"])
            .build();

        assert_eq!(group.name, "test_group");
        assert_eq!(group.members.len(), 2);
    }

    #[test]
    fn test_test_message_builder() {
        let msg = TestMessageBuilder::new()
            .sender("alice")
            .group_id("group_1")
            .content("Hello!")
            .build();

        assert_eq!(msg.sender, "alice");
        assert_eq!(msg.content, "Hello!");
    }
}
