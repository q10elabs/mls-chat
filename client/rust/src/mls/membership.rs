//! MLS Group Membership Management
//!
//! This module encapsulates group session state and operations for a single MLS group.
//! MlsMembership represents a user's participation in one specific group.
//!
//! ## Responsibility
//! - Owns group-specific state (mls_group, group_id, group_name)
//! - Manages group operations (send message, invite user, list members)
//! - Processes incoming messages (ApplicationMessage, CommitMessage)
//!
//! ## Design Principles
//! - **Group Encapsulation**: All group-specific state in one place
//! - **Independent Lifecycle**: Can join/leave groups independently
//! - **Service Parameterization**: In Phase 2, services passed as parameters (Phase 3 will add connection field)
//! - **Single Group Focus**: Each instance manages exactly one group
//!
//! ## Phase 2 Implementation Note
//! In this phase, methods take services (provider, api, websocket) as parameters.
//! In Phase 3, these will be accessed via a `connection: &'a MlsConnection` field.
//!
//! ## Usage Pattern (Phase 2)
//! ```rust
//! // Created from Welcome message
//! let membership = MlsMembership::from_welcome_message(
//!     inviter,
//!     welcome_blob,
//!     ratchet_tree_blob,
//!     &user,
//!     &provider,
//!     &metadata_store,
//! )?;
//!
//! // Operations require service parameters
//! membership.send_message(text, &user, &provider, &api, &websocket).await?;
//! membership.invite_user(invitee, &user, &provider, &api, &websocket).await?;
//! ```

use crate::api::ServerApi;
use crate::crypto;
use crate::error::{Result, ClientError};
use crate::message_processing::{process_application_message, format_display_message};
use crate::models::MlsMessageEnvelope;
use crate::mls::user::MlsUser;
use crate::provider::MlsProvider;
use crate::storage::LocalStore;
use crate::websocket::MessageHandler;
use base64::{engine::general_purpose, Engine as _};
use openmls::prelude::{GroupId, OpenMlsProvider};
use tls_codec::{Deserialize, Serialize as TlsSerialize};

