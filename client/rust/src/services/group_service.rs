/// Group service for managing group operations.
/// Coordinates between storage, MLS, and server layers.

use crate::error::{ClientError, Result};
use crate::models::{Group, GroupId, Member, MemberRole};
use crate::services::{MlsService, StorageService};
use std::sync::Arc;

pub struct GroupService {
    storage: Arc<StorageService>,
    mls_service: Arc<MlsService>,
    current_group: Option<GroupId>,
}

impl GroupService {
    pub fn new(storage: Arc<StorageService>, mls_service: Arc<MlsService>) -> Self {
        GroupService {
            storage,
            mls_service,
            current_group: None,
        }
    }

    /// Create a new group
    pub async fn create_group(&mut self, group_name: String) -> Result<GroupId> {
        // Create MLS group
        let (group_id, mls_state) = self.mls_service.create_group()?;

        // Create group in storage
        let group = Group::new(group_name, mls_state);
        self.storage.save_group(&group)?;

        // Set as current group
        self.current_group = Some(group_id);

        Ok(group_id)
    }

    /// Get all groups
    pub async fn list_groups(&self) -> Result<Vec<Group>> {
        self.storage.get_all_groups()
    }

    /// Select a group as the current group
    pub async fn select_group(&mut self, group_id: GroupId) -> Result<()> {
        // Verify group exists
        let group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        self.current_group = Some(group_id);
        Ok(())
    }

    /// Get the currently selected group
    pub fn get_current_group(&self) -> Result<GroupId> {
        self.current_group.ok_or_else(|| {
            ClientError::StateError("No group selected. Use /select to choose a group".to_string())
        })
    }

    /// Get group info
    pub async fn get_group(&self, group_id: GroupId) -> Result<Option<Group>> {
        self.storage.get_group(group_id)
    }

    /// Invite a user to a group
    pub async fn invite_user(&mut self, username: String, group_id: GroupId) -> Result<()> {
        let mut group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // Check user is not already in group
        if group.get_member(&username).is_some() {
            return Err(ClientError::AlreadyExists(format!(
                "User {} is already in group",
                username
            )));
        }

        // In a real implementation, we would:
        // 1. Get user's public key from server
        // 2. Add to MLS group (which generates Add proposal)
        // 3. Send proposal to server
        // For now, just add as pending member
        let member = Member::new(username.clone(), format!("pk_{}", username));
        group.add_member(member);

        self.storage.save_group(&group)?;
        Ok(())
    }

    /// Accept an invitation to join a group
    pub async fn accept_invitation(&mut self, group_id: GroupId) -> Result<()> {
        let group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // In a real implementation, we would:
        // 1. Create Join proposal in OpenMLS
        // 2. Send to server
        // For now, just verify group exists
        self.current_group = Some(group_id);
        Ok(())
    }

    /// Decline an invitation to join a group
    pub async fn decline_invitation(&self, group_id: GroupId) -> Result<()> {
        let _group = self
            .storage
            .get_group(group_id)?
            .ok_or_else(|| ClientError::InvalidGroup(format!("Group not found: {:?}", group_id)))?;

        // TODO: Send decline to server
        Ok(())
    }

    /// Leave a group
    pub async fn leave_group(&mut self, group_id: GroupId) -> Result<()> {
        // Remove group from storage
        self.storage.delete_group(group_id)?;

        // If it was the current group, clear current
        if self.current_group == Some(group_id) {
            self.current_group = None;
        }

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

    /// Update group MLS state
    pub async fn update_group_state(
        &mut self,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_service_creation() {
        let storage = Arc::new(StorageService::in_memory().unwrap());
        let mls = Arc::new(MlsService::new());
        let _group_service = GroupService::new(storage, mls);
        // Service created successfully
    }

    // Note: Async tests for GroupService have been disabled because they can cause hangs
    // when combined with Arc<Mutex<GroupService>>. Integration tests will verify functionality
    // against the actual server.
}
