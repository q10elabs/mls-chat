//! KeyPackage pool management primitives.
//!
//! Provides configuration and lifecycle operations for generating,
//! tracking, and cleaning up KeyPackages using the OpenMLS storage
//! provider together with LocalStore metadata.

use std::time::{SystemTime, UNIX_EPOCH};

use log::{debug, warn};
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_traits::storage::traits as storage_traits;
use openmls_traits::storage::{self, StorageProvider};
use serde::{Deserialize, Serialize};

use crate::error::{MlsError, Result};
use crate::storage::LocalStore;

/// Describes thresholds for managing the KeyPackage pool.
#[derive(Debug, Clone)]
pub struct KeyPackagePoolConfig {
    pub target_pool_size: usize,
    pub low_watermark: usize,
    pub hard_cap: usize,
}

impl Default for KeyPackagePoolConfig {
    fn default() -> Self {
        Self {
            target_pool_size: 32,
            low_watermark: 8,
            hard_cap: 64,
        }
    }
}

/// Manages KeyPackage lifecycle for a given user.
pub struct KeyPackagePool<'a> {
    username: String,
    config: KeyPackagePoolConfig,
    store: &'a LocalStore,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredKeyPackageRef(Vec<u8>);

impl storage_traits::HashReference<{ storage::CURRENT_VERSION }> for StoredKeyPackageRef {}

impl storage::Key<{ storage::CURRENT_VERSION }> for StoredKeyPackageRef {}

impl<'a> KeyPackagePool<'a> {
    /// Create a new pool manager for the given user.
    pub fn new<S: Into<String>>(
        username: S,
        config: KeyPackagePoolConfig,
        store: &'a LocalStore,
    ) -> Self {
        Self {
            username: username.into(),
            config,
            store,
        }
    }

    /// Generate `count` new KeyPackages, enforcing the pool hard cap.
    pub async fn generate_and_update_pool(
        &self,
        count: usize,
        credential: &CredentialWithKey,
        signer: &SignatureKeyPair,
        provider: &impl OpenMlsProvider,
    ) -> Result<Vec<Vec<u8>>> {
        let unspent_now = self.get_unspent_count()?;
        if unspent_now + count > self.config.hard_cap {
            return Err(MlsError::PoolCapacityExceeded {
                needed: count,
                available: self.config.hard_cap.saturating_sub(unspent_now),
            }
            .into());
        }

        let mut generated_refs = Vec::with_capacity(count);
        let ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

        for _ in 0..count {
            let bundle = KeyPackage::builder()
                .build(ciphersuite, provider, signer, credential.clone())
                .map_err(|e| MlsError::OpenMls(e.to_string()))?;

            let key_package = bundle.key_package();
            let hash_ref = key_package
                .hash_ref(provider.crypto())
                .map_err(|e| MlsError::OpenMls(e.to_string()))?
                .as_slice()
                .to_vec();

            let lifetime = key_package.life_time();
            let not_after = lifetime.not_after() as i64;

            self.store.create_pool_metadata(&hash_ref, not_after)?;

            generated_refs.push(hash_ref);
        }

        debug!(
            "Generated {} key packages for user {}",
            generated_refs.len(),
            self.username
        );

        Ok(generated_refs)
    }

    /// Count the number of available KeyPackages.
    pub fn get_available_count(&self) -> Result<usize> {
        self.store.count_by_status("available")
    }

    /// Count KeyPackages that remain locally and are not yet consumed.
    fn get_unspent_count(&self) -> Result<usize> {
        let available = self.get_available_count()?;
        let created = self.store.count_by_status("created")?;
        Ok(available + created)
    }

    /// Determine if the pool should be replenished.
    pub fn should_replenish(&self) -> Result<bool> {
        Ok(self.get_available_count()? < self.config.low_watermark)
    }

    /// Calculate how many KeyPackages are required to reach the target size.
    pub fn get_replenishment_needed(&self) -> Result<Option<usize>> {
        let available = self.get_available_count()?;
        if available >= self.config.target_pool_size {
            return Ok(None);
        }

        let needed = self.config.target_pool_size - available;
        if needed == 0 {
            Ok(None)
        } else {
            Ok(Some(needed))
        }
    }

    /// Mark a KeyPackage as spent in metadata.
    pub fn mark_as_spent(&self, keypackage_ref: &[u8]) -> Result<()> {
        self.store
            .update_pool_metadata_status(keypackage_ref, "spent")
    }

    /// Remove expired KeyPackages from both storage layers.
    pub fn cleanup_expired(
        &self,
        provider: &impl OpenMlsProvider,
        current_time: SystemTime,
    ) -> Result<usize> {
        let now = current_time
            .duration_since(UNIX_EPOCH)
            .map_err(|e| MlsError::OpenMls(e.to_string()))?
            .as_secs() as i64;

        let expired_refs = self.store.get_expired_refs(now)?;
        let mut removed = 0;

        for ref_hash in expired_refs {
            let kp_ref = StoredKeyPackageRef(ref_hash.clone());

            if let Err(err) = provider.storage().delete_key_package(&kp_ref) {
                warn!(
                    "Failed to delete expired key package from OpenMLS storage for user {}: {}",
                    self.username, err
                );
                continue;
            }

            if let Err(err) = self.store.delete_pool_metadata(&ref_hash) {
                warn!(
                    "Failed to delete expired key package metadata for user {}: {}",
                    self.username, err
                );
                continue;
            }

            removed += 1;
        }

        Ok(removed)
    }
}