/// Group membership for a single MLS group
///
/// Represents a user's participation in one specific group. Each MlsMembership
/// instance manages the state and operations for exactly one group.
///
/// ## Fields
/// - `group_name`: Human-readable group name (e.g., "engineering")
/// - `group_id`: MLS group identifier (unique bytes)
/// - `mls_group`: OpenMLS group state (epoch, members, keys)
///
/// ## Ownership Model
/// - MlsMembership owns all group-specific state directly
/// - Created when user joins a group (via Welcome or creation)
/// - Destroyed when user leaves the group
///
/// ## Lifetime Parameter
/// The `'a` lifetime is prepared for Phase 3 when we add:
/// ```rust
/// connection: &'a MlsConnection
/// ```
/// In Phase 2, this lifetime is unused but included for forward compatibility.
pub struct MlsMembership<'a> {
    /// Human-readable group name
    group_name: String,

    /// MLS group identifier (unique bytes)
    group_id: Vec<u8>,

    /// OpenMLS group state (epoch, members, encryption keys)
    mls_group: openmls::prelude::MlsGroup,

    /// Phantom data to use the lifetime parameter in Phase 2
    /// This will be replaced with `connection: &'a MlsConnection` in Phase 3
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> MlsMembership<'a> {
    /// Create a new MlsMembership from a Welcome message
    ///
    /// Called when a user receives an invitation to join an existing group.
    /// Processes the Welcome message and ratchet tree to initialize the group state.
    ///
    /// ## Process Flow
    /// 1. Decode and deserialize Welcome message and ratchet tree
    /// 2. Process Welcome to create joined MlsGroup
    /// 3. Extract group metadata (authoritative group name)
    /// 4. Store group ID mapping for persistence
    /// 5. Return new MlsMembership instance
    ///
    /// # Arguments
    /// * `inviter` - Username of who sent the invitation
    /// * `welcome_blob_b64` - Base64-encoded TLS-serialized Welcome message
    /// * `ratchet_tree_blob_b64` - Base64-encoded ratchet tree
    /// * `user` - User identity joining the group
    /// * `provider` - MLS provider for crypto operations
    /// * `_metadata_store` - Local storage for group metadata (unused in Phase 2, saved by provider)
    ///
    /// # Errors
    /// * Base64 decoding errors
    /// * TLS deserialization errors
    /// * MLS Welcome processing errors
    /// * Missing group metadata in Welcome
    /// * Storage errors when saving group ID mapping
    ///
    /// # Example
    /// ```rust
    /// let membership = MlsMembership::from_welcome_message(
    ///     "alice",
    ///     &welcome_b64,
    ///     &ratchet_tree_b64,
    ///     &user,
    ///     &provider,
    ///     &metadata_store,
    /// )?;
    /// ```
    pub fn from_welcome_message(
        inviter: &str,
        welcome_blob_b64: &str,
        ratchet_tree_blob_b64: &str,
        user: &MlsUser,
        provider: &MlsProvider,
        _metadata_store: &LocalStore,
    ) -> Result<Self> {
        log::info!("Processing Welcome message from {} to join a group", inviter);

        // === Step 1: Decode and deserialize Welcome message ===
        let welcome_bytes = general_purpose::STANDARD
            .decode(welcome_blob_b64)
            .map_err(|e| {
                log::error!("Failed to decode Welcome message from {}: {}", inviter, e);
                ClientError::Mls(crate::error::MlsError::OpenMls(format!("Failed to decode welcome: {}", e)))
            })?;

        let welcome_message_in = openmls::prelude::MlsMessageIn::tls_deserialize(&mut welcome_bytes.as_slice())
            .map_err(|e| {
                log::error!("Failed to deserialize Welcome message from {}: {}", inviter, e);
                ClientError::Mls(crate::error::MlsError::OpenMls(format!("Failed to deserialize welcome: {}", e)))
            })?;

        // === Step 2: Decode and deserialize ratchet tree ===
        let ratchet_tree_bytes = general_purpose::STANDARD
            .decode(ratchet_tree_blob_b64)
            .map_err(|e| {
                log::error!("Failed to decode ratchet tree from {}: {}", inviter, e);
                ClientError::Mls(crate::error::MlsError::OpenMls(format!("Failed to decode ratchet tree: {}", e)))
            })?;

        let ratchet_tree: openmls::prelude::RatchetTreeIn = serde_json::from_slice(&ratchet_tree_bytes)
            .map_err(|e| {
                log::error!("Failed to deserialize ratchet tree from {}: {}", inviter, e);
                ClientError::Mls(crate::error::MlsError::OpenMls(format!("Failed to deserialize ratchet tree: {}", e)))
            })?;

        // === Step 3: Process the Welcome message to create the group ===
        let join_config = openmls::prelude::MlsGroupJoinConfig::default();
        let joined_group = crypto::process_welcome_message(
            provider,
            &join_config,
            &welcome_message_in,
            Some(ratchet_tree),
        )
        .map_err(|e| {
            log::error!("Failed to process Welcome message from {}: {}", inviter, e);
            e
        })?;

        // === Step 4: Extract group name from encrypted metadata ===
        let metadata = crypto::extract_group_metadata(&joined_group)?
            .ok_or_else(|| {
                log::error!("Welcome message missing group metadata - cannot determine group name");
                ClientError::Config("Missing group metadata in Welcome - inviter may be using incompatible client".to_string())
            })?;

        let group_name = metadata.name.clone();
        let group_id = joined_group.group_id().as_slice().to_vec();

        // === Step 5: Store the group ID mapping for persistence ===
        let group_id_key = format!("{}:{}", user.get_username(), &group_name);
        provider.save_group_name(&group_id_key, &group_id)
            .map_err(|e| {
                log::error!("Failed to store group ID mapping for {}: {}", group_name, e);
                e
            })?;

        log::info!("Successfully joined group '{}' via Welcome message from {}", group_name, inviter);

        // === Step 6: Return new MlsMembership instance ===
        Ok(Self {
            group_name,
            group_id,
            mls_group: joined_group,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Connect to an existing group from storage
    ///
    /// Loads a previously created or joined group from persistent storage.
    /// Used for reconnection scenarios when the group already exists locally.
    ///
    /// ## Process Flow
    /// 1. Look up group ID by name in metadata store
    /// 2. Load MlsGroup from provider storage
    /// 3. Return new MlsMembership instance
    ///
    /// # Arguments
    /// * `group_name` - Human-readable group name to load
    /// * `user` - User identity for this group
    /// * `provider` - MLS provider for loading group state
    ///
    /// # Errors
    /// * Group not found in metadata store
    /// * Group not found in provider storage
    /// * Storage access errors
    ///
    /// # Example
    /// ```rust
    /// let membership = MlsMembership::connect_to_existing_group(
    ///     "engineering",
    ///     &user,
    ///     &provider,
    /// )?;
    /// ```
    pub fn connect_to_existing_group(
        group_name: &str,
        user: &MlsUser,
        provider: &MlsProvider,
    ) -> Result<Self> {
        log::info!("Connecting to existing group: {}", group_name);

        // Look up group ID by name
        let group_id_key = format!("{}:{}", user.get_username(), group_name);
        let stored_group_id = provider.load_group_by_name(&group_id_key)?
            .ok_or_else(|| {
                log::error!("Group {} not found in metadata store", group_name);
                ClientError::Config(format!("Group {} not found", group_name))
            })?;

        // Load the group from storage
        let group_id = GroupId::from_slice(&stored_group_id);
        let mls_group = crypto::load_group_from_storage(provider, &group_id)?
            .ok_or_else(|| {
                log::error!("Group {} not found in provider storage", group_name);
                ClientError::Config(format!("Group {} not found in storage", group_name))
            })?;

        log::info!(
            "Loaded existing MLS group: {} (id: {})",
            group_name,
            base64::engine::general_purpose::STANDARD.encode(&stored_group_id)
        );

        Ok(Self {
            group_name: group_name.to_string(),
            group_id: stored_group_id,
            mls_group,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Create a new group
    ///
    /// Creates a new MLS group if it doesn't already exist in the provider's storage.
    /// If group already exists in metadata, loads it instead.
    ///
    /// # Arguments
    /// * `group_name` - Name of the group to create
    /// * `user` - User identity (creator)
    /// * `provider` - MLS provider for storage
    ///
    /// # Returns
    /// * `Ok(Self)` - New or existing membership
    ///
    /// # Errors
    /// * MLS group creation errors
    /// * Storage errors
    pub fn create_new_group(
        group_name: &str,
        user: &MlsUser,
        provider: &MlsProvider,
    ) -> Result<Self> {
        log::info!("Creating or connecting to group: {}", group_name);

        let group_id_key = format!("{}:{}", user.get_username(), group_name);

        // Try to load existing group first
        match provider.load_group_by_name(&group_id_key) {
            Ok(Some(stored_group_id)) => {
                // Group exists in metadata - try to load it
                match crypto::load_group_from_storage(provider, &GroupId::from_slice(&stored_group_id)) {
                    Ok(Some(mls_group)) => {
                        log::info!(
                            "Loaded existing MLS group: {} (id: {})",
                            group_name,
                            general_purpose::STANDARD.encode(&stored_group_id)
                        );
                        return Ok(Self {
                            group_name: group_name.to_string(),
                            group_id: stored_group_id,
                            mls_group,
                            _phantom: std::marker::PhantomData,
                        });
                    }
                    Ok(None) => {
                        // Group ID in metadata but not in storage - data inconsistency
                        // Recreate the group as fallback
                        log::warn!(
                            "Group metadata exists but group not found in storage. Recreating group."
                        );
                    }
                    Err(e) => {
                        // Error loading group from storage - log but continue with creation
                        log::warn!("Error loading group from storage: {}. Creating new group.", e);
                    }
                }
            }
            Ok(None) => {
                // Group doesn't exist - will be created below
                log::debug!("Group {} does not exist in metadata, creating new group.", group_name);
            }
            Err(e) => {
                // Error checking storage - create new group as fallback
                log::warn!("Error checking group mapping: {}. Creating new group.", e);
            }
        }

        // Create new group
        let mls_group = crypto::create_group_with_config(
            user.get_credential_with_key(),
            user.get_signature_key(),
            provider,
            group_name,
        )?;

        let group_id = mls_group.group_id().as_slice().to_vec();

        // Save the group ID mapping for later retrieval
        if let Err(e) = provider.save_group_name(&group_id_key, &group_id) {
            log::warn!(
                "Failed to save group name mapping for {}: {}. Group created but not persisted.",
                group_name,
                e
            );
        }

        log::info!(
            "Created new MLS group: {} (id: {})",
            group_name,
            general_purpose::STANDARD.encode(&group_id)
        );

        Ok(Self {
            group_name: group_name.to_string(),
            group_id,
            mls_group,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Send a message to the group
    ///
    /// Encrypts the message using MLS and sends it via WebSocket.
    ///
    /// # Arguments
    /// * `text` - Message text to send
    /// * `user` - User identity (for signature)
    /// * `provider` - MLS provider for encryption
    /// * `api` - Server API (unused in Phase 2, but included for consistency)
    /// * `websocket` - WebSocket connection for sending
    ///
    /// # Errors
    /// * MLS encryption errors
    /// * WebSocket send errors
    pub async fn send_message(
        &mut self,
        text: &str,
        user: &MlsUser,
        provider: &MlsProvider,
        _api: &ServerApi,
        websocket: &MessageHandler,
    ) -> Result<()> {
        log::debug!("Sending message to group {}", self.group_name);

        // Encrypt the message using the persistent group state
        let encrypted_msg = crypto::create_application_message(
            &mut self.mls_group,
            provider,
            user.get_signature_key(),
            text.as_bytes(),
        )?;

        // Serialize the encrypted MLS message using TLS codec
        use tls_codec::Serialize;
        let encrypted_bytes = encrypted_msg
            .tls_serialize_detached()
            .map_err(|_e| ClientError::Mls(crate::error::MlsError::OpenMls("Failed to serialize message".to_string())))?;

        // Encode for WebSocket transmission
        let encrypted_b64 = general_purpose::STANDARD.encode(&encrypted_bytes);

        // Send via WebSocket with MLS group ID (base64-encoded) for server routing
        let mls_group_id_b64 = general_purpose::STANDARD.encode(&self.group_id);

        let app_envelope = MlsMessageEnvelope::ApplicationMessage {
            sender: user.get_username().to_string(),
            group_id: mls_group_id_b64,
            encrypted_content: encrypted_b64,
        };

        websocket.send_envelope(&app_envelope).await?;

        log::debug!("Message sent successfully to group {}", self.group_name);
        Ok(())
    }

    /// Invite a user to the group
    ///
    /// Implements proper MLS invitation protocol:
    /// 1. Fetches invitee's KeyPackage from server
    /// 2. Adds them to the MLS group
    /// 3. Exports ratchet tree
    /// 4. Sends Welcome message directly to invitee
    /// 5. Broadcasts Commit to existing members
    ///
    /// # Arguments
    /// * `invitee_username` - Username to invite
    /// * `user` - Inviter's identity
    /// * `provider` - MLS provider for group operations
    /// * `api` - Server API for fetching KeyPackage
    /// * `websocket` - WebSocket for sending messages
    ///
    /// # Errors
    /// * Server errors when fetching KeyPackage
    /// * MLS operation errors
    /// * WebSocket send errors
    pub async fn invite_user(
        &mut self,
        invitee_username: &str,
        user: &MlsUser,
        provider: &MlsProvider,
        api: &ServerApi,
        websocket: &MessageHandler,
    ) -> Result<()> {
        log::info!("Inviting {} to group {}", invitee_username, self.group_name);

        // Verify invitee exists by fetching their key package from server
        let invitee_key_package_bytes = match api.get_user_key(invitee_username).await {
            Ok(key) => key,
            Err(e) => {
                log::error!("Failed to fetch KeyPackage for {}: {}", invitee_username, e);
                return Err(e);
            }
        };

        // Deserialize and validate the invitee's KeyPackage
        let invitee_key_package_in = openmls::key_packages::KeyPackageIn::tls_deserialize(&mut &invitee_key_package_bytes[..])
            .map_err(|e| ClientError::Mls(crate::error::MlsError::OpenMls(format!("Failed to deserialize invitee key package: {}", e))))?;

        // Validate KeyPackage
        let invitee_key_package = invitee_key_package_in
            .validate(provider.crypto(), openmls::prelude::ProtocolVersion::Mls10)
            .map_err(|e| ClientError::Mls(crate::error::MlsError::OpenMls(format!("Invalid invitee key package: {}", e))))?;

        // Add the member to the persistent group
        let (commit_message, welcome_message, _group_info) = crypto::add_members(
            &mut self.mls_group,
            provider,
            user.get_signature_key(),
            &[&invitee_key_package],
        )?;

        // Merge the pending commit to update group state
        crypto::merge_pending_commit(&mut self.mls_group, provider)?;

        // Export ratchet tree for the new member to join
        let ratchet_tree = crypto::export_ratchet_tree(&self.mls_group);

        // Send Welcome message directly to the invitee
        let welcome_bytes = welcome_message
            .tls_serialize_detached()
            .map_err(|e| ClientError::Mls(crate::error::MlsError::OpenMls(
                format!("Failed to serialize welcome: {}", e)
            )))?;
        let welcome_b64 = general_purpose::STANDARD.encode(&welcome_bytes);

        let ratchet_tree_bytes = serde_json::to_vec(&ratchet_tree)
            .map_err(|e| ClientError::Mls(crate::error::MlsError::OpenMls(
                format!("Failed to serialize ratchet tree: {}", e)
            )))?;
        let ratchet_tree_b64 = general_purpose::STANDARD.encode(&ratchet_tree_bytes);

        // Create and send Welcome envelope (no group_id - direct to invitee)
        let welcome_envelope = MlsMessageEnvelope::WelcomeMessage {
            inviter: user.get_username().to_string(),
            invitee: invitee_username.to_string(),
            welcome_blob: welcome_b64,
            ratchet_tree_blob: ratchet_tree_b64,
        };

        websocket.send_envelope(&welcome_envelope).await?;
        log::info!("Sent Welcome message to {} (ratchet tree included)", invitee_username);

        // Broadcast Commit to all existing members
        let mls_group_id_b64 = general_purpose::STANDARD.encode(&self.group_id);

        let commit_bytes = commit_message
            .tls_serialize_detached()
            .map_err(|e| ClientError::Mls(crate::error::MlsError::OpenMls(
                format!("Failed to serialize commit: {}", e)
            )))?;
        let commit_b64 = general_purpose::STANDARD.encode(&commit_bytes);

        let commit_envelope = MlsMessageEnvelope::CommitMessage {
            group_id: mls_group_id_b64,
            sender: user.get_username().to_string(),
            commit_blob: commit_b64,
        };

        websocket.send_envelope(&commit_envelope).await?;
        log::info!("Broadcast Commit message to existing members");

        Ok(())
    }

    /// List group members
    ///
    /// Returns the usernames of all current members in the group.
    ///
    /// # Returns
    /// Vector of member usernames (extracted from BasicCredentials)
    pub fn list_members(&self) -> Vec<String> {
        self.mls_group
            .members()
            .filter_map(|member| {
                // Extract username from BasicCredential identity
                match member.credential.credential_type() {
                    openmls::prelude::CredentialType::Basic => {
                        if let Ok(basic_cred) = openmls::prelude::BasicCredential::try_from(member.credential.clone()) {
                            String::from_utf8(basic_cred.identity().to_vec()).ok()
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// Process an incoming message envelope
    ///
    /// Handles both ApplicationMessage and CommitMessage types.
    /// ApplicationMessages are decrypted and displayed.
    /// CommitMessages update the group state (new members, epoch advancement).
    ///
    /// # Arguments
    /// * `envelope` - Message envelope from WebSocket
    /// * `user` - User identity (to skip own messages)
    /// * `provider` - MLS provider for crypto operations
    ///
    /// # Errors
    /// * Message decryption errors
    /// * Commit processing errors
    /// * Invalid message format
    pub async fn process_incoming_message(
        &mut self,
        envelope: MlsMessageEnvelope,
        user: &MlsUser,
        provider: &MlsProvider,
    ) -> Result<()> {
        match envelope {
            MlsMessageEnvelope::ApplicationMessage {
                sender,
                group_id,
                encrypted_content,
            } => {
                // Skip processing our own application messages
                if sender == user.get_username() {
                    log::debug!("Skipping our own application message (ratchet state already advanced on send)");
                    return Ok(());
                }

                // Process the application message
                match process_application_message(
                    &sender,
                    &group_id,
                    &encrypted_content,
                    &mut self.mls_group,
                    provider,
                ).await {
                    Ok(Some(decrypted_text)) => {
                        println!("{}", format_display_message(&self.group_name, &sender, &decrypted_text));
                    }
                    Ok(None) => {
                        log::debug!("Received non-application message in envelope");
                    }
                    Err(e) => {
                        log::error!("Failed to process message: {}", e);
                        println!("{}", format_display_message(&self.group_name, &sender, "[decryption failed]"));
                    }
                }
            }
            MlsMessageEnvelope::CommitMessage {
                group_id: _group_id_b64,
                sender,
                commit_blob,
            } => {
                log::info!("Received Commit from {} for group {}", sender, self.group_name);

                // Skip processing our own Commit messages
                if sender == user.get_username() {
                    log::debug!("Skipping our own Commit message (already merged when sent)");
                    return Ok(());
                }

                // Decode and process the commit
                match general_purpose::STANDARD.decode(&commit_blob) {
                    Ok(commit_bytes) => {
                        match openmls::prelude::MlsMessageIn::tls_deserialize(&mut commit_bytes.as_slice()) {
                            Ok(commit_message_in) => {
                                match crypto::process_message(&mut self.mls_group, provider, &commit_message_in) {
                                    Ok(processed_commit) => {
                                        match processed_commit.into_content() {
                                            openmls::prelude::ProcessedMessageContent::StagedCommitMessage(staged_commit) => {
                                                match self.mls_group.merge_staged_commit(provider, *staged_commit) {
                                                    Ok(()) => {
                                                        let member_count = self.mls_group.members().count();
                                                        log::info!(
                                                            "Merged Commit from {}, group now has {} members",
                                                            sender,
                                                            member_count
                                                        );
                                                        println!(
                                                            "{}",
                                                            format_display_message(&self.group_name, &sender, "[updated group membership]")
                                                        );
                                                    }
                                                    Err(e) => {
                                                        log::error!("Failed to merge Commit: {}", e);
                                                    }
                                                }
                                            }
                                            _ => {
                                                log::debug!("Received non-commit handshake message: ignoring");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("Failed to process Commit: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to deserialize Commit: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to decode Commit: {}", e);
                    }
                }
            }
            MlsMessageEnvelope::WelcomeMessage { .. } => {
                // Welcome messages are not processed by existing memberships
                log::warn!("Received WelcomeMessage in membership.process_incoming_message() - this should be handled by connection");
            }
        }
        Ok(())
    }

    /// Get the group name
    pub fn get_group_name(&self) -> &str {
        &self.group_name
    }

    /// Get the group ID
    pub fn get_group_id(&self) -> &[u8] {
        &self.group_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use tempfile::tempdir;

    /// Test creating MlsMembership from Welcome message
    ///
    /// Verifies:
    /// - Welcome message is processed correctly
    /// - Group metadata is extracted
    /// - MlsMembership is created with correct state
    #[test]
    fn test_membership_from_welcome_message() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();
        let metadata_store = LocalStore::new(&temp_dir.path().join("metadata.db")).unwrap();

        // Alice creates a group (generate credentials separately to avoid move issues)
        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut alice_group = crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

        // Create alice_user after using alice_key for group (to avoid move)
        // Note: alice_user is only needed for MlsUser API validation, not used in this test
        let (alice_cred2, alice_key2) = crypto::generate_credential_with_key("alice").unwrap();
        let alice_identity = crate::models::Identity {
            username: "alice".to_string(),
            keypair_blob: alice_key2.to_public_vec(),
            credential_blob: vec![],
        };
        let _alice_user = MlsUser::new(
            "alice".to_string(),
            alice_identity,
            alice_key2,
            alice_cred2,
        );

        // Alice invites Bob
        let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
        let bob_identity = crate::models::Identity {
            username: "bob".to_string(),
            keypair_blob: bob_key.to_public_vec(),
            credential_blob: vec![],
        };
        let bob_user = MlsUser::new(
            "bob".to_string(),
            bob_identity,
            bob_key,
            bob_cred.clone(),
        );

        let bob_key_package = crypto::generate_key_package_bundle(&bob_cred, bob_user.get_signature_key(), &provider).unwrap();
        let (_commit, welcome, _) = crypto::add_members(
            &mut alice_group,
            &provider,
            &alice_key,
            &[bob_key_package.key_package()],
        ).unwrap();
        crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

        // Export ratchet tree
        let ratchet_tree = crypto::export_ratchet_tree(&alice_group);

        // Serialize Welcome and ratchet tree
        let welcome_bytes = welcome.tls_serialize_detached().unwrap();
        let welcome_b64 = general_purpose::STANDARD.encode(&welcome_bytes);

        let ratchet_tree_bytes = serde_json::to_vec(&ratchet_tree).unwrap();
        let ratchet_tree_b64 = general_purpose::STANDARD.encode(&ratchet_tree_bytes);

        // Bob processes Welcome to create membership
        let bob_membership = MlsMembership::from_welcome_message(
            "alice",
            &welcome_b64,
            &ratchet_tree_b64,
            &bob_user,
            &provider,
            &metadata_store,
        ).unwrap();

        // Verify membership state
        assert_eq!(bob_membership.get_group_name(), "testgroup");
        assert_eq!(bob_membership.list_members().len(), 2);
        assert!(bob_membership.list_members().contains(&"alice".to_string()));
        assert!(bob_membership.list_members().contains(&"bob".to_string()));
    }

    /// Test connecting to an existing group from storage
    ///
    /// Verifies:
    /// - Group can be loaded from storage
    /// - State is preserved (epoch, members)
    #[test]
    fn test_membership_connect_to_existing_group() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();

        // Alice creates a group
        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let alice_group = crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

        // Create alice_user with separate credentials (after group creation)
        let (alice_cred2, alice_key2) = crypto::generate_credential_with_key("alice").unwrap();
        let alice_identity = crate::models::Identity {
            username: "alice".to_string(),
            keypair_blob: alice_key2.to_public_vec(),
            credential_blob: vec![],
        };
        let alice_user = MlsUser::new(
            "alice".to_string(),
            alice_identity,
            alice_key2,
            alice_cred2,
        );
        let group_id = alice_group.group_id().as_slice().to_vec();

        // Store group ID mapping
        let group_id_key = format!("{}:{}", "alice", "testgroup");
        provider.save_group_name(&group_id_key, &group_id).unwrap();

        // Connect to existing group
        let membership = MlsMembership::connect_to_existing_group(
            "testgroup",
            &alice_user,
            &provider,
        ).unwrap();

        // Verify loaded state
        assert_eq!(membership.get_group_name(), "testgroup");
        assert_eq!(membership.get_group_id(), group_id.as_slice());
        assert_eq!(membership.list_members().len(), 1);
        assert!(membership.list_members().contains(&"alice".to_string()));
    }

    /// Test listing members
    ///
    /// Verifies:
    /// - Members are extracted correctly from MlsGroup
    /// - Usernames are decoded from BasicCredentials
    #[test]
    fn test_membership_list_members() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();

        // Create group with multiple members
        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut alice_group = crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

        // Create alice_user with separate credentials
        let (alice_cred2, alice_key2) = crypto::generate_credential_with_key("alice").unwrap();
        let alice_identity = crate::models::Identity {
            username: "alice".to_string(),
            keypair_blob: alice_key2.to_public_vec(),
            credential_blob: vec![],
        };
        let alice_user = MlsUser::new(
            "alice".to_string(),
            alice_identity,
            alice_key2,
            alice_cred2,
        );

        // Add Bob
        let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
        let bob_key_package = crypto::generate_key_package_bundle(&bob_cred, &bob_key, &provider).unwrap();
        let (_commit, _welcome, _) = crypto::add_members(
            &mut alice_group,
            &provider,
            &alice_key,
            &[bob_key_package.key_package()],
        ).unwrap();
        crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

        // Create membership
        let group_id = alice_group.group_id().as_slice().to_vec();
        let group_id_key = format!("{}:{}", "alice", "testgroup");
        provider.save_group_name(&group_id_key, &group_id).unwrap();

        let membership = MlsMembership::connect_to_existing_group(
            "testgroup",
            &alice_user,
            &provider,
        ).unwrap();

        // Verify members
        let members = membership.list_members();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&"alice".to_string()));
        assert!(members.contains(&"bob".to_string()));
    }

    /// Test processing incoming ApplicationMessage
    ///
    /// Verifies:
    /// - ApplicationMessage is decrypted correctly
    /// - Message content is extracted
    /// - Own messages are skipped
    #[tokio::test]
    async fn test_membership_process_incoming_application_message() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();

        // Setup two-party group (Alice and Bob)
        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut alice_group = crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

        // Add Bob
        let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
        let bob_key_package = crypto::generate_key_package_bundle(&bob_cred, &bob_key, &provider).unwrap();

        let bob_identity = crate::models::Identity {
            username: "bob".to_string(),
            keypair_blob: bob_key.to_public_vec(),
            credential_blob: vec![],
        };
        let bob_user = MlsUser::new(
            "bob".to_string(),
            bob_identity,
            bob_key,
            bob_cred,
        );

        let (_commit, welcome, _) = crypto::add_members(
            &mut alice_group,
            &provider,
            &alice_key,
            &[bob_key_package.key_package()],
        ).unwrap();
        crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

        // Bob joins
        let join_config = openmls::prelude::MlsGroupJoinConfig::default();
        let ratchet_tree = Some(crypto::export_ratchet_tree(&alice_group));
        let serialized = welcome.tls_serialize_detached().unwrap();
        let welcome_in = openmls::prelude::MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let bob_group = crypto::process_welcome_message(&provider, &join_config, &welcome_in, ratchet_tree).unwrap();

        // Alice sends a message
        let message_text = "Hello Bob!";
        let encrypted = crypto::create_application_message(
            &mut alice_group,
            &provider,
            &alice_key,
            message_text.as_bytes(),
        ).unwrap();

        // Serialize and encode for envelope
        let encrypted_bytes = encrypted.tls_serialize_detached().unwrap();
        let encrypted_b64 = general_purpose::STANDARD.encode(&encrypted_bytes);
        let group_id_b64 = general_purpose::STANDARD.encode(alice_group.group_id().as_slice());

        let envelope = MlsMessageEnvelope::ApplicationMessage {
            sender: "alice".to_string(),
            group_id: group_id_b64,
            encrypted_content: encrypted_b64,
        };

        // Bob processes the message
        let group_id = bob_group.group_id().as_slice().to_vec();
        let mut bob_membership = MlsMembership {
            group_name: "testgroup".to_string(),
            group_id,
            mls_group: bob_group,
            _phantom: std::marker::PhantomData,
        };

        // Process message (should succeed and display)
        let result = bob_membership.process_incoming_message(envelope, &bob_user, &provider).await;
        assert!(result.is_ok());
    }

    /// Test processing incoming CommitMessage
    ///
    /// Verifies:
    /// - CommitMessage updates group state
    /// - Member count increases after processing
    /// - Own commits are skipped
    #[tokio::test]
    async fn test_membership_process_incoming_commit_message() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();

        // Setup: Alice creates group, adds Bob, then Bob will receive commit when Alice adds Carol
        let (alice_cred, alice_key) = crypto::generate_credential_with_key("alice").unwrap();
        let mut alice_group = crypto::create_group_with_config(&alice_cred, &alice_key, &provider, "testgroup").unwrap();

        // Add Bob
        let (bob_cred, bob_key) = crypto::generate_credential_with_key("bob").unwrap();
        let bob_key_package = crypto::generate_key_package_bundle(&bob_cred, &bob_key, &provider).unwrap();

        let bob_identity = crate::models::Identity {
            username: "bob".to_string(),
            keypair_blob: bob_key.to_public_vec(),
            credential_blob: vec![],
        };
        let bob_user = MlsUser::new(
            "bob".to_string(),
            bob_identity,
            bob_key,
            bob_cred,
        );

        let (_commit1, welcome1, _) = crypto::add_members(
            &mut alice_group,
            &provider,
            &alice_key,
            &[bob_key_package.key_package()],
        ).unwrap();
        crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

        // Bob joins
        let join_config = openmls::prelude::MlsGroupJoinConfig::default();
        let ratchet_tree1 = Some(crypto::export_ratchet_tree(&alice_group));
        let serialized1 = welcome1.tls_serialize_detached().unwrap();
        let welcome1_in = openmls::prelude::MlsMessageIn::tls_deserialize(&mut serialized1.as_slice()).unwrap();
        let bob_group = crypto::process_welcome_message(&provider, &join_config, &welcome1_in, ratchet_tree1).unwrap();

        let group_id = bob_group.group_id().as_slice().to_vec();
        let mut bob_membership = MlsMembership {
            group_name: "testgroup".to_string(),
            group_id: group_id.clone(),
            mls_group: bob_group,
            _phantom: std::marker::PhantomData,
        };

        // Verify Bob initially sees 2 members
        assert_eq!(bob_membership.list_members().len(), 2);

        // Alice adds Carol
        let (carol_cred, carol_key) = crypto::generate_credential_with_key("carol").unwrap();
        let carol_key_package = crypto::generate_key_package_bundle(&carol_cred, &carol_key, &provider).unwrap();
        let (commit2, _welcome2, _) = crypto::add_members(
            &mut alice_group,
            &provider,
            &alice_key,
            &[carol_key_package.key_package()],
        ).unwrap();
        crypto::merge_pending_commit(&mut alice_group, &provider).unwrap();

        // Serialize commit for envelope
        let commit_bytes = commit2.tls_serialize_detached().unwrap();
        let commit_b64 = general_purpose::STANDARD.encode(&commit_bytes);
        let group_id_b64 = general_purpose::STANDARD.encode(&group_id);

        let commit_envelope = MlsMessageEnvelope::CommitMessage {
            group_id: group_id_b64,
            sender: "alice".to_string(),
            commit_blob: commit_b64,
        };

        // Bob processes commit
        let result = bob_membership.process_incoming_message(commit_envelope, &bob_user, &provider).await;
        assert!(result.is_ok());

        // Verify Bob now sees 3 members
        assert_eq!(bob_membership.list_members().len(), 3);
        assert!(bob_membership.list_members().contains(&"alice".to_string()));
        assert!(bob_membership.list_members().contains(&"bob".to_string()));
        assert!(bob_membership.list_members().contains(&"carol".to_string()));
    }
}
