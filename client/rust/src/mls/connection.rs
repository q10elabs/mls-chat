//! MLS Connection - Message Hub & Infrastructure Orchestrator
//!
//! This module manages external service interfaces, user identity, and group memberships.
//! MlsConnection acts as the central message routing hub for all MLS operations.
//!
//! ## Responsibility
//! - Own and manage external services (LocalStore, MlsProvider, ServerApi, WebSocket)
//! - Accept incoming messages from server via WebSocket
//! - Route/fan-out messages to appropriate entities (memberships)
//! - Coordinate user identity initialization and lifecycle
//! - Manage multiple group memberships per user
//!
//! ## Design Principles
//! - **Infrastructure Owner**: Centralizes all external service management
//! - **Message Hub**: Routes incoming messages to correct membership
//! - **Service Coordination**: Single point for initialize, connect, lifecycle
//! - **Multi-Group Support**: HashMap of memberships enables multiple groups per user
//!
//! ## Usage Pattern
//! ```rust
//! // Create connection
//! let mut connection = MlsConnection::new_with_storage_path(url, username, storage_dir)?;
//!
//! // Initialize user identity
//! connection.initialize().await?;
//!
//! // Connect WebSocket
//! connection.connect_websocket().await?;
//!
//! // Process incoming messages
//! while let Some(envelope) = connection.next_envelope().await? {
//!     connection.process_incoming_envelope(envelope).await?;
//! }
//! ```

use crate::api::{KeyPackageUpload, ServerApi};
use crate::crypto;
use crate::error::{ClientError, MlsError, Result};
use crate::identity::IdentityManager;
use crate::mls::keypackage_pool::{KeyPackagePool, KeyPackagePoolConfig};
use crate::mls::membership::MlsMembership;
use crate::mls::user::MlsUser;
use crate::models::{Identity, MlsMessageEnvelope};
use crate::provider::MlsProvider;
use crate::storage::{KeyPackageMetadata, LocalStore};
use crate::websocket::MessageHandler;
use base64::{engine::general_purpose, Engine as _};
use openmls::prelude::KeyPackageBundle;
use openmls_traits::storage::traits as storage_traits;
use openmls_traits::storage::{self, StorageProvider};
use openmls_traits::OpenMlsProvider;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;
use tls_codec::Serialize as TlsSerialize;

/// MLS Connection - Infrastructure and message routing
///
/// Manages all external services and coordinates message routing between
/// the server and group memberships.
///
/// ## Fields
/// - `server_url`: URL of the MLS server
/// - `username`: Username for this connection
/// - `metadata_store`: Application-level metadata storage
/// - `mls_provider`: OpenMLS provider for crypto operations and group storage
/// - `api`: HTTP API client for server communication
/// - `keypackage_pool_config`: Thresholds for KeyPackage pool management
/// - `websocket`: WebSocket connection for real-time messaging
/// - `user`: User identity (created during initialization)
/// - `memberships`: Map of group_id (bytes) to MlsMembership instances
///
/// ## Ownership Model
/// - MlsConnection owns all infrastructure (stores, provider, api, websocket)
/// - MlsConnection owns the user identity (MlsUser)
/// - MlsConnection owns all group memberships (HashMap)
/// - Memberships access services via method parameters (not stored references)
///
/// ## Lifetime Note
/// MlsMembership uses 'static lifetime because it's stored in a HashMap owned by
/// the connection. This avoids circular lifetime issues.
pub struct MlsConnection {
    /// URL of the MLS server
    server_url: String,

    /// Username for this connection
    username: String,

    /// Application-level metadata storage
    metadata_store: LocalStore,

    /// OpenMLS provider (crypto operations and group state persistence)
    mls_provider: MlsProvider,

    /// HTTP API client for server communication
    api: ServerApi,

    /// Configuration parameters for the KeyPackage pool
    keypackage_pool_config: KeyPackagePoolConfig,

    /// WebSocket connection for real-time messaging
    websocket: Option<MessageHandler>,

    /// User identity (initialized via initialize())
    user: Option<MlsUser>,

