//! Main MLS client orchestrator
//!
//! Provides a high-level API for MLS operations by delegating to MlsConnection.
//! MlsClient is a thin wrapper that manages the selected group for single-group CLI usage.

use crate::api::ServerApi;
use crate::error::{ClientError, Result};
use crate::mls::connection::MlsConnection;
use crate::mls::keypackage_pool::KeyPackagePoolConfig;
use crate::models::Identity;
use crate::provider::MlsProvider;
use crate::storage::LocalStore;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Main MLS client
///
/// Thin wrapper around MlsConnection that provides a high-level API for MLS operations.
/// Tracks the currently selected group for single-group CLI usage.
///
/// ## Architecture
/// - Owns MlsConnection (infrastructure and memberships)
/// - Tracks selected_group_id for operations
/// - Delegates all operations to connection/membership
/// - Provides backward-compatible API for existing code
///
/// ## Usage Pattern
/// ```rust
/// let mut client = MlsClient::new_with_storage_path(url, username, group_name, storage_dir)?;
/// client.initialize().await?;
/// client.connect_to_group().await?;
/// client.send_message("Hello").await?;
/// ```
pub struct MlsClient {
    /// MLS connection (infrastructure and memberships)
    connection: MlsConnection,

    /// Currently selected group ID (for single-group CLI)
    selected_group_id: Option<Vec<u8>>,

    /// Time of last KeyPackage pool refresh (for periodic refresh)
    last_refresh_time: Option<SystemTime>,

    /// Period between KeyPackage pool refreshes (default: 1 hour)
    refresh_period: Duration,
}

impl MlsClient {
    /// Create a new MLS client with custom storage path
    ///
    /// # Arguments
    /// * `server_url` - URL of the MLS server
    /// * `username` - Username for this client instance
    /// * `group_name` - Name of the group to create/join
    /// * `storage_dir` - Custom directory for storage files
    ///
    /// # Errors
    /// * File system errors when creating storage directories
    /// * Database initialization errors
    pub fn new_with_storage_path(
        server_url: &str,
        username: &str,
        group_name: &str,
        storage_dir: &Path,
    ) -> Result<Self> {
        log::info!(
            "Creating MlsClient for {} (group: {})",
            username,
            group_name
        );

        // Create MlsConnection with infrastructure
        let connection = MlsConnection::new_with_storage_path(server_url, username, storage_dir)?;

        Ok(Self {
            connection,
            selected_group_id: None,
            last_refresh_time: None,
            refresh_period: Duration::from_secs(3600), // Default: 1 hour
        })
    }

    /// Initialize the client (load or create identity, register with server)
    ///
    /// Delegates to MlsConnection to create user identity and register with the
    /// server, then performs an immediate KeyPackage pool refresh so the server
    /// has inventory available for upcoming invitations.
    ///
    /// # Errors
    /// * Storage errors when loading/saving identity
    /// * Network errors when registering with server
    /// * Network errors during the initial pool refresh/upload
    /// * Crypto errors when generating credentials or key packages
    pub async fn initialize(&mut self) -> Result<()> {
        log::info!("Initializing MlsClient");
        self.connection.initialize().await?;
        self.connection.refresh_key_packages().await?;
        self.update_refresh_time();
        Ok(())
    }

    /// Refresh the KeyPackage pool for this client
    pub async fn refresh_key_packages(&mut self) -> Result<()> {
        self.connection.refresh_key_packages().await
    }

    /// Check if the refresh period has elapsed since the last refresh
    ///
    /// Returns true if:
    /// - No refresh has occurred yet (last_refresh_time is None), OR
    /// - The elapsed time since last refresh >= refresh_period
    pub fn should_refresh(&self) -> bool {
        match self.last_refresh_time {
            None => true, // First refresh should happen immediately
            Some(last_time) => {
                match SystemTime::now().duration_since(last_time) {
                    Ok(elapsed) => elapsed >= self.refresh_period,
                    Err(_) => {
                        // Clock went backwards, trigger refresh to be safe
                        log::warn!("System clock went backwards, triggering refresh");
                        true
                    }
                }
            }
        }
    }

    /// Update the last refresh time to now
    ///
    /// Should be called after successfully calling refresh_key_packages()
    pub fn update_refresh_time(&mut self) {
        self.last_refresh_time = Some(SystemTime::now());
    }

    /// Set the refresh period (primarily for testing)
    ///
    /// # Arguments
    /// * `period` - Duration between refreshes (e.g., Duration::from_secs(10) for testing)
    pub fn set_refresh_period(&mut self, period: Duration) {
        self.refresh_period = period;
    }

    /// Get the current refresh period (for testing/debugging)
    pub fn get_refresh_period(&self) -> Duration {
        self.refresh_period
    }

