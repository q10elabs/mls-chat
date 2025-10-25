/// MLS cryptographic operations using OpenMLS

use crate::error::{Result, MlsError};
use openmls::prelude::*;
use openmls::messages::group_info::GroupInfo;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_basic_credential::SignatureKeyPair;

/// Generate a credential with key for a username
pub fn generate_credential_with_key(username: &str) -> Result<(CredentialWithKey, SignatureKeyPair)> {
    let provider = &OpenMlsRustCrypto::default();
    let ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
    
    // Create basic credential
    let credential = BasicCredential::new(username.as_bytes().to_vec());
    
    // Generate signature key pair
    let signature_keys = SignatureKeyPair::new(ciphersuite.signature_algorithm())
        .map_err(|e| MlsError::OpenMls(e.to_string()))?;

    // Store the signature key into the key store so OpenMLS has access to it
    signature_keys
        .store(provider.storage())
        .map_err(|e| MlsError::OpenMls(e.to_string()))?;
     
    let credential_with_key = CredentialWithKey {
        credential: credential.into(),
        signature_key: signature_keys.to_public_vec().into(),
    };
    
    Ok((credential_with_key, signature_keys))
}

/// Generate a key package bundle for the given credential and signature key
pub fn generate_key_package_bundle(
    credential: &CredentialWithKey, 
    signer: &SignatureKeyPair,
    provider: &impl OpenMlsProvider,
) -> Result<KeyPackageBundle> {
    let ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
    
    let key_package = KeyPackage::builder()
        .build(
            ciphersuite,
            provider,
            signer,
            credential.clone(),
        )
        .map_err(|e| MlsError::OpenMls(e.to_string()))?;
    
    Ok(key_package)
}

/// Create a new MLS group with configuration
pub fn create_group_with_config(
    credential: &CredentialWithKey,
    signer: &SignatureKeyPair,
    provider: &impl OpenMlsProvider,
    group_name: &str,
) -> Result<MlsGroup> {
    // Create group metadata extension (encrypted in group state)
    let metadata = crate::extensions::GroupMetadata::new(group_name.to_string());
    let metadata_bytes = metadata.to_bytes()
        .map_err(|e| MlsError::OpenMls(format!("Failed to serialize group metadata: {}", e)))?;

    let group_metadata_ext = Extensions::single(Extension::Unknown(
        crate::extensions::GROUP_METADATA_EXTENSION_TYPE,
        UnknownExtension(metadata_bytes),
    ));

    // Create group with metadata extension in GroupContext
    let group_config = MlsGroupCreateConfig::builder()
        .with_group_context_extensions(group_metadata_ext)
        .map_err(|e| MlsError::OpenMls(e.to_string()))?
        .build();

    let group = MlsGroup::new(
        provider,
        signer,
        &group_config,
        credential.clone(),
    )
    .map_err(|e| MlsError::OpenMls(e.to_string()))?;

    Ok(group)
}

/// Create an application message
pub fn create_application_message(
    group: &mut MlsGroup,
    provider: &impl OpenMlsProvider,
    signer: &SignatureKeyPair,
    plaintext: &[u8],
) -> Result<MlsMessageOut> {
    let message = group.create_message(
        provider,
        signer,
        plaintext,
    )
    .map_err(|e| MlsError::OpenMls(e.to_string()))?;
    
    Ok(message)
}

/// Process an incoming MLS message
pub fn process_message(
    group: &mut MlsGroup,
    provider: &impl OpenMlsProvider,
    message: &MlsMessageIn,
) -> Result<ProcessedMessage> {
    let protocol_message = message.clone().try_into_protocol_message()
        .map_err(|e| MlsError::OpenMls(e.to_string()))?;
    
    let processed_message = group.process_message(
        provider,
        protocol_message,
    )
    .map_err(|e| MlsError::OpenMls(e.to_string()))?;
    
    Ok(processed_message)
}

