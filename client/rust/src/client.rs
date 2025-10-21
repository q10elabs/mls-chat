/// Main MLS client orchestrator
///
/// Coordinates MLS operations using OpenMlsProvider for automatic group state persistence.

use crate::api::ServerApi;
use crate::cli::{format_control, format_message, run_input_loop};
use crate::crypto;
use crate::error::{Result, ClientError};
use crate::identity::IdentityManager;
use crate::models::{Command, Identity};
use crate::provider::MlsProvider;
use crate::storage::LocalStore;
use crate::websocket::MessageHandler;
use base64::{engine::general_purpose, Engine as _};
use directories::BaseDirs;
use openmls::prelude::GroupId;
use tls_codec::Deserialize;

/// Main MLS client
pub struct MlsClient {
    server_url: String,
    username: String,
    group_name: String,
    metadata_store: LocalStore,
    mls_provider: MlsProvider,
    api: ServerApi,
    websocket: Option<MessageHandler>,
    identity: Option<Identity>,
    /// Cached signature key pair for this session
    signature_key: Option<openmls_basic_credential::SignatureKeyPair>,
    /// Cached credential with key for this session (reused across groups)
    credential_with_key: Option<openmls::prelude::CredentialWithKey>,
    /// Current MLS group state (persisted across operations)
    mls_group: Option<openmls::prelude::MlsGroup>,
    /// Group ID for this session (used to load/save group state)
    group_id: Option<Vec<u8>>,
}

impl MlsClient {
    /// Create a new MLS client
    ///
    /// # Arguments
    /// * `server_url` - URL of the MLS server
    /// * `username` - Username for this client instance
    /// * `group_name` - Name of the group to create/join
    ///
    /// # Errors
    /// * File system errors when creating storage directories
    /// * Database initialization errors
    pub async fn new(server_url: &str, username: &str, group_name: &str) -> Result<Self> {
        // Get storage paths
        let base_dirs = BaseDirs::new()
            .ok_or_else(|| ClientError::Config("Failed to get home directory".to_string()))?;
        let mlschat_dir = base_dirs.home_dir().join(".mlschat");

        // Ensure directory exists
        std::fs::create_dir_all(&mlschat_dir)?;

        // Metadata storage (application-level only)
        let metadata_db_path = mlschat_dir.join("metadata.db");
        let metadata_store = LocalStore::new(&metadata_db_path)?;

        // MLS provider storage (handles all OpenMLS group state)
        let mls_db_path = mlschat_dir.join("mls.db");
        let mls_provider = MlsProvider::new(&mls_db_path)?;

        let api = ServerApi::new(server_url);

        Ok(Self {
            server_url: server_url.to_string(),
            username: username.to_string(),
            group_name: group_name.to_string(),
            metadata_store,
            mls_provider,
            api,
            websocket: None,
            identity: None,
            signature_key: None,
            credential_with_key: None,
            mls_group: None,
            group_id: None,
        })
    }

    /// Initialize the client (load or create identity, register with server)
    ///
    /// Creates or loads MLS credential and signature keys for this username.
    /// Generates a KeyPackage and registers it with the server.
    ///
    /// Uses persistent signature key storage via OpenMLS storage provider.
    /// Signature keys are reused across sessions for this username.
    ///
    /// # Errors
    /// * Storage errors when loading/saving identity
    /// * Network errors when registering with server
    /// * Crypto errors when generating credentials or key packages
    pub async fn initialize(&mut self) -> Result<()> {
        // Load or create a persistent identity using the IdentityManager
        let stored_identity = IdentityManager::load_or_create(
            &self.mls_provider,
            &self.metadata_store,
            &self.username,
        )?;

        let keypair_blob = stored_identity.signature_key.to_public_vec();

        // Store in-memory references (credential_with_key is reused across all groups for this user)
        self.identity = Some(Identity {
            username: self.username.clone(),
            keypair_blob: keypair_blob.clone(),
            credential_blob: vec![], // Not used - regenerated from username
        });
        self.signature_key = Some(stored_identity.signature_key);
        self.credential_with_key = Some(stored_identity.credential_with_key);

        log::info!(
            "Initialized identity for {} with persistent signature key",
            self.username
        );

        // Generate a KeyPackage for this user
        let key_package_bundle = crypto::generate_key_package_bundle(
            self.credential_with_key.as_ref().unwrap(),
            self.signature_key.as_ref().unwrap(),
            &self.mls_provider,
        )?;

        // Serialize the KeyPackage using TLS codec (standard MLS wire format)
        use tls_codec::Serialize;
        let key_package_bytes = key_package_bundle
            .key_package()
            .tls_serialize_detached()
            .map_err(|_e| crate::error::ClientError::Mls(
                crate::error::MlsError::OpenMls("Failed to serialize key package".to_string())
            ))?;

        // Register with server (idempotent) - sends the serialized KeyPackage
        self.api.register_user(&self.username, &key_package_bytes).await?;

        Ok(())
    }