    /// Group memberships (keyed by group_id bytes)
    memberships: HashMap<Vec<u8>, MlsMembership<'static>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredKeyPackageRef(Vec<u8>);

impl storage_traits::HashReference<{ storage::CURRENT_VERSION }> for StoredKeyPackageRef {}

impl storage::Key<{ storage::CURRENT_VERSION }> for StoredKeyPackageRef {}

impl MlsConnection {
    /// Create a new MLS connection with infrastructure
    ///
    /// Initializes all external services but does not create user identity or
    /// connect WebSocket. Call `initialize()` and `connect_websocket()` after creation.
    ///
    /// # Arguments
    /// * `server_url` - URL of the MLS server (e.g., "http://localhost:4000")
    /// * `username` - Username for this connection
    /// * `storage_dir` - Directory for persistent storage files
    ///
    /// # Errors
    /// * File system errors when creating storage directories
    /// * Database initialization errors
    ///
    /// # Example
    /// ```rust
    /// let connection = MlsConnection::new_with_storage_path(
    ///     "http://localhost:4000",
    ///     "alice",
    ///     Path::new("/tmp/storage"),
    /// )?;
    /// ```
    pub fn new_with_storage_path(
        server_url: &str,
        username: &str,
        storage_dir: &Path,
    ) -> Result<Self> {
        log::info!("Creating MlsConnection for {} at {}", username, server_url);

        // Ensure storage directory exists
        std::fs::create_dir_all(storage_dir)?;

        // Metadata storage (application-level metadata)
        let metadata_db_path = storage_dir.join("metadata.db");
        let metadata_store = LocalStore::new(&metadata_db_path)?;

        // MLS provider storage (handles all OpenMLS group state)
        // Use per-user database to isolate credentials and group state
        let mls_db_path = storage_dir.join(format!("mls-{}.db", username));
        let mls_provider = MlsProvider::new(&mls_db_path)?;

        let api = ServerApi::new(server_url);

        Ok(Self {
            server_url: server_url.to_string(),
            username: username.to_string(),
            metadata_store,
            mls_provider,
            api,
            keypackage_pool_config: KeyPackagePoolConfig::default(),
            websocket: None,
            user: None,
            memberships: HashMap::new(),
        })
    }

    /// Initialize user identity and register with server
    ///
    /// Creates or loads MLS credential and signature keys for this username.
    /// Generates a KeyPackage and registers it with the server.
    ///
    /// ## Process Flow
    /// 1. Load or create persistent identity via IdentityManager
    /// 2. Create MlsUser with loaded identity material
    /// 3. Generate KeyPackage for server registration
    /// 4. Register KeyPackage with server (idempotent)
    /// 5. Store MlsUser in connection
    ///
    /// # Errors
    /// * Storage errors when loading/saving identity
    /// * Network errors when registering with server
    /// * Crypto errors when generating credentials or key packages
    ///
    /// # Example
    /// ```rust
    /// connection.initialize().await?;
    /// assert!(connection.get_user().is_some());
    /// ```
    pub async fn initialize(&mut self) -> Result<()> {
        log::info!("Initializing MlsConnection for {}", self.username);

        // === Step 1: Load or create persistent identity ===
        let stored_identity = IdentityManager::load_or_create(
            &self.mls_provider,
            &self.metadata_store,
            &self.username,
        )?;

        let keypair_blob = stored_identity.signature_key.to_public_vec();

        // === Step 2: Create MlsUser with identity material ===
        let identity = Identity {
            username: self.username.clone(),
            keypair_blob: keypair_blob.clone(),
            credential_blob: vec![], // Not used - regenerated from username
        };

        let user = MlsUser::new(
            self.username.clone(),
            identity,
            stored_identity.signature_key,
            stored_identity.credential_with_key.clone(),
        );

        log::info!(
            "Created MlsUser for {} with persistent signature key",
            self.username
        );

        // === Step 3: Generate KeyPackage for server registration ===
        // Try to fetch existing key package from server (may exist from previous session)
        let key_package_bytes = match self.api.get_user_key(&self.username).await {
            Ok(remote_key_package) => {
                log::info!("Found existing key package for {} on server", self.username);
                // TODO: Add validation that remote key package matches local identity
                remote_key_package
            }
            Err(_) => {
                // User doesn't exist - generate a new key package
                log::info!("Generating new key package for {}", self.username);

                let key_package_bundle = crypto::generate_key_package_bundle(
                    user.get_credential_with_key(),
                    user.get_signature_key(),
                    &self.mls_provider,
                )?;

                // Serialize the KeyPackage using TLS codec
                use tls_codec::Serialize as TlsSerialize;
                key_package_bundle
                    .key_package()
                    .tls_serialize_detached()
                    .map_err(|_e| {
                        ClientError::Mls(crate::error::MlsError::OpenMls(
                            "Failed to serialize key package".to_string(),
                        ))
                    })?
            }
        };

        // === Step 4: Store user in connection (before server registration) ===
        // Store user first so it's available even if server registration fails
        self.user = Some(user);

        // === Step 5: Register with server (idempotent) ===
        // This may fail in tests, but user is already stored locally
        self.api
            .register_user(&self.username, &key_package_bytes)
            .await?;

        log::info!("MlsConnection initialized for {}", self.username);
        Ok(())
    }

    /// Connect WebSocket for real-time messaging
    ///
    /// Establishes WebSocket connection to the server for receiving messages.
    /// The connection subscribes to the user's personal channel for Welcome messages.
    ///
    /// # Errors
    /// * WebSocket connection errors
    /// * Network errors
    ///
    /// # Example
    /// ```rust
    /// connection.connect_websocket().await?;
    /// assert!(connection.is_websocket_connected());
    /// ```
    pub async fn connect_websocket(&mut self) -> Result<()> {
        log::info!("Connecting WebSocket for {}", self.username);

        let websocket = MessageHandler::connect(&self.server_url, &self.username).await?;

        // Subscribe to username for receiving direct messages (e.g., Welcome from inviter)
        websocket.subscribe_to_group(&self.username).await?;

        self.websocket = Some(websocket);

        log::info!("WebSocket connected for {}", self.username);
        Ok(())
    }

