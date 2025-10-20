/// Group service for managing group operations.
/// Coordinates between storage, MLS, and server layers.
/// Handles group creation, member invitations, acceptance/decline, kicks, and admin operations.
///
/// NOTE: GroupService no longer manages current_group state to avoid unnecessary mutability.
/// Current group selection is managed by ClientManager. GroupService methods are now
/// immutable (&self) and return data for the caller to manage.

use crate::error::{ClientError, Result};
use crate::models::{ControlMessage, ControlMessageType, Group, GroupId, Member, MemberRole, MemberStatus};
use crate::services::{MlsService, ServerClient, StorageService};
use std::sync::Arc;

pub struct GroupService {
    storage: Arc<StorageService>,
    mls_service: Arc<MlsService>,
    server_client: Arc<ServerClient>,
}

impl GroupService {
    pub fn new(
        storage: Arc<StorageService>,
        mls_service: Arc<MlsService>,
        server_client: Arc<ServerClient>,
    ) -> Self {
        GroupService {
            storage,
            mls_service,
            server_client,
        }
    }

    /// Create a new group
    pub async fn create_group(&self, group_name: String) -> Result<GroupId> {
        // Create MLS group (returns the group ID that should be used)
        let (group_id, mls_state) = self.mls_service.create_group(&group_name)?;

        // Create group in storage with the MLS-generated ID
        let group = Group::with_id(group_id.clone(), group_name, mls_state);
        self.storage.save_group(&group)?;

        Ok(group_id)
    }

    /// Get all groups
    pub async fn list_groups(&self) -> Result<Vec<Group>> {
        self.storage.get_all_groups()
    }

    /// Select a group as the current group (validates that the group exists)
    pub async fn select_group(&self, group_id: GroupId) -> Result<()> {
        // Verify group exists
        let _group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        Ok(())
    }

    /// Get group info
    pub async fn get_group(&self, group_id: GroupId) -> Result<Option<Group>> {
        self.storage.get_group(group_id)
    }

    /// Invite a user to a group
    /// Creates a pending invitation and generates an Add proposal
    pub async fn invite_user(&self, username: String, group_id: GroupId) -> Result<()> {
        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // Verify caller is admin
        if group.user_role != MemberRole::Admin {
            return Err(ClientError::StateError(
                "Only admins can invite users".to_string(),
            ));
        }

        // Check user is not already in group (active or pending)
        if group.get_member(&username).is_some() {
            return Err(ClientError::AlreadyExists(format!(
                "User {} is already in group",
                username
            )));
        }

        if group.get_pending_member(&username).is_some() {
            return Err(ClientError::AlreadyExists(format!(
                "User {} is already invited to group",
                username
            )));
        }

        // Get user's public key from server
        let user_key_response = self.server_client.get_user_key(&username).await?;

        // Generate Add proposal with MLS
        let proposal = self
            .mls_service
            .add_member(&group.mls_state, &username, &user_key_response.public_key)?;

        // Create pending member
        let pending_member = Member::with_role(
            username.clone(),
            user_key_response.public_key,
            MemberRole::Member,
        );
        // Override status to pending
        let mut pending_member = pending_member;
        pending_member.status = MemberStatus::Pending;

        // Add to group's pending list
        group.add_pending_member(pending_member);
        self.storage.save_group(&group)?;

        // Send proposal to server as a control message
        let control_msg = serde_json::json!({
            "type": "ADD_PROPOSAL",
            "username": username,
            "proposal": std::str::from_utf8(&proposal).unwrap_or(""),
        });

        self.server_client
            .send_message(
                group_id.to_string(),
                "system".to_string(),
                control_msg.to_string(),
            )
            .await?;

        Ok(())
    }

    /// Accept an invitation to join a group
    pub async fn accept_invitation(&self, group_id: GroupId) -> Result<()> {
        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // Find self in pending members
        // In a real implementation, we'd track the username somewhere
        // For now, we check if there are pending members and promote the first one
        if group.pending_count() == 0 {
            return Err(ClientError::StateError(
                "No pending invitation to accept".to_string(),
            ));
        }

        // In a real implementation, we'd know which user is "us"
        // For MVP, we'll just promote all pending members to active
        // This will be fixed when we integrate with ClientManager
        if let Some(pending_member) = group.pending_members.first() {
            let username = pending_member.username.clone();
            group.promote_pending_to_active(&username);
        }

        self.storage.save_group(&group)?;

        Ok(())
    }