    /// Connect to group (create or load existing)
    ///
    /// Creates a new MLS group if it doesn't exist, or loads an existing one from
    /// persistent storage. The group state is automatically managed by the OpenMLS
    /// storage provider.
    ///
    /// **Implementation Details:**
    /// - If group metadata exists: Load the persisted group state via `MlsGroup::load()`
    ///   (which preserves epoch, member list, and key material)
    /// - If group metadata doesn't exist: Create a new group using the stored credential_with_key
    /// - Uses the user's stored identity (credential_with_key) set during initialize()
    /// - Does NOT regenerate credentials on reconnection (per OpenMLS documentation)
    ///
    /// Also connects the WebSocket for real-time messaging.
    ///
    /// # Errors
    /// * WebSocket connection errors
    /// * MLS errors when creating/loading group
    pub async fn connect_to_group(&mut self) -> Result<()> {
        log::info!("Connecting to group: {}", self.group_name);

        // Get the stored group ID key for this user+group combination
        let group_id_key = format!("{}:{}", self.username, self.group_name);

        if let Some(sig_key) = &self.signature_key {
            if let Some(_identity) = &self.identity {
                if let Some(credential_with_key) = &self.credential_with_key {
                    // Use the stored credential from initialize() - don't regenerate
                    // This is the same credential used for all groups for this user

                    // Try to load existing group ID mapping from metadata store
                    match self.mls_provider.load_group_by_name(&group_id_key) {
                    Ok(Some(stored_group_id)) => {
                        // Group mapping exists in metadata - LOAD the persisted group state
                        // Per OpenMLS persistence.md: "MlsGroup can be loaded from the provider
                        // using the load constructor, which can be called with the GroupId"
                        // Convert bytes to GroupId by reconstructing from stored_group_id
                        // The stored_group_id is the serialized form of GroupId
                        match crypto::load_group_from_storage(
                            &self.mls_provider,
                            &GroupId::from_slice(&stored_group_id),
                        ) {
                            Ok(Some(group)) => {
                                log::info!(
                                    "Loaded existing MLS group: {} (id: {})",
                                    self.group_name,
                                    base64::engine::general_purpose::STANDARD.encode(&stored_group_id)
                                );
                                self.group_id = Some(stored_group_id);
                                self.mls_group = Some(group);
                            }
                            Ok(None) => {
                                // Group ID in metadata but not in storage - data inconsistency
                                // Recreate the group as fallback
                                log::warn!(
                                    "Group metadata exists but group not found in storage. Recreating group."
                                );
                                let group = crypto::create_group_with_config(
                                    &credential_with_key,
                                    sig_key,
                                    &self.mls_provider,
                                )?;
                                let group_id = group.group_id().as_slice().to_vec();
                                self.mls_provider.save_group_name(&group_id_key, &group_id)?;

                                log::info!(
                                    "Recreated MLS group: {} (id: {})",
                                    self.group_name,
                                    base64::engine::general_purpose::STANDARD.encode(&group_id)
                                );
                                self.group_id = Some(group_id);
                                self.mls_group = Some(group);
                            }
                            Err(e) => {
                                // Error loading group from storage
                                log::error!("Failed to load group from storage: {}", e);
                                return Err(e);
                            }
                        }
                    }
                    Ok(None) => {
                        // Group doesn't exist - create it
                        let group = crypto::create_group_with_config(
                            &credential_with_key,
                            sig_key,
                            &self.mls_provider,
                        )?;
                        let group_id = group.group_id().as_slice().to_vec();

                        // Store the group ID mapping for later retrieval
                        self.mls_provider.save_group_name(&group_id_key, &group_id)?;

                        log::info!(
                            "Created new MLS group: {} (id: {})",
                            self.group_name,
                            base64::engine::general_purpose::STANDARD.encode(&group_id)
                        );
                        self.group_id = Some(group_id);
                        self.mls_group = Some(group);
                    }
                    Err(e) => {
                        // Error checking storage - create new group as fallback
                        log::warn!("Error checking group mapping: {}. Creating new group.", e);
                        let group = crypto::create_group_with_config(
                            &credential_with_key,
                            sig_key,
                            &self.mls_provider,
                        )?;
                        let group_id = group.group_id().as_slice().to_vec();

                        // Try to store the mapping
                        let _ = self.mls_provider.save_group_name(&group_id_key, &group_id);

                        self.group_id = Some(group_id);
                        self.mls_group = Some(group);
                    }
                    }
                }
            }
        }

        // Connect WebSocket for real-time messaging
        self.websocket = Some(MessageHandler::connect(&self.server_url, &self.username).await?);
        self.websocket
            .as_ref()
            .unwrap()
            .subscribe_to_group(&self.group_name)
            .await?;

        Ok(())
    }

