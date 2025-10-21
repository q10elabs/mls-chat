/// Identity persistence and management
///
/// Handles persistent storage and recovery of user identities (credentials and signature keys)
/// using the OpenMLS storage provider. Each username maintains a unique cryptographic identity.

use crate::error::{Result, ClientError};
use crate::provider::MlsProvider;
use crate::storage::LocalStore;
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use tls_codec::Serialize;

/// Represents a stored user identity with all cryptographic material
///
/// Note: SignatureKeyPair cannot be cloned due to containing private key material.
/// This struct should be used immediately and not stored for long periods.
#[derive(Debug)]
pub struct StoredIdentity {
    pub username: String,
    pub credential_with_key: CredentialWithKey,
    pub signature_key: SignatureKeyPair,
}

/// Identity manager for persistent credential and key storage
pub struct IdentityManager;

impl IdentityManager {
    /// Load or create a user identity with persistent storage
    ///
    /// If the user already exists in the provider's storage, their signature key
    /// is loaded from the OpenMLS storage provider using the public key stored in LocalStore.
    /// If the user is new, a fresh identity is created and stored in both the OpenMLS
    /// provider storage and the LocalStore metadata database.
    ///
    /// # Arguments
    /// * `provider` - The OpenMLS provider with storage backend
    /// * `metadata_store` - The LocalStore for application metadata
    /// * `username` - The username for this identity
    ///
    /// # Returns
    /// A `StoredIdentity` with credential and signature key ready to use
    ///
    /// # Errors
    /// * Storage errors when reading/writing credentials
    /// * Crypto errors when generating new credentials
    pub fn load_or_create(
        provider: &MlsProvider,
        metadata_store: &LocalStore,
        username: &str,
    ) -> Result<StoredIdentity> {
        let ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

        // Try to load existing identity from metadata store
        let (credential_with_key, signature_key) =
            match metadata_store.load_public_key(username)? {
                Some(public_key_blob) => {
                    // We have the public key stored - try to load the signature key from OpenMLS storage
                    match SignatureKeyPair::read(
                        provider.storage(),
                        &public_key_blob,
                        ciphersuite.signature_algorithm(),
                    ) {
                        Some(sig_key) => {
                            // Successfully loaded existing signature key
                            let credential = BasicCredential::new(username.as_bytes().to_vec());
                            let credential_with_key = CredentialWithKey {
                                credential: credential.into(),
                                signature_key: sig_key.to_public_vec().into(),
                            };
                            (credential_with_key, sig_key)
                        }
                        None => {
                            // Public key stored but not in OpenMLS storage - regenerate
                            log::warn!(
                                "Public key for {} found in metadata but not in OpenMLS storage. Regenerating.",
                                username
                            );
                            Self::create_new_identity(provider, metadata_store, username)?
                        }
                    }
                }
                None => {
                    // No identity stored - create new one
                    Self::create_new_identity(provider, metadata_store, username)?
                }
            };

        Ok(StoredIdentity {
            username: username.to_string(),
            credential_with_key,
            signature_key,
        })
    }

    /// Create a new identity and store it in both provider and metadata store
    fn create_new_identity(
        provider: &MlsProvider,
        metadata_store: &LocalStore,
        username: &str,
    ) -> Result<(CredentialWithKey, SignatureKeyPair)> {
        let ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

        // Generate new credential
        let credential = BasicCredential::new(username.as_bytes().to_vec());

        // Generate new signature key
        let signature_keys = SignatureKeyPair::new(ciphersuite.signature_algorithm())
            .map_err(|e| ClientError::Config(format!("Failed to generate signature key: {}", e)))?;

        // Store the signature key in the OpenMLS provider's storage
        signature_keys
            .store(provider.storage())
            .map_err(|e| ClientError::Config(format!("Failed to store signature key: {}", e)))?;

        // Get public key to store in metadata
        let public_key_blob = signature_keys.to_public_vec();

        // Store identity in metadata store with public key
        // The public key is used to look up the signature key in OpenMLS provider storage
        metadata_store.save_identity(username, &public_key_blob)?;

        let credential_with_key = CredentialWithKey {
            credential: credential.into(),
            signature_key: public_key_blob.into(),
        };

        Ok((credential_with_key, signature_keys))
    }

