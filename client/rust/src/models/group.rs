/// Group model for the MLS chat client.
/// Represents a group, its members, and MLS state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(Uuid);

impl GroupId {
    pub fn new() -> Self {
        GroupId(Uuid::new_v4())
    }

    pub fn from_string(s: &str) -> Self {
        GroupId(Uuid::parse_str(s).unwrap_or_else(|_| Uuid::new_v4()))
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for GroupId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberRole {
    Member,
    Moderator,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub username: String,
    pub public_key: String,
    pub role: MemberRole,
    pub joined_at: DateTime<Utc>,
}

impl Member {
    pub fn new(username: String, public_key: String) -> Self {
        Member {
            username,
            public_key,
            role: MemberRole::Member,
            joined_at: Utc::now(),
        }
    }

    pub fn with_role(username: String, public_key: String, role: MemberRole) -> Self {
        Member {
            username,
            public_key,
            role,
            joined_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub members: Vec<Member>,
    /// Serialized OpenMLS group state (binary data)
    pub mls_state: Vec<u8>,
    /// Current user's role in this group
    pub user_role: MemberRole,
    pub created_at: DateTime<Utc>,
}

impl Group {
    pub fn new(name: String, mls_state: Vec<u8>) -> Self {
        Group {
            id: GroupId::new(),
            name,
            members: Vec::new(),
            mls_state,
            user_role: MemberRole::Admin, // Creator is admin
            created_at: Utc::now(),
        }
    }

    pub fn add_member(&mut self, member: Member) {
        if !self.members.iter().any(|m| m.username == member.username) {
            self.members.push(member);
        }
    }

    pub fn get_member(&self, username: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.username == username)
    }

    pub fn member_count(&self) -> usize {
        self.members.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_id_generation() {
        let id1 = GroupId::new();
        let id2 = GroupId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_member_creation() {
        let member = Member::new("alice".to_string(), "pk_alice".to_string());
        assert_eq!(member.username, "alice");
        assert_eq!(member.role, MemberRole::Member);
    }

    #[test]
    fn test_member_with_role() {
        let member = Member::with_role(
            "bob".to_string(),
            "pk_bob".to_string(),
            MemberRole::Admin,
        );
        assert_eq!(member.role, MemberRole::Admin);
    }

    #[test]
    fn test_group_creation() {
        let group = Group::new("test_group".to_string(), vec![1, 2, 3]);
        assert_eq!(group.name, "test_group");
        assert_eq!(group.member_count(), 0);
        assert_eq!(group.user_role, MemberRole::Admin);
    }

    #[test]
    fn test_group_add_member() {
        let mut group = Group::new("test_group".to_string(), vec![1, 2, 3]);
        let member = Member::new("alice".to_string(), "pk_alice".to_string());

        group.add_member(member);
        assert_eq!(group.member_count(), 1);
        assert!(group.get_member("alice").is_some());
    }

    #[test]
    fn test_group_no_duplicate_members() {
        let mut group = Group::new("test_group".to_string(), vec![1, 2, 3]);
        let member1 = Member::new("alice".to_string(), "pk_alice".to_string());
        let member2 = Member::new("alice".to_string(), "pk_alice".to_string());

        group.add_member(member1);
        group.add_member(member2);
        assert_eq!(group.member_count(), 1);
    }

    #[test]
    fn test_group_serialization() {
        let group = Group::new("test_group".to_string(), vec![1, 2, 3]);
        let json = serde_json::to_string(&group).unwrap();
        let deserialized: Group = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test_group");
    }
}