/// Add members to the group
/// Returns (commit_message_for_existing_members, welcome_message_for_new_members, group_info)
pub fn add_members(
    group: &mut MlsGroup,
    provider: &impl OpenMlsProvider,
    signer: &SignatureKeyPair,
    key_packages: &[&KeyPackage],
) -> Result<(MlsMessageOut, MlsMessageOut, Option<GroupInfo>)> {
    // Convert &[&KeyPackage] to &[KeyPackage] by cloning
    let key_packages_owned: Vec<KeyPackage> = key_packages.iter().map(|kp| (*kp).clone()).collect();
    
    let (commit_message, welcome_message, group_info) = group.add_members(
        provider,
        signer,
        &key_packages_owned,
    )
    .map_err(|e| MlsError::OpenMls(e.to_string()))?;
    
    Ok((commit_message, welcome_message, group_info))
}

/// Process a Welcome message to join a group (for new members)
/// The welcome_message is the encrypted Welcome message received from the group organizer
pub fn process_welcome_message(
    provider: &impl OpenMlsProvider,
    config: &MlsGroupJoinConfig,
    welcome_message: &MlsMessageIn,
    ratchet_tree: Option<RatchetTreeIn>,
) -> Result<MlsGroup> {
    // Extract Welcome from the incoming message
    let welcome = match welcome_message.clone().extract() {
        MlsMessageBodyIn::Welcome(w) => w,
        _ => return Err(MlsError::OpenMls("Expected Welcome message".to_string()).into()),
    };
    
    // Create a staged join from the welcome message
    let staged_join = StagedWelcome::new_from_welcome(
        provider,
        config,
        welcome,
        ratchet_tree,
    )
    .map_err(|e| MlsError::OpenMls(e.to_string()))?;
    
    // Convert the staged join into a group
    let group = staged_join.into_group(provider)
        .map_err(|e| MlsError::OpenMls(e.to_string()))?;
    
    Ok(group)
}

/// Merge pending commit after adding members
pub fn merge_pending_commit(
    group: &mut MlsGroup,
    provider: &impl OpenMlsProvider,
) -> Result<()> {
    group.merge_pending_commit(provider)
        .map_err(|e| MlsError::OpenMls(e.to_string()))?;
    
    Ok(())
}

/// Export ratchet tree for new members
pub fn export_ratchet_tree(group: &MlsGroup) -> RatchetTreeIn {
    group.export_ratchet_tree().into()
}

/// Load an MLS group from storage by its group ID
///
/// This retrieves a previously created and persisted MLS group from the storage provider.
/// The OpenMLS storage provider automatically persists all group state (epoch, secrets,
/// ratcheting tree, credentials) when the group is modified. This function reconstructs
/// the full MlsGroup instance from that persisted state.
///
/// # Arguments
/// * `provider` - The MLS provider containing the storage backend
/// * `group_id` - The ID of the group to load
///
/// # Returns
/// `Ok(Some(group))` if the group exists and was loaded successfully
/// `Ok(None)` if the group does not exist in storage
/// `Err(...)` if there was an error loading the group
pub fn load_group_from_storage(
    provider: &impl OpenMlsProvider,
    group_id: &GroupId,
) -> Result<Option<MlsGroup>> {
    MlsGroup::load(provider.storage(), group_id)
        .map_err(|e| MlsError::OpenMls(format!("Failed to load group: {}", e)).into())
}