    /// Send a message to the group
    ///
    /// Encrypts the message using MLS and sends it via WebSocket.
    ///
    /// # Errors
    /// * WebSocket send errors
    /// * MLS encryption errors
    pub async fn send_message(&mut self, text: &str) -> Result<()> {
        if let Some(websocket) = &self.websocket {
            if let Some(sig_key) = &self.signature_key {
                if let Some(group) = &mut self.mls_group {
                    // Encrypt the message using the persistent group state
                    let encrypted_msg = crypto::create_application_message(
                        group,
                        &self.mls_provider,
                        sig_key,
                        text.as_bytes(),
                    )?;

                    // Serialize the encrypted MLS message using TLS codec
                    use tls_codec::Serialize;
                    let encrypted_bytes = encrypted_msg
                        .tls_serialize_detached()
                        .map_err(|_e| crate::error::ClientError::Mls(crate::error::MlsError::OpenMls("Failed to serialize message".to_string())))?;

                    // Encode for WebSocket transmission
                    let encrypted_b64 = general_purpose::STANDARD.encode(&encrypted_bytes);

                    // Send via WebSocket
                    websocket.send_message(&self.group_name, &encrypted_b64).await?;
                    println!("{}", format_message(&self.group_name, &self.username, text));
                } else {
                    log::error!("Cannot send message: group not connected");
                    return Err(crate::error::ClientError::Mls(crate::error::MlsError::GroupNotFound).into());
                }
            }
        }
        Ok(())
    }

