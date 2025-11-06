//! MLS User Identity Management
//!
//! This module encapsulates user identity and cryptographic credentials for MLS operations.
//! MlsUser represents a single user's identity that persists across all group memberships.
//!
//! ## Responsibility
//! - Owns user's signature key pair (persistent across sessions)
//! - Owns user's credential with public key (shared across all groups)
//! - Owns identity metadata (username, key material references)
//!
//! ## Design Principles
//! - **No External Dependencies**: MlsUser does not access LocalStore, MlsProvider, or ServerApi
//! - **Immutable Identity**: Once created, identity fields cannot be modified
//! - **Shared Across Groups**: Same credential_with_key is used for all group memberships
//! - **Single Responsibility**: Only manages identity, not group state or network operations
//!
//! ## Usage Pattern
//! ```rust
//! // Created by MlsConnection during initialization
//! let user = MlsUser::new(username, identity, signature_key, credential_with_key);
//!
//! // Passed to memberships for group operations
//! membership.send_message(text, &user);
//! membership.invite_user(invitee, &user, provider, api, store, websocket);
//! ```

use crate::models::Identity;

/// User identity for MLS operations
///
/// Represents a single user's identity that is shared across all group memberships.
/// Contains the cryptographic material needed for MLS operations (signature keys, credentials).
///
/// ## Fields
/// - `username`: Human-readable username (e.g., "alice")
/// - `identity`: Identity metadata (username + key references)
/// - `signature_key`: Long-term signature key pair (persistent across sessions)
/// - `credential_with_key`: MLS credential containing username and public key
///
/// ## Ownership Model
/// - MlsUser owns all fields directly (not Option<>)
/// - Created once during connection initialization
/// - Borrowed by memberships for operations (never moved)
///
/// ## Why These Fields Belong Together
/// - All fields represent the same logical identity
/// - signature_key and credential_with_key must always be paired (same key material)
/// - identity provides metadata about the same user
/// - All fields are invariant during a session (never change)
pub struct MlsUser {
    /// Username for this user (e.g., "alice")
    username: String,

    /// Identity metadata (username + key material references)
    identity: Identity,

    /// Persistent signature key pair (same across all groups and sessions)
    ///
    /// This is the long-term signing key that proves the user's identity.
    /// It is loaded from persistent storage or generated once per username.
    signature_key: openmls_basic_credential::SignatureKeyPair,

    /// MLS credential containing the user's identity and public key
    ///
    /// This is reused across all groups the user joins. The credential
    /// binds the username to the public key material from signature_key.
    credential_with_key: openmls::prelude::CredentialWithKey,
}

impl MlsUser {
    /// Create a new MlsUser with complete identity material
    ///
    /// All fields are required - partial construction is not supported.
    /// This ensures MlsUser always has a complete, valid identity.
    ///
    /// # Arguments
    /// * `username` - Human-readable username (e.g., "alice")
    /// * `identity` - Identity metadata structure
    /// * `signature_key` - Persistent signature key pair
    /// * `credential_with_key` - MLS credential (username + public key)
    ///
    /// # Example
    /// ```rust
    /// let user = MlsUser::new(
    ///     "alice".to_string(),
    ///     identity,
    ///     signature_key,
    ///     credential_with_key,
    /// );
    /// ```
    ///
    /// # Design Note
    /// Constructor takes ownership of all parameters. This is appropriate because:
    /// - MlsUser is created once and lives for the entire connection lifetime
    /// - Identity material should not be duplicated across multiple MlsUser instances
    /// - Ownership ensures clear lifecycle management
    pub fn new(
        username: String,
        identity: Identity,
        signature_key: openmls_basic_credential::SignatureKeyPair,
        credential_with_key: openmls::prelude::CredentialWithKey,
    ) -> Self {
        Self {
            username,
            identity,
            signature_key,
            credential_with_key,
        }
    }

    /// Get the username
    ///
    /// Returns a reference to avoid copying. The username is owned by MlsUser
    /// and should not be duplicated unnecessarily.
    pub fn get_username(&self) -> &str {
        &self.username
    }

    /// Get the identity metadata
    ///
    /// Returns a reference to the Identity structure containing username and
    /// key material references. This is primarily for compatibility with
    /// existing code that expects Identity.
    pub fn get_identity(&self) -> &Identity {
        &self.identity
    }

    /// Get the signature key pair
    ///
    /// Returns a reference to the persistent signature key. This key is used for:
    /// - Signing MLS messages (application messages, commits)
    /// - Creating key packages
    /// - Proving identity in MLS operations
    ///
    /// The key is never copied - always borrowed for operations.
    pub fn get_signature_key(&self) -> &openmls_basic_credential::SignatureKeyPair {
        &self.signature_key
    }