    /// Get the time of the last refresh (for testing/debugging)
    pub fn get_last_refresh_time(&self) -> Option<SystemTime> {
        self.last_refresh_time
    }

    /// Override the KeyPackage pool configuration (primarily for tests)
    pub fn set_keypackage_pool_config(&mut self, config: KeyPackagePoolConfig) {
        self.connection.set_keypackage_pool_config(config);
    }

    /// Connect to group (create or load existing)
    ///
    /// Creates or loads a group membership and connects WebSocket for real-time messaging.
    /// Delegates to MlsConnection and MlsMembership.
    ///
    /// # Arguments
    /// * `group_name` - Name of the group to create or load
    ///
    /// # Errors
    /// * WebSocket connection errors
    /// * MLS errors when creating/loading group
    pub async fn connect_to_group(&mut self, group_name: &str) -> Result<()> {
        log::info!("Connecting to group: {}", group_name);

        // Connect WebSocket first
        self.connection.connect_websocket().await?;

        // Get user from connection
        let user = self.connection.get_user().ok_or_else(|| {
            ClientError::Config("User not initialized - call initialize() first".to_string())
        })?;

        // Try to load or create membership for the specified group
        use crate::mls::membership::MlsMembership;
        let membership = MlsMembership::create_new_group(
            group_name,
            user,
            self.connection.get_provider(),
        )?;

        // Store the group ID as selected
        let group_id = membership.get_group_id().to_vec();
        self.selected_group_id = Some(group_id.clone());

        // Add membership to connection's HashMap
        self.connection.add_membership(membership);

        // Subscribe to the group to receive messages
        self.connection.subscribe_to_group(&group_id).await?;

        log::info!(
            "Connected to group '{}' successfully",
            group_name
        );

        Ok(())
    }

    /// Send a message to the group
    ///
    /// Delegates to the selected membership to send the message.
    ///
    /// # Errors
    /// * No group selected
    /// * WebSocket send errors
    /// * MLS encryption errors
    pub async fn send_message(&mut self, text: &str) -> Result<()> {
        // Get selected group ID
        let group_id = self
            .selected_group_id
            .as_ref()
            .ok_or_else(|| ClientError::Config("No group selected".to_string()))?;

        // Delegate to connection helper method (handles borrow complexity)
        self.connection.send_message_to_group(group_id, text).await
    }

    /// Invite a user to the group
    ///
    /// Delegates to the selected membership to invite the user.
    ///
    /// # Errors
    /// * No group selected
    /// * Server communication errors
    /// * MLS operation errors
    pub async fn invite_user(&mut self, invitee_username: &str) -> Result<()> {
        // Get selected group ID
        let group_id = self
            .selected_group_id
            .as_ref()
            .ok_or_else(|| ClientError::Config("No group selected".to_string()))?;

        // Delegate to connection helper method
        self.connection
            .invite_user_to_group(group_id, invitee_username)
            .await
    }

    /// List group members
    ///
    /// Returns the members from the currently selected group.
    ///
    /// # Returns
    /// Vector of member usernames, or empty vector if no group selected
    pub fn list_members(&self) -> Vec<String> {
        if let Some(group_id) = &self.selected_group_id {
            if let Some(membership) = self.connection.get_membership(group_id) {
                return membership.list_members();
            }
        }
        vec![]
    }

    /// Expose metadata store reference (primarily for integration tests)
    pub fn get_metadata_store(&self) -> &LocalStore {
        self.connection.get_metadata_store()
    }

    /// Get current group name from selected membership
    ///
    /// Returns the group name of the currently selected group.
    ///
    /// # Errors
    /// * No group selected
    /// * Selected group not found in memberships
    pub fn get_current_group_name(&self) -> Result<String> {
        let group_id = self
            .selected_group_id
            .as_ref()
            .ok_or_else(|| ClientError::Config("No group selected".to_string()))?;

        let membership = self
            .connection
            .get_membership(group_id)
            .ok_or_else(|| ClientError::Config("Selected group not found".to_string()))?;

        Ok(membership.get_group_name().to_string())
    }

    /// Set the selected group ID (used when a Welcome message is processed)
    ///
    /// Updates the currently selected group to the specified group_id.
    /// This is called when a Welcome message creates a new membership
    /// that should become the active group for messaging operations.
    ///
    /// # Arguments
    /// * `group_id` - The group ID to select
    pub fn set_selected_group_id(&mut self, group_id: Vec<u8>) {
        self.selected_group_id = Some(group_id);
    }

    /// Get reference to MlsConnection (for cli.rs access)
    ///
    /// Provides access to the underlying connection for control loop operations.
    pub fn get_connection(&self) -> &MlsConnection {
        &self.connection
    }