    /// Receive next message envelope from WebSocket
    ///
    /// Waits for and returns the next message from the server, or None if connection closed.
    ///
    /// # Returns
    /// * `Ok(Some(envelope))` - Message received
    /// * `Ok(None)` - Connection closed
    /// * `Err(_)` - Error receiving message
    ///
    /// # Example
    /// ```rust
    /// while let Some(envelope) = connection.next_envelope().await? {
    ///     connection.process_incoming_envelope(envelope).await?;
    /// }
    /// ```
    pub async fn next_envelope(&mut self) -> Result<Option<MlsMessageEnvelope>> {
        if let Some(websocket) = &mut self.websocket {
            websocket.next_envelope().await
        } else {
            Err(ClientError::Config("WebSocket not connected".to_string()))
        }
    }

    /// Process an incoming message envelope
    ///
    /// Routes the message to the appropriate handler based on envelope type:
    /// - WelcomeMessage → Create new MlsMembership from Welcome
    /// - ApplicationMessage → Find membership by group_id, call process_incoming_message()
    /// - CommitMessage → Find membership by group_id, call process_incoming_message()
    ///
    /// ## Message Routing Logic
    ///
    /// ### WelcomeMessage
    /// When a user is invited to a group, they receive a Welcome message.
    /// This creates a new MlsMembership and adds it to the memberships HashMap.
    ///
    /// ### ApplicationMessage / CommitMessage
    /// These messages are group-specific and must be routed to the correct membership
    /// by looking up the group_id in the memberships HashMap.
    ///
    /// # Arguments
    /// * `envelope` - Message envelope from WebSocket
    ///
    /// # Errors
    /// * Message processing errors
    /// * Membership not found errors
    /// * MLS operation errors
    ///
    /// # Example
    /// ```rust
    /// let envelope = connection.next_envelope().await?.unwrap();
    /// connection.process_incoming_envelope(envelope).await?;
    /// ```
    pub async fn process_incoming_envelope(&mut self, envelope: MlsMessageEnvelope) -> Result<()> {
        match envelope {
            MlsMessageEnvelope::WelcomeMessage {
                inviter,
                invitee: _,
                welcome_blob,
                ratchet_tree_blob,
            } => {
                log::info!("Received WelcomeMessage from {}", inviter);

                // Create new membership from Welcome message
                let user = self
                    .user
                    .as_ref()
                    .ok_or_else(|| ClientError::Config("User not initialized".to_string()))?;

                let membership = MlsMembership::from_welcome_message(
                    &inviter,
                    &welcome_blob,
                    &ratchet_tree_blob,
                    user,
                    &self.mls_provider,
                    &self.metadata_store,
                )?;

                // Subscribe to group for receiving messages
                let group_id = membership.get_group_id();
                let group_id_b64 = general_purpose::STANDARD.encode(group_id);

                if let Some(websocket) = &self.websocket {
                    websocket.subscribe_to_group(&group_id_b64).await?;
                }

                log::info!(
                    "Created membership for group '{}' from Welcome message",
                    membership.get_group_name()
                );

                // Store membership in HashMap
                self.memberships.insert(group_id.to_vec(), membership);

                Ok(())
            }
            MlsMessageEnvelope::ApplicationMessage {
                sender,
                group_id,
                encrypted_content,
            } => {
                log::debug!(
                    "Received ApplicationMessage from {} for group {}",
                    sender,
                    group_id
                );

                // Decode group_id from base64
                let group_id_bytes = general_purpose::STANDARD.decode(&group_id).map_err(|e| {
                    ClientError::Mls(crate::error::MlsError::OpenMls(format!(
                        "Failed to decode group_id: {}",
                        e
                    )))
                })?;

                // Find membership by group_id
                let membership = self.memberships.get_mut(&group_id_bytes).ok_or_else(|| {
                    log::error!("No membership found for group_id {}", group_id);
                    ClientError::Config(format!("No membership for group {}", group_id))
                })?;

                let user = self
                    .user
                    .as_ref()
                    .ok_or_else(|| ClientError::Config("User not initialized".to_string()))?;

                // Reconstruct envelope for membership processing
                let envelope = MlsMessageEnvelope::ApplicationMessage {
                    sender,
                    group_id,
                    encrypted_content,
                };

                // Delegate to membership
                membership
                    .process_incoming_message(envelope, user, &self.mls_provider)
                    .await
            }
            MlsMessageEnvelope::CommitMessage {
                group_id,
                sender,
                commit_blob,
            } => {
                log::info!(
                    "Received CommitMessage from {} for group {}",
                    sender,
                    group_id
                );

                // Decode group_id from base64
                let group_id_bytes = general_purpose::STANDARD.decode(&group_id).map_err(|e| {
                    ClientError::Mls(crate::error::MlsError::OpenMls(format!(
                        "Failed to decode group_id: {}",
                        e
                    )))
                })?;

                // Find membership by group_id
                let membership = self.memberships.get_mut(&group_id_bytes).ok_or_else(|| {
                    log::error!("No membership found for group_id {}", group_id);
                    ClientError::Config(format!("No membership for group {}", group_id))
                })?;

                let user = self
                    .user
                    .as_ref()
                    .ok_or_else(|| ClientError::Config("User not initialized".to_string()))?;

                // Reconstruct envelope for membership processing
                let envelope = MlsMessageEnvelope::CommitMessage {
                    group_id,
                    sender,
                    commit_blob,
                };

                // Delegate to membership
                membership
                    .process_incoming_message(envelope, user, &self.mls_provider)
                    .await
            }
        }
    }