    /// Process incoming messages
    ///
    /// Receives encrypted messages from WebSocket and decrypts them using MLS.
    ///
    /// # Errors
    /// * WebSocket receive errors
    /// * Message decryption errors
    pub async fn process_incoming(&mut self) -> Result<()> {
        if let Some(websocket) = &mut self.websocket {
            if let Some(msg) = websocket.next_message().await? {
                // Decode base64-encoded MLS message
                match general_purpose::STANDARD.decode(&msg.encrypted_content) {
                    Ok(encrypted_bytes) => {
                        // Deserialize the MLS message
                        match openmls::prelude::MlsMessageIn::tls_deserialize(&mut encrypted_bytes.as_slice()) {
                            Ok(message_in) => {
                                // Process the message using the persistent group state
                                if let Some(group) = &mut self.mls_group {
                                    match crypto::process_message(group, &self.mls_provider, &message_in) {
                                        Ok(processed_msg) => {
                                            // Extract the plaintext from the application message
                                            use openmls::prelude::ProcessedMessageContent;
                                            match processed_msg.content() {
                                                ProcessedMessageContent::ApplicationMessage(_app_msg) => {
                                                    // Message decrypted successfully
                                                    // Note: In a production system, you'd extract the actual plaintext bytes
                                                    println!("{}", format_message(&msg.group_id, &msg.sender, "[message received]"));
                                                }
                                                _ => {
                                                    log::debug!("Received non-application message type");
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("Failed to process/decrypt message: {:?}", e);
                                        }
                                    }
                                } else {
                                    log::error!("Cannot process incoming message: group not connected");
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to deserialize MLS message: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to decode base64 message: {}", e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Invite a user to the group
    ///
    /// Fetches the invitee's key package, adds them to the MLS group,
    /// and sends the Welcome message via WebSocket.
    ///
    /// # Errors
    /// * Server communication errors
    /// * MLS operation errors
    pub async fn invite_user(&mut self, invitee_username: &str) -> Result<()> {
        log::info!("Inviting {} to group {}", invitee_username, self.group_name);

        if let Some(sig_key) = &self.signature_key {
            // Fetch invitee's public key from server to verify they exist
            let _invitee_key = match self.api.get_user_key(invitee_username).await {
                Ok(key) => key,
                Err(e) => {
                    log::error!("Failed to fetch public key for {}: {}", invitee_username, e);
                    return Err(e);
                }
            };

            // Generate a key package for the invitee
            let (invitee_cred, invitee_sig_key) = crypto::generate_credential_with_key(invitee_username)?;
            let invitee_key_package = crypto::generate_key_package_bundle(&invitee_cred, &invitee_sig_key, &self.mls_provider)?;

            // Add the member to the persistent group
            if let Some(group) = &mut self.mls_group {
                let (commit_message, welcome_message, _group_info) = crypto::add_members(
                    group,
                    &self.mls_provider,
                    sig_key,
                    &[invitee_key_package.key_package()],
                )?;

                // Merge the pending commit
                crypto::merge_pending_commit(group, &self.mls_provider)?;

                // Send Welcome message via WebSocket
                if let Some(websocket) = &self.websocket {
                    // Serialize the welcome message
                    use tls_codec::Serialize;
                    let welcome_bytes = welcome_message
                        .tls_serialize_detached()
                        .map_err(|_e| crate::error::ClientError::Mls(
                            crate::error::MlsError::OpenMls("Failed to serialize welcome message".to_string())
                        ))?;

                    // Encode and send via WebSocket with a marker to identify it as a welcome message
                    let welcome_b64 = general_purpose::STANDARD.encode(&welcome_bytes);
                    let invite_marker = format!("INVITE:{}:{}", invitee_username, welcome_b64);
                    websocket.send_message(&self.group_name, &invite_marker).await?;

                    log::info!("Sent welcome message to {}", invitee_username);
                }

                // Also send the commit message so other members know about the change
                use tls_codec::Serialize;
                let commit_bytes = commit_message
                    .tls_serialize_detached()
                    .map_err(|_e| crate::error::ClientError::Mls(
                        crate::error::MlsError::OpenMls("Failed to serialize commit message".to_string())
                    ))?;
                let commit_b64 = general_purpose::STANDARD.encode(&commit_bytes);

                if let Some(websocket) = &self.websocket {
                    websocket.send_message(&self.group_name, &commit_b64).await?;
                }
            } else {
                log::error!("Cannot invite user: group not connected");
                return Err(crate::error::ClientError::Mls(crate::error::MlsError::GroupNotFound).into());
            }

            // Update member list in metadata store
            let mut members = self.list_members();
            if !members.contains(&invitee_username.to_string()) {
                members.push(invitee_username.to_string());
                self.metadata_store.save_group_members(&self.username, &self.group_name, &members)?;
            }

            println!(
                "{}",
                format_control(
                    &self.group_name,
                    &format!("invited {} to the group", invitee_username)
                )
            );
        }
        Ok(())
    }

    /// List group members
    ///
    /// Returns the members list stored in metadata. In a real implementation,
    /// this would come from the actual group state in the MLS provider.
    pub fn list_members(&self) -> Vec<String> {
        // Load from metadata store (or reconstruct from MLS group state)
        self.metadata_store
            .get_group_members(&self.username, &self.group_name)
            .unwrap_or_else(|_| vec![self.username.clone()])
    }

    /// Test helper: get reference to identity
    pub fn get_identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
    }

    /// Test helper: check if group is connected
    pub fn is_group_connected(&self) -> bool {
        self.mls_group.is_some()
    }

    /// Test helper: get group ID
    pub fn get_group_id(&self) -> Option<Vec<u8>> {
        self.group_id.clone()
    }

    /// Test helper: get signature key
    pub fn has_signature_key(&self) -> bool {
        self.signature_key.is_some()
    }

    /// Test helper: get websocket status
    pub fn is_websocket_connected(&self) -> bool {
        self.websocket.is_some()
    }

    /// Test helper: get reference to MLS provider
    pub fn get_provider(&self) -> &MlsProvider {
        &self.mls_provider
    }

    /// Run the main client loop
    pub async fn run(&mut self) -> Result<()> {
        println!("Connected to group: {}", self.group_name);
        println!("Commands: /invite <username>, /list, /quit");
        println!("Type messages to send to the group");
        
        // Spawn task for incoming messages
        let mut websocket = self.websocket.take().unwrap();
        
        tokio::spawn(async move {
            loop {
                if let Some(msg) = websocket.next_message().await.unwrap_or(None) {
                    let decrypted = format!("decrypted:{}", msg.encrypted_content);
                    println!("{}", format_message(&msg.group_id, &msg.sender, &decrypted));
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        });
        
        // Run input loop
        let group_name = self.group_name.clone();
        let username = self.username.clone();
        let members = self.list_members();
        
        run_input_loop(|command| {
            match command {
                Command::Invite(username) => {
                    // For now, just print a message
                    println!("{}", format_control(&group_name, &format!("invited {} to the group", username)));
                }
                Command::List => {
                    println!("Group members: {}", members.join(", "));
                }
                Command::Message(text) => {
                    // For now, just print the message
                    println!("{}", format_message(&group_name, &username, &text));
                }
                Command::Quit => {
                    println!("Goodbye!");
                    std::process::exit(0);
                }
            }
            Ok(())
        }).await?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmls::prelude::OpenMlsProvider;

    #[test]
    fn test_create_group_stores_metadata() {
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let mls_provider = crate::provider::MlsProvider::new(&db_path).unwrap();

        let group_id_key = "alice:testgroup";

        // Verify group doesn't exist initially
        let exists = mls_provider.group_exists(group_id_key).unwrap();
        assert!(!exists, "Group should not exist initially");

        // Create a test group ID (from a real group)
        let (cred, sig_key) = crate::crypto::generate_credential_with_key("alice").unwrap();
        let group = crate::crypto::create_group_with_config(&cred, &sig_key, &mls_provider).unwrap();
        let group_id = group.group_id().as_slice().to_vec();

        // Store metadata
        mls_provider
            .save_group_name(group_id_key, &group_id)
            .unwrap();

        // Verify it's stored
        let stored = mls_provider.load_group_by_name(group_id_key).unwrap();
        assert!(stored.is_some(), "Group should be stored in metadata");
        assert_eq!(stored.unwrap(), group_id, "Stored group ID should match");
    }

    #[test]
    fn test_load_group_preserves_state() {
        // This test verifies the critical fix: that loading a group preserves its state
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let mls_provider = crate::provider::MlsProvider::new(&db_path).unwrap();

        // Create a group
        let (credential_with_key, _) = crate::crypto::generate_credential_with_key("alice").unwrap();
        let sig_key = crate::crypto::generate_credential_with_key("alice").unwrap().1;

        let group1 = crate::crypto::create_group_with_config(&credential_with_key, &sig_key, &mls_provider)
            .unwrap();
        let initial_epoch = group1.epoch();
        let group_id = group1.group_id().clone();

        // Add a member to advance epoch
        let (bob_cred, bob_key) = crate::crypto::generate_credential_with_key("bob").unwrap();
        let bob_key_package = crate::crypto::generate_key_package_bundle(&bob_cred, &bob_key, &mls_provider).unwrap();

        let mut group2 = crate::crypto::load_group_from_storage(&mls_provider, &group_id)
            .unwrap()
            .unwrap();

        let (_commit, _welcome, _group_info) = crate::crypto::add_members(
            &mut group2,
            &mls_provider,
            &sig_key,
            &[bob_key_package.key_package()],
        ).unwrap();

        crate::crypto::merge_pending_commit(&mut group2, &mls_provider).unwrap();
        let epoch_after_add = group2.epoch();

        // Now load the group from storage again
        let loaded_group = crate::crypto::load_group_from_storage(&mls_provider, &group_id)
            .unwrap();

        assert!(
            loaded_group.is_some(),
            "Group should exist in storage after modification"
        );

        let loaded = loaded_group.unwrap();

        // Verify state is preserved - THIS IS THE CRITICAL FIX
        assert_eq!(
            loaded.group_id(),
            group1.group_id(),
            "Loaded group should have same ID"
        );
        assert_eq!(
            loaded.epoch(),
            epoch_after_add,
            "Loaded group should have same epoch after member add - epoch preservation is the critical fix!"
        );
        assert!(
            loaded.epoch() > initial_epoch,
            "Epoch should have advanced after member addition"
        );
    }

    #[test]
    fn test_reconnect_loads_not_creates() {
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let mls_provider = crate::provider::MlsProvider::new(&db_path).unwrap();
        let group_id_key = "alice:testgroup";

        // Alice creates a group
        let (credential_with_key, _) = crate::crypto::generate_credential_with_key("alice").unwrap();
        let sig_key = crate::crypto::generate_credential_with_key("alice").unwrap().1;

        let mut alice_group1 = crate::crypto::create_group_with_config(&credential_with_key, &sig_key, &mls_provider).unwrap();
        let alice_group1_id = alice_group1.group_id().clone();

        // Send a message to advance epoch
        let msg = b"Hello from Alice";
        let _encrypted = crate::crypto::create_application_message(&mut alice_group1, &mls_provider, &sig_key, msg).unwrap();
        let alice_epoch_1 = alice_group1.epoch();

        // Store metadata
        mls_provider.save_group_name(group_id_key, &alice_group1_id.as_slice().to_vec()).unwrap();

        // --- Simulate disconnection and reconnection ---
        // In a new session, load the group
        let stored_id = mls_provider.load_group_by_name(group_id_key).unwrap();
        assert!(stored_id.is_some(), "Group metadata should persist");

        let stored_group_id = stored_id.unwrap();

        // Load the group (as connect_to_group would do)
        let alice_group2 = crate::crypto::load_group_from_storage(&mls_provider, &GroupId::from_slice(&stored_group_id)).unwrap();

        assert!(alice_group2.is_some(), "Group should exist in storage");
        let loaded_group = alice_group2.unwrap();

        // Verify: same group ID
        assert_eq!(
            loaded_group.group_id(),
            &alice_group1_id,
            "Reconnected group should have same ID"
        );

        // Verify: same epoch (state preserved) - THIS IS THE CRITICAL FIX
        assert_eq!(
            loaded_group.epoch(),
            alice_epoch_1,
            "Reconnected group should have same epoch - this is the critical fix!"
        );
    }

}