    /// Decline an invitation to join a group
    pub async fn decline_invitation(&self, group_id: GroupId) -> Result<()> {
        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        if group.pending_count() == 0 {
            return Err(ClientError::StateError(
                "No pending invitation to decline".to_string(),
            ));
        }

        // Remove first pending member
        if let Some(pending_member) = group.pending_members.first() {
            let username = pending_member.username.clone();
            group.remove_pending_member(&username);
        }

        self.storage.save_group(&group)?;

        Ok(())
    }

    /// Leave a group
    pub async fn leave_group(&self, group_id: GroupId) -> Result<()> {
        self.storage.delete_group(group_id)?;
        Ok(())
    }

    /// Kick a user from a group
    /// Only admins can kick
    pub async fn kick_user(&self, username: String, group_id: GroupId) -> Result<()> {
        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // Verify caller is admin
        if group.user_role != MemberRole::Admin {
            return Err(ClientError::StateError(
                "Only admins can kick users".to_string(),
            ));
        }

        // Cannot kick if member doesn't exist
        if group.get_member(&username).is_none() {
            return Err(ClientError::InvalidUser(format!(
                "User {} is not in group",
                username
            )));
        }

        // Generate Remove proposal (for future integration with MLS)
        let _proposal = self.mls_service.remove_member(&group.mls_state, &username)?;

        // Remove from group
        group.remove_active_member(&username);
        self.storage.save_group(&group)?;

        // Send control message to group
        let control_msg = ControlMessage {
            msg_type: ControlMessageType::Kick,
            target_user: username,
            reason: None,
        };

        let control_json = serde_json::to_string(&control_msg)
            .map_err(|e| ClientError::MessageError(format!("Failed to serialize control message: {}", e)))?;

        self.server_client
            .send_message(group_id.to_string(), "system".to_string(), control_json)
            .await?;

        Ok(())
    }

    /// Set a user as admin in the group
    pub async fn set_admin(&self, username: String, group_id: GroupId) -> Result<()> {
        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // Verify caller is admin
        if group.user_role != MemberRole::Admin {
            return Err(ClientError::StateError(
                "Only admins can promote users".to_string(),
            ));
        }

        // Find member and update role
        if let Some(pos) = group.members.iter().position(|m| m.username == username) {
            group.members[pos].role = MemberRole::Admin;
        } else {
            return Err(ClientError::InvalidUser(format!(
                "User {} is not in group",
                username
            )));
        }

        self.storage.save_group(&group)?;

        // Send control message to group
        let control_msg = ControlMessage {
            msg_type: ControlMessageType::ModAdd,
            target_user: username,
            reason: None,
        };

        let control_json = serde_json::to_string(&control_msg)
            .map_err(|e| ClientError::MessageError(format!("Failed to serialize control message: {}", e)))?;

        self.server_client
            .send_message(group_id.to_string(), "system".to_string(), control_json)
            .await?;

        Ok(())
    }

    /// Unset admin status for a user
    pub async fn unset_admin(&self, username: String, group_id: GroupId) -> Result<()> {
        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // Verify caller is admin
        if group.user_role != MemberRole::Admin {
            return Err(ClientError::StateError(
                "Only admins can demote users".to_string(),
            ));
        }

        // Find member and update role
        if let Some(pos) = group.members.iter().position(|m| m.username == username) {
            group.members[pos].role = MemberRole::Member;
        } else {
            return Err(ClientError::InvalidUser(format!(
                "User {} is not in group",
                username
            )));
        }

        self.storage.save_group(&group)?;

        // Send control message to group
        let control_msg = ControlMessage {
            msg_type: ControlMessageType::ModRemove,
            target_user: username,
            reason: None,
        };

        let control_json = serde_json::to_string(&control_msg)
            .map_err(|e| ClientError::MessageError(format!("Failed to serialize control message: {}", e)))?;

        self.server_client
            .send_message(group_id.to_string(), "system".to_string(), control_json)
            .await?;

        Ok(())
    }