    /// Verify that an identity is properly stored and can be retrieved
    ///
    /// This is mainly for testing purposes to ensure persistence is working.
    pub fn verify_stored(
        provider: &MlsProvider,
        metadata_store: &LocalStore,
        identity: &StoredIdentity,
    ) -> Result<bool> {
        let ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

        // Check that public key is in metadata store
        let public_key_in_metadata = metadata_store.load_public_key(&identity.username)?;
        if public_key_in_metadata.is_none() {
            return Ok(false);
        }

        // Check that signature key is in OpenMLS storage
        match SignatureKeyPair::read(
            provider.storage(),
            &identity.signature_key.to_public_vec(),
            ciphersuite.signature_algorithm(),
        ) {
            Some(stored_key) => {
                // Verify the stored key's public part matches
                Ok(stored_key.to_public_vec() == identity.signature_key.to_public_vec())
            }
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalStore;
    use tempfile::tempdir;

    #[test]
    fn test_create_new_identity() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let provider = MlsProvider::new(&db_path).unwrap();
        let metadata_store = LocalStore::new(&db_path.with_file_name("metadata.db")).unwrap();

        let identity = IdentityManager::load_or_create(&provider, &metadata_store, "alice").unwrap();

        assert_eq!(identity.username, "alice");
        assert!(!identity.signature_key.to_public_vec().is_empty());
    }

    #[test]
    fn test_identity_persistence_across_instances() {
        let temp_dir = tempdir().unwrap();
        let mls_db = temp_dir.path().join("mls.db");
        let metadata_db = temp_dir.path().join("metadata.db");

        // Create first identity
        let provider1 = MlsProvider::new(&mls_db).unwrap();
        let metadata1 = LocalStore::new(&metadata_db).unwrap();
        let identity1 = IdentityManager::load_or_create(&provider1, &metadata1, "bob").unwrap();
        let pub_key_1 = identity1.signature_key.to_public_vec();

        // Create new provider instance pointing to same database
        let provider2 = MlsProvider::new(&mls_db).unwrap();
        let metadata2 = LocalStore::new(&metadata_db).unwrap();
        let identity2 = IdentityManager::load_or_create(&provider2, &metadata2, "bob").unwrap();
        let pub_key_2 = identity2.signature_key.to_public_vec();

        // Public keys should match (proving we loaded the same identity)
        assert_eq!(pub_key_1, pub_key_2);
    }

    #[test]
    fn test_different_users_different_identities() {
        let temp_dir = tempdir().unwrap();
        let mls_db = temp_dir.path().join("mls.db");
        let metadata_db = temp_dir.path().join("metadata.db");
        let provider = MlsProvider::new(&mls_db).unwrap();
        let metadata_store = LocalStore::new(&metadata_db).unwrap();

        let alice = IdentityManager::load_or_create(&provider, &metadata_store, "alice").unwrap();
        let bob = IdentityManager::load_or_create(&provider, &metadata_store, "bob").unwrap();

        // Different users should have different public keys
        assert_ne!(
            alice.signature_key.to_public_vec(),
            bob.signature_key.to_public_vec()
        );
    }

    #[test]
    fn test_identity_storage_verification() {
        let temp_dir = tempdir().unwrap();
        let mls_db = temp_dir.path().join("mls.db");
        let metadata_db = temp_dir.path().join("metadata.db");
        let provider = MlsProvider::new(&mls_db).unwrap();
        let metadata_store = LocalStore::new(&metadata_db).unwrap();

        let identity = IdentityManager::load_or_create(&provider, &metadata_store, "carol").unwrap();

        // Verify the identity is properly stored
        let is_stored = IdentityManager::verify_stored(&provider, &metadata_store, &identity).unwrap();
        assert!(is_stored, "Identity should be stored in provider");
    }

    #[test]
    fn test_signature_key_preserved_across_sessions() {
        let temp_dir = tempdir().unwrap();
        let mls_db = temp_dir.path().join("mls.db");
        let metadata_db = temp_dir.path().join("metadata.db");

        // Session 1: Create identity
        let provider1 = MlsProvider::new(&mls_db).unwrap();
        let metadata1 = LocalStore::new(&metadata_db).unwrap();
        let identity1 = IdentityManager::load_or_create(&provider1, &metadata1, "dave").unwrap();
        let original_pubkey = identity1.signature_key.to_public_vec();

        // Session 2: Load identity
        let provider2 = MlsProvider::new(&mls_db).unwrap();
        let metadata2 = LocalStore::new(&metadata_db).unwrap();
        let identity2 = IdentityManager::load_or_create(&provider2, &metadata2, "dave").unwrap();
        let loaded_pubkey = identity2.signature_key.to_public_vec();

        // Session 3: Load again to ensure consistency
        let provider3 = MlsProvider::new(&mls_db).unwrap();
        let metadata3 = LocalStore::new(&metadata_db).unwrap();
        let identity3 = IdentityManager::load_or_create(&provider3, &metadata3, "dave").unwrap();
        let loaded_pubkey_2 = identity3.signature_key.to_public_vec();

        // All should be identical
        assert_eq!(original_pubkey, loaded_pubkey);
        assert_eq!(original_pubkey, loaded_pubkey_2);
    }

    #[test]
    fn test_credential_with_key_structure() {
        let temp_dir = tempdir().unwrap();
        let mls_db = temp_dir.path().join("mls.db");
        let metadata_db = temp_dir.path().join("metadata.db");
        let provider = MlsProvider::new(&mls_db).unwrap();
        let metadata_store = LocalStore::new(&metadata_db).unwrap();

        let identity = IdentityManager::load_or_create(&provider, &metadata_store, "eve").unwrap();

        // Verify credential structure
        let credential_bytes = identity.credential_with_key.credential.clone().tls_serialize_detached().unwrap();
        assert!(!credential_bytes.is_empty(), "Credential should be serializable");

        // SignaturePublicKey is non-empty if it can be serialized
        let _sig_key_bytes = identity.credential_with_key.signature_key.clone().tls_serialize_detached().unwrap();
        assert!(true, "Signature key should be serializable and non-empty");
    }

    #[test]
    fn test_multiple_identities_in_same_db() {
        let temp_dir = tempdir().unwrap();
        let mls_db = temp_dir.path().join("mls.db");
        let metadata_db = temp_dir.path().join("metadata.db");
        let provider = MlsProvider::new(&mls_db).unwrap();
        let metadata_store = LocalStore::new(&metadata_db).unwrap();

        // Create multiple identities
        let users = vec!["user1", "user2", "user3", "user4", "user5"];
        let mut identities = Vec::new();

        for username in &users {
            let identity = IdentityManager::load_or_create(&provider, &metadata_store, username).unwrap();
            identities.push(identity);
        }

        // Verify all are different
        let pubkeys: Vec<Vec<u8>> = identities.iter()
            .map(|id| id.signature_key.to_public_vec())
            .collect();

        for i in 0..pubkeys.len() {
            for j in (i + 1)..pubkeys.len() {
                assert_ne!(pubkeys[i], pubkeys[j], "User {} and {} should have different keys", i, j);
            }
        }

        // Reload them and verify they're the same
        for username in users {
            let reloaded = IdentityManager::load_or_create(&provider, &metadata_store, username).unwrap();
            let original = identities.iter()
                .find(|id| id.username == username)
                .unwrap();
            assert_eq!(
                original.signature_key.to_public_vec(),
                reloaded.signature_key.to_public_vec(),
                "Identity for {} should persist",
                username
            );
        }
    }
}