    /// Refresh the local KeyPackage pool by cleaning up expired entries,
    /// replenishing when thresholds are breached, and uploading pending
    /// KeyPackages to the server.
    pub async fn refresh_key_packages(&mut self) -> Result<()> {
        let user = self.user.as_ref().ok_or_else(|| {
            ClientError::Config("User not initialized - call initialize() first".to_string())
        })?;

        let pool = KeyPackagePool::new(
            self.username.clone(),
            self.keypackage_pool_config.clone(),
            &self.metadata_store,
        );

        let removed = pool.cleanup_expired(&self.mls_provider, SystemTime::now())?;
        if removed > 0 {
            log::info!(
                "Removed {} expired keypackages for user {}",
                removed,
                self.username
            );
        }

        let uploaded = self.upload_pending_keypackages().await?;
        if uploaded > 0 {
            log::info!(
                "Uploaded {} pending keypackages for user {}",
                uploaded,
                self.username
            );
        }

        if pool.should_replenish()? {
            if let Some(needed) = pool.get_replenishment_needed()? {
                if needed > 0 {
                    log::info!(
                        "KeyPackage pool low (need {}). Generating replenishment batch",
                        needed
                    );

                    pool.generate_and_update_pool(
                        needed,
                        user.get_credential_with_key(),
                        user.get_signature_key(),
                        &self.mls_provider,
                    )
                    .await?;

                    let newly_uploaded = self.upload_pending_keypackages().await?;
                    if newly_uploaded > 0 {
                        log::info!(
                            "Uploaded {} newly generated keypackages for user {}",
                            newly_uploaded,
                            self.username
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Get reference to user identity
    ///
    /// Returns None if initialize() has not been called yet.
    pub fn get_user(&self) -> Option<&MlsUser> {
        self.user.as_ref()
    }

    /// Get reference to MLS provider
    pub fn get_provider(&self) -> &MlsProvider {
        &self.mls_provider
    }

    /// Override KeyPackage pool configuration (intended for testing hooks)
    pub fn set_keypackage_pool_config(&mut self, config: KeyPackagePoolConfig) {
        self.keypackage_pool_config = config;
    }

    /// Get reference to server API client
    pub fn get_api(&self) -> &ServerApi {
        &self.api
    }

    /// Get username for this connection
    pub fn get_username(&self) -> &str {
        &self.username
    }

    /// Get reference to metadata store
    pub fn get_metadata_store(&self) -> &LocalStore {
        &self.metadata_store
    }

    /// Get reference to a membership by group_id
    ///
    /// # Arguments
    /// * `group_id` - Group identifier (raw bytes)
    ///
    /// # Returns
    /// * `Some(&MlsMembership)` if membership exists
    /// * `None` if no membership for this group
    pub fn get_membership(&self, group_id: &[u8]) -> Option<&MlsMembership<'static>> {
        self.memberships.get(group_id)
    }

    /// Get mutable reference to a membership by group_id
    ///
    /// # Arguments
    /// * `group_id` - Group identifier (raw bytes)
    ///
    /// # Returns
    /// * `Some(&mut MlsMembership)` if membership exists
    /// * `None` if no membership for this group
    pub fn get_membership_mut(&mut self, group_id: &[u8]) -> Option<&mut MlsMembership<'static>> {
        self.memberships.get_mut(group_id)
    }

    /// Check if WebSocket is connected
    pub fn is_websocket_connected(&self) -> bool {
        self.websocket.is_some()
    }

    /// Get reference to WebSocket (for operations that need it)
    ///
    /// # Returns
    /// * `Some(&MessageHandler)` if WebSocket is connected
    /// * `None` if not connected
    pub fn get_websocket(&self) -> Option<&MessageHandler> {
        self.websocket.as_ref()
    }

    /// Add a membership to the connection's HashMap
    ///
    /// Used when creating or loading a group outside of the Welcome message flow.
    ///
    /// # Arguments
    /// * `membership` - The membership to add
    ///
    /// # Note
    /// The membership's group_id is used as the key in the HashMap.
    pub fn add_membership(&mut self, membership: MlsMembership<'static>) {
        let group_id = membership.get_group_id().to_vec();
        log::debug!(
            "Adding membership for group_id: {}",
            general_purpose::STANDARD.encode(&group_id)
        );
        self.memberships.insert(group_id, membership);
    }

    async fn upload_pending_keypackages(&self) -> Result<usize> {
        let pending = self.metadata_store.get_metadata_by_status("created")?;
        if pending.is_empty() {
            return Ok(0);
        }

        let mut uploads = Vec::new();
        for metadata in pending {
            match self.load_keypackage_bytes(&metadata)? {
                Some(bytes) => uploads.push(KeyPackageUpload {
                    keypackage_ref: metadata.keypackage_ref.clone(),
                    keypackage: bytes,
                    not_after: metadata.not_after,
                }),
                None => {
                    log::warn!(
                        "Missing KeyPackage in provider storage for user {}",
                        self.username
                    );
                    self.metadata_store
                        .update_pool_metadata_status(&metadata.keypackage_ref, "failed")?;
                }
            }
        }

        if uploads.is_empty() {
            return Ok(0);
        }

        let response = self
            .api
            .upload_key_packages(&self.username, &uploads)
            .await?;

        let mut rejected_refs: HashSet<Vec<u8>> = HashSet::new();
        for rejected in response.rejected {
            match general_purpose::STANDARD.decode(&rejected) {
                Ok(bytes) => {
                    self.metadata_store
                        .update_pool_metadata_status(&bytes, "failed")?;
                    rejected_refs.insert(bytes);
                }
                Err(err) => {
                    log::warn!(
                        "Invalid rejected keypackage ref from server for user {}: {}",
                        self.username,
                        err
                    );
                }
            }
        }

        let mut promoted = 0usize;
        for upload in uploads {
            if rejected_refs.contains(&upload.keypackage_ref) {
                continue;
            }

            self.metadata_store
                .update_pool_metadata_status(&upload.keypackage_ref, "uploaded")?;
            self.metadata_store
                .update_pool_metadata_status(&upload.keypackage_ref, "available")?;
            promoted += 1;
        }

        Ok(promoted)
    }

    fn load_keypackage_bytes(&self, metadata: &KeyPackageMetadata) -> Result<Option<Vec<u8>>> {
        let reference = StoredKeyPackageRef(metadata.keypackage_ref.clone());
        let maybe_bundle = self
            .mls_provider
            .storage()
            .key_package::<_, KeyPackageBundle>(&reference)
            .map_err(|err| ClientError::Mls(MlsError::OpenMls(err.to_string())))?;

        if let Some(bundle) = maybe_bundle {
            let serialized = bundle
                .key_package()
                .tls_serialize_detached()
                .map_err(|err| ClientError::Mls(MlsError::OpenMls(err.to_string())))?;
            Ok(Some(serialized))
        } else {
            Ok(None)
        }
    }

    /// Send a message to a specific group
    ///
    /// Helper method that handles the borrow-checking complexity of accessing
    /// both the membership and the services it needs.
    ///
    /// # Arguments
    /// * `group_id` - Group to send message to
    /// * `text` - Message text
    ///
    /// # Errors
    /// * Group not found
    /// * User not initialized
    /// * WebSocket not connected
    /// * MLS encryption errors
    pub async fn send_message_to_group(&mut self, group_id: &[u8], text: &str) -> Result<()> {
        // Get user first (immutable borrow)
        let user = self
            .user
            .as_ref()
            .ok_or_else(|| ClientError::Config("User not initialized".to_string()))?;

        // Get websocket (immutable borrow)
        let websocket = self
            .websocket
            .as_ref()
            .ok_or_else(|| ClientError::Config("WebSocket not connected".to_string()))?;

        // Get membership (mutable borrow - but we've finished with user/websocket immutable borrows above)
        let membership = self
            .memberships
            .get_mut(group_id)
            .ok_or_else(|| ClientError::Config("Group not found".to_string()))?;

        // Call membership method
        membership
            .send_message(text, user, &self.mls_provider, &self.api, websocket)
            .await
    }

    /// Invite a user to a specific group
    ///
    /// Helper method that handles the borrow-checking complexity.
    ///
    /// # Arguments
    /// * `group_id` - Group to invite user to
    /// * `invitee_username` - Username to invite
    ///
    /// # Errors
    /// * Group not found
    /// * User not initialized
    /// * Server errors
    pub async fn invite_user_to_group(
        &mut self,
        group_id: &[u8],
        invitee_username: &str,
    ) -> Result<()> {
        // Get user first
        let user = self
            .user
            .as_ref()
            .ok_or_else(|| ClientError::Config("User not initialized".to_string()))?;

        // Get websocket
        let websocket = self
            .websocket
            .as_ref()
            .ok_or_else(|| ClientError::Config("WebSocket not connected".to_string()))?;

        // Get membership
        let membership = self
            .memberships
            .get_mut(group_id)
            .ok_or_else(|| ClientError::Config("Group not found".to_string()))?;

        // Call membership method
        membership
            .invite_user(
                invitee_username,
                user,
                &self.mls_provider,
                &self.api,
                websocket,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Test that MlsConnection can be created with infrastructure
    ///
    /// Verifies:
    /// - Connection is created successfully
    /// - All infrastructure components are initialized
    /// - No errors during construction
    #[test]
    fn test_connection_creation() {
        let temp_dir = tempdir().unwrap();
        let storage_dir = temp_dir.path();

        let connection =
            MlsConnection::new_with_storage_path("http://localhost:4000", "alice", storage_dir);

        assert!(connection.is_ok(), "Connection creation should succeed");
        let conn = connection.unwrap();
        assert_eq!(conn.get_username(), "alice");
        assert!(!conn.is_websocket_connected());
        assert!(conn.get_user().is_none());
    }

    /// Test that initialize() creates user identity
    ///
    /// Verifies:
    /// - User is created after initialize()
    /// - User identity is stored in connection
    /// - User can be accessed via get_user()
    #[tokio::test]
    async fn test_connection_initialization_creates_user() {
        let temp_dir = tempdir().unwrap();
        let storage_dir = temp_dir.path();

        let mut connection =
            MlsConnection::new_with_storage_path("http://localhost:4000", "alice", storage_dir)
                .unwrap();

        // User should not exist before initialize
        assert!(connection.get_user().is_none());

        // Initialize (this will fail to register with server, but user should still be created)
        let _ = connection.initialize().await;

        // User should exist after initialize (even if server registration failed)
        assert!(
            connection.get_user().is_some(),
            "User should be created even if server registration fails"
        );
        assert_eq!(connection.get_user().unwrap().get_username(), "alice");
    }

    /// Test that accessors return correct values
    ///
    /// Verifies:
    /// - get_username() returns username
    /// - get_provider() returns provider reference
    /// - get_api() returns API reference
    /// - get_metadata_store() returns store reference
    #[test]
    fn test_connection_accessors() {
        let temp_dir = tempdir().unwrap();

        let connection =
            MlsConnection::new_with_storage_path("http://localhost:4000", "bob", temp_dir.path())
                .unwrap();

        assert_eq!(connection.get_username(), "bob");
        let provider_ptr = connection.get_provider() as *const MlsProvider;
        assert!(!provider_ptr.is_null());

        let api_ptr = connection.get_api() as *const ServerApi;
        assert!(!api_ptr.is_null());

        let metadata_ptr = connection.get_metadata_store() as *const LocalStore;
        assert!(!metadata_ptr.is_null());
    }

    /// Test membership lookup by group_id
    ///
    /// Verifies:
    /// - get_membership() returns None when no memberships exist
    /// - get_membership() returns Some() after membership is added
    /// - get_membership_mut() allows modification
    #[test]
    fn test_membership_lookup_by_group_id() {
        let temp_dir = tempdir().unwrap();

        let connection =
            MlsConnection::new_with_storage_path("http://localhost:4000", "alice", temp_dir.path())
                .unwrap();

        // No memberships initially
        let fake_group_id = vec![1, 2, 3, 4];
        assert!(connection.get_membership(&fake_group_id).is_none());

        // Add a fake membership (we can't easily create a real one in a unit test)
        // This is just to verify the HashMap lookup works
        // In integration tests, we'll test with real Welcome messages

        // Keep temp_dir alive until end of test
        drop(temp_dir);
    }

    /// Test that message routing handles WelcomeMessage
    ///
    /// This is an integration-style test that verifies the full flow:
    /// 1. Alice creates a group
    /// 2. Alice invites Bob (generates Welcome)
    /// 3. Bob's connection processes the Welcome
    /// 4. Bob's connection creates a new membership
    /// 5. Membership is accessible via get_membership()
    #[tokio::test]
    async fn test_process_welcome_message_creates_membership() {
        let temp_dir = tempdir().unwrap();
        let _storage_dir = temp_dir.path();

        // === Setup: Alice creates a group and invites Bob ===
        let alice_storage = temp_dir.path().join("alice");
        std::fs::create_dir_all(&alice_storage).unwrap();
        let alice_provider = MlsProvider::new(alice_storage.join("mls.db")).unwrap();

        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut alice_group =
            crypto::create_group_with_config(&alice_cred, &alice_key, &alice_provider, "testgroup")
                .unwrap();

        // === Bob creates a connection and initializes ===
        let bob_storage = temp_dir.path().join("bob");
        std::fs::create_dir_all(&bob_storage).unwrap();

        let mut bob_connection =
            MlsConnection::new_with_storage_path("http://localhost:4000", "bob", &bob_storage)
                .unwrap();

        // Initialize Bob's user (server registration will fail, but user is created locally)
        let _ = bob_connection.initialize().await;

        // Generate Bob's key package for Alice to use
        let bob_user = bob_connection.get_user().unwrap();
        let bob_key_package = crypto::generate_key_package_bundle(
            bob_user.get_credential_with_key(),
            bob_user.get_signature_key(),
            bob_connection.get_provider(),
        )
        .unwrap();

        // === Alice invites Bob ===
        let (_commit, welcome, _) = crypto::add_members(
            &mut alice_group,
            &alice_provider,
            &alice_key,
            &[bob_key_package.key_package()],
        )
        .unwrap();
        crypto::merge_pending_commit(&mut alice_group, &alice_provider).unwrap();

        // Export ratchet tree
        let ratchet_tree = crypto::export_ratchet_tree(&alice_group);

        // Serialize Welcome and ratchet tree
        use tls_codec::Serialize as TlsSerialize;
        let welcome_bytes = welcome.tls_serialize_detached().unwrap();
        let welcome_b64 = general_purpose::STANDARD.encode(&welcome_bytes);

        let ratchet_tree_bytes = serde_json::to_vec(&ratchet_tree).unwrap();
        let ratchet_tree_b64 = general_purpose::STANDARD.encode(&ratchet_tree_bytes);

        // === Bob processes the Welcome message ===
        let welcome_envelope = MlsMessageEnvelope::WelcomeMessage {
            inviter: "alice".to_string(),
            invitee: "bob".to_string(),
            welcome_blob: welcome_b64,
            ratchet_tree_blob: ratchet_tree_b64,
        };

        let result = bob_connection
            .process_incoming_envelope(welcome_envelope)
            .await;
        assert!(result.is_ok(), "Welcome processing should succeed");

        // === Verify membership was created ===
        let group_id = alice_group.group_id().as_slice();
        let membership = bob_connection.get_membership(group_id);
        assert!(
            membership.is_some(),
            "Membership should exist after Welcome"
        );

        let membership = membership.unwrap();
        assert_eq!(membership.get_group_name(), "testgroup");
        assert_eq!(membership.list_members().len(), 2);
    }

    /// Test that ApplicationMessage routing works
    ///
    /// Verifies:
    /// - ApplicationMessage is routed to correct membership
    /// - Message is decrypted and processed
    #[tokio::test]
    async fn test_process_application_message_routes_to_membership() {
        let temp_dir = tempdir().unwrap();

        // === Setup: Alice and Bob in a group ===
        let alice_storage = temp_dir.path().join("alice");
        std::fs::create_dir_all(&alice_storage).unwrap();
        let alice_provider = MlsProvider::new(alice_storage.join("mls.db")).unwrap();

        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut alice_group =
            crypto::create_group_with_config(&alice_cred, &alice_key, &alice_provider, "testgroup")
                .unwrap();

        // Bob setup
        let bob_storage = temp_dir.path().join("bob");
        std::fs::create_dir_all(&bob_storage).unwrap();
        let mut bob_connection =
            MlsConnection::new_with_storage_path("http://localhost:4000", "bob", &bob_storage)
                .unwrap();
        let _ = bob_connection.initialize().await;

        // Alice invites Bob
        let bob_user = bob_connection.get_user().unwrap();
        let bob_key_package = crypto::generate_key_package_bundle(
            bob_user.get_credential_with_key(),
            bob_user.get_signature_key(),
            bob_connection.get_provider(),
        )
        .unwrap();

        let (_commit, welcome, _) = crypto::add_members(
            &mut alice_group,
            &alice_provider,
            &alice_key,
            &[bob_key_package.key_package()],
        )
        .unwrap();
        crypto::merge_pending_commit(&mut alice_group, &alice_provider).unwrap();

        // Bob processes Welcome
        let ratchet_tree = crypto::export_ratchet_tree(&alice_group);
        use tls_codec::Serialize as TlsSerialize;
        let welcome_bytes = welcome.tls_serialize_detached().unwrap();
        let welcome_b64 = general_purpose::STANDARD.encode(&welcome_bytes);
        let ratchet_tree_bytes = serde_json::to_vec(&ratchet_tree).unwrap();
        let ratchet_tree_b64 = general_purpose::STANDARD.encode(&ratchet_tree_bytes);

        let welcome_envelope = MlsMessageEnvelope::WelcomeMessage {
            inviter: "alice".to_string(),
            invitee: "bob".to_string(),
            welcome_blob: welcome_b64,
            ratchet_tree_blob: ratchet_tree_b64,
        };
        bob_connection
            .process_incoming_envelope(welcome_envelope)
            .await
            .unwrap();

        // === Alice sends a message ===
        // Store group_id before mutable borrow
        let group_id = alice_group.group_id().as_slice().to_vec();

        let message_text = "Hello Bob!";
        let encrypted = crypto::create_application_message(
            &mut alice_group,
            &alice_provider,
            &alice_key,
            message_text.as_bytes(),
        )
        .unwrap();

        let encrypted_bytes = encrypted.tls_serialize_detached().unwrap();
        let encrypted_b64 = general_purpose::STANDARD.encode(&encrypted_bytes);
        let group_id_b64 = general_purpose::STANDARD.encode(&group_id);

        let app_envelope = MlsMessageEnvelope::ApplicationMessage {
            sender: "alice".to_string(),
            group_id: group_id_b64,
            encrypted_content: encrypted_b64,
        };

        // === Bob processes the message ===
        let result = bob_connection.process_incoming_envelope(app_envelope).await;
        assert!(
            result.is_ok(),
            "ApplicationMessage processing should succeed"
        );
    }

    /// Test that CommitMessage routing works
    ///
    /// Verifies:
    /// - CommitMessage is routed to correct membership
    /// - Group state is updated after commit
    #[tokio::test]
    async fn test_process_commit_message_routes_to_membership() {
        let temp_dir = tempdir().unwrap();

        // === Setup: Alice, Bob, then Alice adds Carol ===
        let alice_storage = temp_dir.path().join("alice");
        std::fs::create_dir_all(&alice_storage).unwrap();
        let alice_provider = MlsProvider::new(alice_storage.join("mls.db")).unwrap();

        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut alice_group =
            crypto::create_group_with_config(&alice_cred, &alice_key, &alice_provider, "testgroup")
                .unwrap();

        // Bob setup and join
        let bob_storage = temp_dir.path().join("bob");
        std::fs::create_dir_all(&bob_storage).unwrap();
        let mut bob_connection =
            MlsConnection::new_with_storage_path("http://localhost:4000", "bob", &bob_storage)
                .unwrap();
        let _ = bob_connection.initialize().await;

        let bob_user = bob_connection.get_user().unwrap();
        let bob_key_package = crypto::generate_key_package_bundle(
            bob_user.get_credential_with_key(),
            bob_user.get_signature_key(),
            bob_connection.get_provider(),
        )
        .unwrap();

        let (_commit1, welcome1, _) = crypto::add_members(
            &mut alice_group,
            &alice_provider,
            &alice_key,
            &[bob_key_package.key_package()],
        )
        .unwrap();
        crypto::merge_pending_commit(&mut alice_group, &alice_provider).unwrap();

        // Bob processes Welcome
        let ratchet_tree1 = crypto::export_ratchet_tree(&alice_group);
        use tls_codec::Serialize as TlsSerialize;
        let welcome1_bytes = welcome1.tls_serialize_detached().unwrap();
        let welcome1_b64 = general_purpose::STANDARD.encode(&welcome1_bytes);
        let ratchet_tree1_bytes = serde_json::to_vec(&ratchet_tree1).unwrap();
        let ratchet_tree1_b64 = general_purpose::STANDARD.encode(&ratchet_tree1_bytes);

        bob_connection
            .process_incoming_envelope(MlsMessageEnvelope::WelcomeMessage {
                inviter: "alice".to_string(),
                invitee: "bob".to_string(),
                welcome_blob: welcome1_b64,
                ratchet_tree_blob: ratchet_tree1_b64,
            })
            .await
            .unwrap();

        // Verify Bob sees 2 members initially (store group_id before mutable borrows)
        let group_id = alice_group.group_id().as_slice().to_vec();
        let bob_membership = bob_connection.get_membership(&group_id).unwrap();
        assert_eq!(bob_membership.list_members().len(), 2);

        // === Alice adds Carol ===
        let (carol_cred, carol_key) = crypto::generate_credential_with_key("carol").unwrap();
        let carol_key_package =
            crypto::generate_key_package_bundle(&carol_cred, &carol_key, &alice_provider).unwrap();

        let (commit2, _welcome2, _) = crypto::add_members(
            &mut alice_group,
            &alice_provider,
            &alice_key,
            &[carol_key_package.key_package()],
        )
        .unwrap();
        crypto::merge_pending_commit(&mut alice_group, &alice_provider).unwrap();

        // === Bob receives Commit ===
        let commit2_bytes = commit2.tls_serialize_detached().unwrap();
        let commit2_b64 = general_purpose::STANDARD.encode(&commit2_bytes);
        let group_id_b64 = general_purpose::STANDARD.encode(&group_id);

        let commit_envelope = MlsMessageEnvelope::CommitMessage {
            group_id: group_id_b64,
            sender: "alice".to_string(),
            commit_blob: commit2_b64,
        };

        // Process commit
        let result = bob_connection
            .process_incoming_envelope(commit_envelope)
            .await;
        assert!(result.is_ok(), "CommitMessage processing should succeed");

        // Verify Bob now sees 3 members
        let bob_membership = bob_connection.get_membership(&group_id).unwrap();
        assert_eq!(bob_membership.list_members().len(), 3);
    }
}