/// Extract group metadata from group context extensions
pub fn extract_group_metadata(group: &MlsGroup) -> Result<Option<crate::extensions::GroupMetadata>> {
    let extensions = group.extensions();

    if let Some(ext) = extensions.unknown(crate::extensions::GROUP_METADATA_EXTENSION_TYPE) {
        let metadata = crate::extensions::GroupMetadata::from_bytes(&ext.0)
            .map_err(|e| MlsError::OpenMls(format!("Failed to parse group metadata: {}", e)))?;
        Ok(Some(metadata))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tls_codec::{Deserialize, Serialize};

    #[test]
    fn test_generate_credential_with_key() {
        let (_credential, _keypair) = generate_credential_with_key("alice").unwrap();
        // Credential and keypair generated successfully
    }

    #[test]
    fn test_generate_key_package_bundle() {
        let provider = &OpenMlsRustCrypto::default();
        let (credential, keypair) = generate_credential_with_key("alice").unwrap();
        let _key_package = generate_key_package_bundle(&credential, &keypair, provider).unwrap();
        // Key package generated successfully
    }

    #[test]
    fn test_create_group_with_config() {
        let provider = &OpenMlsRustCrypto::default();
        let (credential, keypair) = generate_credential_with_key("alice").unwrap();
        let group = create_group_with_config(&credential, &keypair, provider, "testgroup").unwrap();
        assert!(!group.group_id().as_slice().is_empty());

        // Verify metadata was stored
        let metadata = extract_group_metadata(&group).unwrap();
        assert!(metadata.is_some());
        assert_eq!(metadata.unwrap().name, "testgroup");
    }

    #[test]
    fn test_create_and_process_application_message() {
        let provider = &OpenMlsRustCrypto::default();

        // Alice creates a group
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group = create_group_with_config(&alice_cred, &alice_key, provider, "testgroup").unwrap();

        // Bob generates key package
        let (bob_cred, bob_key) = generate_credential_with_key("bob").unwrap();
        let bob_key_package = generate_key_package_bundle(&bob_cred, &bob_key, provider).unwrap();

        // Alice adds Bob to the group
        let (_commit, welcome_message, _group_info) = add_members(
            &mut alice_group,
            provider,
            &alice_key,
            &[bob_key_package.key_package()],
        )
        .unwrap();

        // Alice merges the pending commit
        merge_pending_commit(&mut alice_group, provider).unwrap();

        // Bob joins the group via Welcome
        let ratchet_tree = Some(export_ratchet_tree(&alice_group));
        let join_config = MlsGroupJoinConfig::default();
        let serialized = welcome_message.tls_serialize_detached().unwrap();
        let welcome_in = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let mut bob_group =
            process_welcome_message(provider, &join_config, &welcome_in, ratchet_tree).unwrap();

        // Alice sends a message
        let plaintext = b"Hello from Alice!";
        let encrypted = create_application_message(&mut alice_group, provider, &alice_key, plaintext)
            .unwrap();

        // Serialize the message for transport
        let serialized = encrypted.tls_serialize_detached().unwrap();
        let deserialized = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();

        // Bob processes Alice's message
        let processed = process_message(&mut bob_group, provider, &deserialized).unwrap();

        // Verify Bob received an application message
        match processed.content() {
            ProcessedMessageContent::ApplicationMessage(_app_msg) => {
                // ApplicationMessage received successfully by Bob
            }
            _ => panic!("Expected application message"),
        }
    }

    #[test]
    fn test_add_member_flow() {
        let provider = &OpenMlsRustCrypto::default();

        // Alice creates group
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group = create_group_with_config(&alice_cred, &alice_key, provider, "testgroup").unwrap();

        // Bob generates key package
        let (bob_cred, bob_key) = generate_credential_with_key("bob").unwrap();
        let bob_key_package = generate_key_package_bundle(&bob_cred, &bob_key, provider).unwrap();

        // Alice adds Bob
        let (_commit, welcome_message, _group_info) = add_members(
            &mut alice_group,
            provider,
            &alice_key,
            &[bob_key_package.key_package()],
        )
        .unwrap();

        // Alice merges the pending commit
        merge_pending_commit(&mut alice_group, provider).unwrap();

        // Bob processes Welcome message (with ratchet tree from Alice's group)
        let ratchet_tree = Some(export_ratchet_tree(&alice_group));
        let join_config = MlsGroupJoinConfig::default();
        // Serialize and deserialize the welcome message to convert MlsMessageOut to MlsMessageIn
        let serialized = welcome_message.tls_serialize_detached().unwrap();
        let welcome_in = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let bob_group = process_welcome_message(provider, &join_config, &welcome_in, ratchet_tree).unwrap();

        assert_eq!(bob_group.group_id().as_slice(), alice_group.group_id().as_slice());
    }

    #[test]
    fn test_two_party_messaging() {
        let provider = &OpenMlsRustCrypto::default();

        // Alice creates group
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group = create_group_with_config(&alice_cred, &alice_key, provider, "testgroup").unwrap();

        // Bob generates key package
        let (bob_cred, bob_key) = generate_credential_with_key("bob").unwrap();
        let bob_key_package = generate_key_package_bundle(&bob_cred, &bob_key, provider).unwrap();

        // Alice adds Bob
        let (_commit, welcome_message, _group_info) = add_members(
            &mut alice_group,
            provider,
            &alice_key,
            &[bob_key_package.key_package()],
        )
        .unwrap();

        // Alice merges the pending commit
        merge_pending_commit(&mut alice_group, provider).unwrap();

        // Bob processes Welcome message
        let ratchet_tree = Some(export_ratchet_tree(&alice_group));
        let join_config = MlsGroupJoinConfig::default();
        let serialized = welcome_message.tls_serialize_detached().unwrap();
        let welcome_in = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let mut bob_group = process_welcome_message(provider, &join_config, &welcome_in, ratchet_tree).unwrap();

        // Alice sends a message
        let alice_message = b"Hello from Alice!";
        let encrypted_alice = create_application_message(&mut alice_group, provider, &alice_key, alice_message).unwrap();

        // Bob processes Alice's message
        let serialized = encrypted_alice.tls_serialize_detached().unwrap();
        let deserialized = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let processed = process_message(&mut bob_group, provider, &deserialized).unwrap();

        // Verify Bob received Alice's message
        match processed.content() {
            ProcessedMessageContent::ApplicationMessage(_app_msg) => {
                // ApplicationMessage received successfully
            }
            _ => panic!("Expected application message"),
        }
    }

    #[test]
    fn test_three_party_messaging() {
        let provider = &OpenMlsRustCrypto::default();

        // Alice creates group
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group = create_group_with_config(&alice_cred, &alice_key, provider, "testgroup").unwrap();

        // Bob joins
        let (bob_cred, bob_key) = generate_credential_with_key("bob").unwrap();
        let bob_key_package = generate_key_package_bundle(&bob_cred, &bob_key, provider).unwrap();
        let (_commit1, welcome_bob_message, _) =
            add_members(&mut alice_group, provider, &alice_key, &[bob_key_package.key_package()])
                .unwrap();
        merge_pending_commit(&mut alice_group, provider).unwrap();

        let ratchet_tree_bob = Some(export_ratchet_tree(&alice_group));
        let join_config = MlsGroupJoinConfig::default();
        let serialized = welcome_bob_message.tls_serialize_detached().unwrap();
        let welcome_bob_in = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let mut bob_group =
            process_welcome_message(provider, &join_config, &welcome_bob_in, ratchet_tree_bob)
                .unwrap();

        // Bob sends a message to Alice (while they're both at epoch 1)
        let bob_message = b"Hello from Bob!";
        let encrypted_bob = create_application_message(&mut bob_group, provider, &bob_key, bob_message)
            .unwrap();

        // Alice processes Bob's message
        let serialized = encrypted_bob.tls_serialize_detached().unwrap();
        let deserialized = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let processed_alice = process_message(&mut alice_group, provider, &deserialized).unwrap();

        // Verify Alice received Bob's message
        match processed_alice.content() {
            ProcessedMessageContent::ApplicationMessage(_app_msg) => {
                // ApplicationMessage received successfully
            }
            _ => panic!("Expected application message from Bob"),
        }

        // Now Carol joins (Alice is at epoch 2, Bob is still at epoch 1)
        let (carol_cred, carol_key) = generate_credential_with_key("carol").unwrap();
        let carol_key_package = generate_key_package_bundle(&carol_cred, &carol_key, provider).unwrap();
        let (_commit2, welcome_carol_message, _) =
            add_members(&mut alice_group, provider, &alice_key, &[carol_key_package.key_package()])
                .unwrap();
        merge_pending_commit(&mut alice_group, provider).unwrap();

        let ratchet_tree_carol = Some(export_ratchet_tree(&alice_group));
        let serialized = welcome_carol_message.tls_serialize_detached().unwrap();
        let welcome_carol_in = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let mut carol_group = process_welcome_message(provider, &join_config, &welcome_carol_in, ratchet_tree_carol)
            .unwrap();

        // Carol sends a message to Alice (who is at epoch 2)
        let carol_message = b"Hello from Carol!";
        let encrypted_carol =
            create_application_message(&mut carol_group, provider, &carol_key, carol_message).unwrap();

        // Alice processes Carol's message
        let serialized = encrypted_carol.tls_serialize_detached().unwrap();
        let deserialized = MlsMessageIn::tls_deserialize(&mut serialized.as_slice()).unwrap();
        let processed_alice_from_carol = process_message(&mut alice_group, provider, &deserialized)
            .unwrap();

        // Verify Alice received Carol's message
        match processed_alice_from_carol.content() {
            ProcessedMessageContent::ApplicationMessage(_app_msg) => {
                // ApplicationMessage received successfully
            }
            _ => panic!("Expected application message from Carol"),
        }
    }

    #[test]
    fn test_group_persistence_through_metadata() {
        use tempfile::tempdir;
        use crate::provider::MlsProvider;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("groups.db");

        // === Session 1: Create group and store metadata ===
        let provider1 = MlsProvider::new(&db_path).unwrap();
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let alice_group_1 = create_group_with_config(&alice_cred, &alice_key, &provider1, "testgroup").unwrap();
        let group_id = alice_group_1.group_id().as_slice().to_vec();

        // Store the group ID in metadata (simulating client storing group mapping)
        let group_id_key = "alice:testgroup";
        provider1.save_group_name(group_id_key, &group_id).unwrap();

        // === Session 2: Verify metadata persists across provider instances ===
        let provider2 = MlsProvider::new(&db_path).unwrap();

        // Load the group ID from metadata
        let loaded_group_id = provider2.load_group_by_name(group_id_key).unwrap();

        assert!(
            loaded_group_id.is_some(),
            "Group ID should be stored in metadata"
        );

        let loaded_id = loaded_group_id.unwrap();
        assert_eq!(
            loaded_id, group_id,
            "Loaded group ID should match original"
        );
    }

    #[test]
    fn test_group_id_metadata_persists_with_member_addition() {
        use tempfile::tempdir;
        use crate::provider::MlsProvider;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("groups.db");

        // === Session 1: Create group and add Bob ===
        let provider1 = MlsProvider::new(&db_path).unwrap();
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group_1 = create_group_with_config(&alice_cred, &alice_key, &provider1, "group").unwrap();
        let group_id = alice_group_1.group_id().as_slice().to_vec();

        // Bob generates his key package
        let (bob_cred, bob_key) = generate_credential_with_key("bob").unwrap();
        let bob_key_package = generate_key_package_bundle(&bob_cred, &bob_key, &provider1).unwrap();

        // Alice adds Bob to the group
        let (_commit, _welcome_msg, _) = add_members(
            &mut alice_group_1,
            &provider1,
            &alice_key,
            &[bob_key_package.key_package()],
        )
        .unwrap();
        merge_pending_commit(&mut alice_group_1, &provider1).unwrap();

        let group_id_key = "alice:testgroup";
        provider1.save_group_name(group_id_key, &group_id).unwrap();

        // === Session 2: Verify group metadata persists ===
        let provider2 = MlsProvider::new(&db_path).unwrap();

        // Load group ID from metadata - this is the key persistence mechanism
        let loaded_id = provider2.load_group_by_name(group_id_key).unwrap();
        assert!(loaded_id.is_some(), "Group ID should persist in metadata");

        assert_eq!(
            loaded_id.unwrap(),
            group_id,
            "Group ID should match after member addition - metadata persistence works"
        );
    }

    #[test]
    fn test_group_id_metadata_for_multiple_groups() {
        use tempfile::tempdir;
        use crate::provider::MlsProvider;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("groups.db");

        let provider1 = MlsProvider::new(&db_path).unwrap();

        // Create and store first group
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let group1 = create_group_with_config(&alice_cred, &alice_key, &provider1, "group1").unwrap();
        let group1_id = group1.group_id().as_slice().to_vec();
        provider1.save_group_name("alice:group1", &group1_id).unwrap();

        // Create and store second group
        let (bob_cred, bob_key) = generate_credential_with_key("bob").unwrap();
        let group2 = create_group_with_config(&bob_cred, &bob_key, &provider1, "group2").unwrap();
        let group2_id = group2.group_id().as_slice().to_vec();
        provider1.save_group_name("bob:group2", &group2_id).unwrap();

        // === Session 2: Load both groups via metadata ===
        let provider2 = MlsProvider::new(&db_path).unwrap();

        let loaded1 = provider2.load_group_by_name("alice:group1").unwrap();
        let loaded2 = provider2.load_group_by_name("bob:group2").unwrap();

        assert_eq!(loaded1.unwrap(), group1_id, "Group 1 metadata should persist");
        assert_eq!(loaded2.unwrap(), group2_id, "Group 2 metadata should persist");
    }

    #[test]
    fn test_group_metadata_persists_during_activity() {
        use tempfile::tempdir;
        use crate::provider::MlsProvider;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("groups.db");

        // === Session 1: Create group and perform messaging operations ===
        let provider1 = MlsProvider::new(&db_path).unwrap();
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group_1 = create_group_with_config(&alice_cred, &alice_key, &provider1, "group").unwrap();
        let group_id_1 = alice_group_1.group_id().as_slice().to_vec();

        // Perform some group operations (send message)
        let msg = b"Hello from session 1";
        let _encrypted = create_application_message(&mut alice_group_1, &provider1, &alice_key, msg).unwrap();

        // Store group ID mapping in metadata
        provider1.save_group_name("alice:testgroup", &group_id_1).unwrap();

        // === Session 2: Verify metadata persists across sessions ===
        let provider2 = MlsProvider::new(&db_path).unwrap();

        // Load group ID from metadata - verify it persists even after messaging
        let loaded_id = provider2.load_group_by_name("alice:testgroup").unwrap();
        assert!(
            loaded_id.is_some(),
            "Group ID should persist in metadata after messaging activity"
        );

        let loaded_group_id = loaded_id.unwrap();
        assert_eq!(
            loaded_group_id,
            group_id_1,
            "Loaded group ID from metadata should match original group ID"
        );

        // Note: The OpenMLS storage provider will have persisted the actual group state,
        // but create_group_with_config() always creates a NEW group.
        // The key insight is that we use metadata storage to maintain the group ID mapping
        // so we know which group belongs to which (user, group_name) pair.
        // On reconnection, we use that metadata to know which group ID to work with.
    }

    #[test]
    fn test_load_group_from_storage_basic() {
        use tempfile::tempdir;
        use crate::provider::MlsProvider;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("groups.db");

        // === Session 1: Create group and persist state ===
        let provider1 = MlsProvider::new(&db_path).unwrap();
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group_1 = create_group_with_config(&alice_cred, &alice_key, &provider1, "group").unwrap();
        let group_id = alice_group_1.group_id().clone();

        // Send a message to modify state
        let msg = b"First message";
        let _encrypted_msg = create_application_message(&mut alice_group_1, &provider1, &alice_key, msg).unwrap();

        let epoch_1 = alice_group_1.epoch();

        // === Session 2: Load the group from storage ===
        let provider2 = MlsProvider::new(&db_path).unwrap();

        // Load the group using MlsGroup::load API
        let loaded_group = load_group_from_storage(&provider2, &group_id).unwrap();
        assert!(loaded_group.is_some(), "Group should exist in storage");

        let alice_group_2 = loaded_group.unwrap();

        // Verify the loaded group has the same state
        assert_eq!(
            alice_group_2.group_id(),
            &group_id,
            "Loaded group should have same group ID"
        );

        assert_eq!(
            alice_group_2.epoch(),
            epoch_1,
            "Loaded group should have same epoch (state persisted)"
        );
    }

    #[test]
    fn test_load_group_after_member_additions() {
        use tempfile::tempdir;
        use crate::provider::MlsProvider;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("groups.db");

        // === Session 1: Create group and add members ===
        let provider1 = MlsProvider::new(&db_path).unwrap();
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group_1 = create_group_with_config(&alice_cred, &alice_key, &provider1, "group").unwrap();
        let group_id = alice_group_1.group_id().clone();

        // Initial state - just Alice
        assert_eq!(alice_group_1.members().count(), 1, "Group should start with just Alice");

        // Generate key packages for Bob and Carol
        let (bob_cred, bob_key) = generate_credential_with_key("bob").unwrap();
        let bob_key_package = generate_key_package_bundle(&bob_cred, &bob_key, &provider1).unwrap();

        let (carol_cred, carol_key) = generate_credential_with_key("carol").unwrap();
        let carol_key_package = generate_key_package_bundle(&carol_cred, &carol_key, &provider1).unwrap();

        // Add Bob
        let (_commit_bob, _welcome_bob, _) = add_members(
            &mut alice_group_1,
            &provider1,
            &alice_key,
            &[bob_key_package.key_package()],
        )
        .unwrap();
        merge_pending_commit(&mut alice_group_1, &provider1).unwrap();

        // Add Carol
        let (_commit_carol, _welcome_carol, _) = add_members(
            &mut alice_group_1,
            &provider1,
            &alice_key,
            &[carol_key_package.key_package()],
        )
        .unwrap();
        merge_pending_commit(&mut alice_group_1, &provider1).unwrap();

        let epoch_after_adds = alice_group_1.epoch();

        // === Session 2: Load group and verify member state persists ===
        let provider2 = MlsProvider::new(&db_path).unwrap();

        let loaded_group = load_group_from_storage(&provider2, &group_id).unwrap();
        assert!(loaded_group.is_some(), "Group should exist in storage");

        let alice_group_2 = loaded_group.unwrap();

        // Verify group loaded correctly
        assert_eq!(alice_group_2.group_id(), &group_id, "Group ID should match");
        assert_eq!(alice_group_2.epoch(), epoch_after_adds, "Epoch should reflect member additions");
        assert_eq!(alice_group_2.members().count(), 3, "All members should be present after load");
    }

    #[test]
    fn test_load_group_across_multiple_sessions() {
        use tempfile::tempdir;
        use crate::provider::MlsProvider;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("groups.db");

        // === Session 1: Create group and add member (epoch advances) ===
        let provider1 = MlsProvider::new(&db_path).unwrap();
        let (alice_cred, alice_key) = generate_credential_with_key("alice").unwrap();
        let mut alice_group_1 = create_group_with_config(&alice_cred, &alice_key, &provider1, "group").unwrap();
        let group_id = alice_group_1.group_id().clone();
        let epoch_initial = alice_group_1.epoch();

        // Add Bob (epoch advances)
        let (bob_cred, bob_key) = generate_credential_with_key("bob").unwrap();
        let bob_key_package = generate_key_package_bundle(&bob_cred, &bob_key, &provider1).unwrap();
        let (_commit, _welcome, _) = add_members(
            &mut alice_group_1,
            &provider1,
            &alice_key,
            &[bob_key_package.key_package()],
        ).unwrap();
        merge_pending_commit(&mut alice_group_1, &provider1).unwrap();
        let epoch_1 = alice_group_1.epoch();

        // === Session 2: Load and add another member (epoch advances again) ===
        let provider2 = MlsProvider::new(&db_path).unwrap();
        let mut alice_group_2 = load_group_from_storage(&provider2, &group_id)
            .unwrap()
            .expect("Group should exist");

        assert_eq!(alice_group_2.epoch(), epoch_1, "Session 2 should load at session 1 epoch");
        assert!(epoch_1 > epoch_initial, "Epoch should have advanced after adding Bob");

        // Add Carol (epoch advances in session 2)
        let (carol_cred, carol_key) = generate_credential_with_key("carol").unwrap();
        let carol_key_package = generate_key_package_bundle(&carol_cred, &carol_key, &provider2).unwrap();
        let (_commit, _welcome, _) = add_members(
            &mut alice_group_2,
            &provider2,
            &alice_key,
            &[carol_key_package.key_package()],
        ).unwrap();
        merge_pending_commit(&mut alice_group_2, &provider2).unwrap();
        let epoch_2 = alice_group_2.epoch();

        // === Session 3: Verify all state persists ===
        let provider3 = MlsProvider::new(&db_path).unwrap();
        let alice_group_3 = load_group_from_storage(&provider3, &group_id)
            .unwrap()
            .expect("Group should exist");

        assert_eq!(alice_group_3.group_id(), &group_id, "Group ID persists");
        assert_eq!(alice_group_3.epoch(), epoch_2, "Session 3 should load at session 2 epoch");
        assert_eq!(alice_group_3.members().count(), 3, "All members should be present");
        assert!(epoch_2 > epoch_1, "Epoch should advance with member addition in session 2");
    }
}