    /// Get the credential with key
    ///
    /// Returns a reference to the MLS credential (username + public key).
    /// This credential is:
    /// - Shared across all groups the user joins
    /// - Used when creating groups or joining via Welcome
    /// - Validated by other members during group operations
    ///
    /// The credential is never duplicated - always borrowed.
    pub fn get_credential_with_key(&self) -> &openmls::prelude::CredentialWithKey {
        &self.credential_with_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;

    /// Test that MlsUser can be created with valid identity material
    ///
    /// Verifies:
    /// - Constructor accepts all required fields
    /// - MlsUser is created successfully
    /// - No panics or errors during construction
    #[test]
    fn test_mls_user_creation() {
        // Generate identity material (same as in client.rs initialization)
        let username = "alice";
        let (credential_with_key, signature_key) =
            crypto::generate_credential_with_key(username).expect("Failed to generate credential");

        // Create identity metadata
        let keypair_blob = signature_key.to_public_vec();
        let identity = Identity {
            username: username.to_string(),
            keypair_blob: keypair_blob.clone(),
            credential_blob: vec![], // Not used - regenerated from username
        };

        // Create MlsUser
        let user = MlsUser::new(
            username.to_string(),
            identity,
            signature_key,
            credential_with_key,
        );

        // Verify construction succeeded
        assert_eq!(user.get_username(), username);
    }

    /// Test that all MlsUser getters return correct values
    ///
    /// Verifies:
    /// - get_username() returns the original username
    /// - get_identity() returns the identity metadata
    /// - get_signature_key() returns a valid signature key
    /// - get_credential_with_key() returns a valid credential
    /// - All getters return references (not copies)
    #[test]
    fn test_mls_user_getters() {
        let username = "bob";
        let (credential_with_key, signature_key) =
            crypto::generate_credential_with_key(username).expect("Failed to generate credential");

        // Store public key bytes for later comparison
        let public_key_bytes = signature_key.to_public_vec();
        let keypair_blob = public_key_bytes.clone();

        let identity = Identity {
            username: username.to_string(),
            keypair_blob: keypair_blob.clone(),
            credential_blob: vec![],
        };

        let user = MlsUser::new(
            username.to_string(),
            identity.clone(),
            signature_key,
            credential_with_key,
        );

        // Test username getter
        assert_eq!(user.get_username(), username);

        // Test identity getter
        assert_eq!(user.get_identity().username, username);
        assert_eq!(user.get_identity().keypair_blob, keypair_blob);

        // Test signature_key getter - compare public key bytes
        let retrieved_sig_key = user.get_signature_key();
        assert_eq!(
            retrieved_sig_key.to_public_vec(),
            public_key_bytes,
            "Signature key public bytes should match original"
        );

        // Test credential_with_key getter
        let retrieved_credential = user.get_credential_with_key();
        // Compare credentials by checking the identity (BasicCredential)
        use openmls::prelude::*;
        let retrieved_basic = BasicCredential::try_from(retrieved_credential.credential.clone())
            .expect("Retrieved credential should be BasicCredential");

        assert_eq!(
            retrieved_basic.identity(),
            username.as_bytes(),
            "Credential identity should match username"
        );
    }

    /// Test that signature key is retained across operations
    ///
    /// Verifies:
    /// - Signature key can be retrieved multiple times
    /// - Key material remains consistent
    /// - No accidental mutations or copies
    ///
    /// This is important because signature keys must be persistent
    /// across all group operations for the same user.
    #[test]
    fn test_signature_key_persistence() {
        let username = "carol";
        let (credential_with_key, signature_key) =
            crypto::generate_credential_with_key(username).expect("Failed to generate credential");

        // Store public key bytes before moving signature_key into MlsUser
        let original_public_bytes = signature_key.to_public_vec();

        let identity = Identity {
            username: username.to_string(),
            keypair_blob: original_public_bytes.clone(),
            credential_blob: vec![],
        };

        let user = MlsUser::new(
            username.to_string(),
            identity,
            signature_key,
            credential_with_key,
        );

        // Retrieve signature key multiple times
        let key1 = user.get_signature_key();
        let key2 = user.get_signature_key();

        // Verify both retrievals return the same key material
        assert_eq!(
            key1.to_public_vec(),
            key2.to_public_vec(),
            "Multiple retrievals should return the same key"
        );

        // Verify it matches the original public bytes
        assert_eq!(
            key1.to_public_vec(),
            original_public_bytes,
            "Retrieved key should match original public bytes"
        );
    }

    /// Test that MlsUser fields are immutable
    ///
    /// Verifies:
    /// - Username cannot be changed after construction
    /// - Identity material cannot be modified
    /// - MlsUser enforces immutability
    ///
    /// This ensures identity consistency across all operations.
    #[test]
    fn test_mls_user_immutability() {
        let username = "dave";
        let (credential_with_key, signature_key) =
            crypto::generate_credential_with_key(username).expect("Failed to generate credential");

        let identity = Identity {
            username: username.to_string(),
            keypair_blob: signature_key.to_public_vec(),
            credential_blob: vec![],
        };

        let user = MlsUser::new(
            username.to_string(),
            identity,
            signature_key,
            credential_with_key,
        );

        // Verify username hasn't changed
        let original_username = user.get_username();
        let later_username = user.get_username();
        assert_eq!(original_username, later_username);

        // Verify no methods allow mutation (compiler enforces this via &self)
        // This test documents the design intent - Rust enforces it automatically
    }
}