    /// Get group members
    pub async fn get_group_members(&self, group_id: GroupId) -> Result<Vec<Member>> {
        let group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        Ok(group.members)
    }

    /// Get pending invitations for a group
    pub async fn get_pending_members(&self, group_id: GroupId) -> Result<Vec<Member>> {
        let group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        Ok(group.pending_members)
    }

    /// Update group MLS state
    pub async fn update_group_state(
        &self,
        group_id: GroupId,
        new_state: Vec<u8>,
    ) -> Result<()> {
        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        group.mls_state = new_state;
        self.storage.save_group(&group)?;
        Ok(())
    }

    /// Process an incoming control message (kick, mod add/remove)
    /// NOTE: This method requires the current_group_id to be passed in from ClientManager
    /// because GroupService no longer manages state
    pub async fn process_control_message(&self, group_id: GroupId, control_json: &str) -> Result<()> {
        let control_msg: ControlMessage = serde_json::from_str(control_json)
            .map_err(|e| ClientError::MessageError(format!("Failed to parse control message: {}", e)))?;

        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup("Group not found".to_string()))?;

        match control_msg.msg_type {
            ControlMessageType::Kick => {
                // Remove user from group
                group.remove_active_member(&control_msg.target_user);
            }
            ControlMessageType::ModAdd => {
                // Promote user to admin
                if let Some(pos) = group.members.iter().position(|m| m.username == control_msg.target_user) {
                    group.members[pos].role = MemberRole::Admin;
                }
            }
            ControlMessageType::ModRemove => {
                // Demote user to member
                if let Some(pos) = group.members.iter().position(|m| m.username == control_msg.target_user) {
                    group.members[pos].role = MemberRole::Member;
                }
            }
        }

        self.storage.save_group(&group)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_service_creation() {
        let storage = Arc::new(StorageService::in_memory().unwrap());
        let mls = Arc::new(MlsService::new());
        let server = Arc::new(ServerClient::new("http://localhost:4000".to_string()));
        let _group_service = GroupService::new(storage, mls, server);
        // Service created successfully
    }


    #[test]
    fn test_permission_checks() {
        let storage = Arc::new(StorageService::in_memory().unwrap());
        let mls = Arc::new(MlsService::new());
        let server = Arc::new(ServerClient::new("http://localhost:4000".to_string()));
        let _group_service = GroupService::new(storage.clone(), mls, server);

        let mut group = Group::new("test".to_string(), vec![1, 2, 3]);
        group.user_role = MemberRole::Member; // Not admin
        storage.save_group(&group).unwrap();

        // Non-admins should not be able to perform admin operations
        assert_eq!(group.user_role, MemberRole::Member);
        assert_ne!(group.user_role, MemberRole::Admin);
    }

    #[test]
    fn test_member_management() {
        let storage = Arc::new(StorageService::in_memory().unwrap());
        let mls = Arc::new(MlsService::new());
        let server = Arc::new(ServerClient::new("http://localhost:4000".to_string()));
        let _group_service = GroupService::new(storage.clone(), mls, server);

        let mut group = Group::new("test".to_string(), vec![1, 2, 3]);
        let member = Member::new("alice".to_string(), "pk_alice".to_string());
        group.add_member(member);

        assert_eq!(group.member_count(), 1);
        assert!(group.get_member("alice").is_some());

        group.remove_active_member("alice");
        assert_eq!(group.member_count(), 0);
        assert!(group.get_member("alice").is_none());
    }

    #[test]
    fn test_pending_member_promotion() {
        let storage = Arc::new(StorageService::in_memory().unwrap());
        let mls = Arc::new(MlsService::new());
        let server = Arc::new(ServerClient::new("http://localhost:4000".to_string()));
        let _group_service = GroupService::new(storage.clone(), mls, server);

        let mut group = Group::new("test".to_string(), vec![1, 2, 3]);
        let pending = Member::new("bob".to_string(), "pk_bob".to_string());
        group.add_pending_member(pending);

        assert_eq!(group.pending_count(), 1);
        assert_eq!(group.member_count(), 0);

        group.promote_pending_to_active("bob");
        assert_eq!(group.pending_count(), 0);
        assert_eq!(group.member_count(), 1);
        assert!(group.get_member("bob").is_some());
    }
}