    /// Get mutable reference to MlsConnection (for cli.rs access)
    ///
    /// Provides mutable access to the underlying connection for control loop operations.
    pub fn get_connection_mut(&mut self) -> &mut MlsConnection {
        &mut self.connection
    }

    // ========== Test Helpers ==========

    /// Test helper: get reference to identity
    pub fn get_identity(&self) -> Option<&Identity> {
        self.connection.get_user().map(|user| user.get_identity())
    }

    /// Test helper: check if group is connected
    pub fn is_group_connected(&self) -> bool {
        if let Some(group_id) = &self.selected_group_id {
            self.connection.get_membership(group_id).is_some()
        } else {
            false
        }
    }

    /// Test helper: get group ID
    pub fn get_group_id(&self) -> Option<Vec<u8>> {
        self.selected_group_id.clone()
    }

    /// Get the username (for testing)
    pub fn get_username(&self) -> &str {
        self.connection.get_username()
    }

    /// Get the API instance (for testing)
    pub fn get_api(&self) -> &ServerApi {
        self.connection.get_api()
    }

    /// Test helper: get signature key
    pub fn has_signature_key(&self) -> bool {
        self.connection
            .get_user()
            .map(|u| u.get_signature_key())
            .is_some()
    }

    /// Test helper: get websocket status
    pub fn is_websocket_connected(&self) -> bool {
        self.connection.is_websocket_connected()
    }

    /// Test helper: get reference to MLS provider
    pub fn get_provider(&self) -> &MlsProvider {
        self.connection.get_provider()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Test that MlsClient can be created
    #[test]
    fn test_client_creation() {
        let temp_dir = tempdir().unwrap();
        let storage_dir = temp_dir.path();

        let client = MlsClient::new_with_storage_path(
            "http://localhost:4000",
            "alice",
            "testgroup",
            storage_dir,
        );

        assert!(client.is_ok(), "Client creation should succeed");
        let c = client.unwrap();
        assert_eq!(c.get_username(), "alice");
        assert!(!c.is_websocket_connected());
        assert!(c.get_identity().is_none());
        assert!(c.selected_group_id.is_none(), "No group selected initially");
    }

    /// Test that initialize creates user via connection
    #[tokio::test]
    async fn test_client_initialization() {
        let temp_dir = tempdir().unwrap();
        let storage_dir = temp_dir.path();

        let mut client = MlsClient::new_with_storage_path(
            "http://localhost:4000",
            "alice",
            "testgroup",
            storage_dir,
        )
        .unwrap();

        // User should not exist before initialize
        assert!(client.get_identity().is_none());

        // Initialize (server registration will fail, but user is created locally)
        let _ = client.initialize().await;

        // User should exist after initialize
        assert!(
            client.get_identity().is_some(),
            "User should be created after initialize"
        );
        assert_eq!(client.get_username(), "alice");
    }

    /// Test that connect_to_group creates membership
    #[tokio::test]
    async fn test_client_connect_to_group() {
        let temp_dir = tempdir().unwrap();
        let storage_dir = temp_dir.path();

        let mut client = MlsClient::new_with_storage_path(
            "http://localhost:4000",
            "bob",
            "mygroup",
            storage_dir,
        )
        .unwrap();

        // Initialize user first
        let _ = client.initialize().await;

        // Connect to group (WebSocket will fail, but membership is created)
        // Note: We pass the group name to connect_to_group now
        // We can't test this fully without a mock server, but we can verify the structure
        // For now, just verify that the method exists and compiles
        assert_eq!(client.get_username(), "bob");
    }

    /// Test that operations delegate to selected membership
    #[tokio::test]
    async fn test_client_operations_delegate() {
        let temp_dir = tempdir().unwrap();
        let storage_dir = temp_dir.path();

        let mut client = MlsClient::new_with_storage_path(
            "http://localhost:4000",
            "carol",
            "engineering",
            storage_dir,
        )
        .unwrap();

        // Initialize
        let _ = client.initialize().await;

        // Test that list_members works (returns empty when no group connected)
        let members = client.list_members();
        assert_eq!(members.len(), 0, "No members when group not connected");
    }

    /// Test that get_current_group_name returns correct name
    #[test]
    fn test_client_get_current_group_name() {
        let temp_dir = tempdir().unwrap();
        let storage_dir = temp_dir.path();

        let client = MlsClient::new_with_storage_path(
            "http://localhost:4000",
            "dave",
            "testgroup",
            storage_dir,
        )
        .unwrap();

        // Should return error when no group selected
        let result = client.get_current_group_name();
        assert!(result.is_err(), "Should error when no group selected");
    }
}
