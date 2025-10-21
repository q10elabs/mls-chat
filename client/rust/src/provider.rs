/// OpenMLS provider implementation using SQLite for persistence
///
/// This module implements the OpenMlsProvider trait, integrating:
/// - RustCrypto for cryptographic operations
/// - SqliteStorageProvider for persistent group state
/// - Automatic serialization/deserialization of MLS state

use crate::error::{Result, ClientError};
use openmls::prelude::*;
use openmls_rust_crypto::RustCrypto;
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

/// Binary codec for efficient serialization
#[derive(Default)]
pub struct BincodeCodec;

impl openmls_sqlite_storage::Codec for BincodeCodec {
    type Error = bincode::Error;

    fn to_vec<T: Serialize>(value: &T) -> std::result::Result<Vec<u8>, Self::Error> {
        bincode::serialize(value)
    }

    fn from_slice<T: serde::de::DeserializeOwned>(slice: &[u8]) -> std::result::Result<T, Self::Error> {
        bincode::deserialize(slice)
    }
}

/// OpenMLS provider combining cryptography, randomness, and storage
pub struct MlsProvider {
    crypto: RustCrypto,
    storage: SqliteStorageProvider<BincodeCodec, Connection>,
}

impl MlsProvider {
    /// Create a new provider with file-based SQLite storage
    ///
    /// # Arguments
    /// * `db_path` - Path to the SQLite database file
    ///
    /// # Errors
    /// * Database connection errors
    /// * Migration errors during initialization
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let connection = Connection::open(db_path)?;
        let mut storage = SqliteStorageProvider::<BincodeCodec, Connection>::new(connection);

        // Run migrations to initialize schema
        storage
            .run_migrations()
            .map_err(|e| ClientError::Config(format!("Migration error: {}", e)))?;

        Ok(Self {
            crypto: RustCrypto::default(),
            storage,
        })
    }

    /// Create a new provider with in-memory SQLite storage (testing only)
    ///
    /// # Errors
    /// * Migration errors during initialization
    pub fn new_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        let mut storage = SqliteStorageProvider::<BincodeCodec, Connection>::new(connection);

        // Run migrations to initialize schema
        storage
            .run_migrations()
            .map_err(|e| ClientError::Config(format!("Migration error: {}", e)))?;

        Ok(Self {
            crypto: RustCrypto::default(),
            storage,
        })
    }
}

impl OpenMlsProvider for MlsProvider {
    type CryptoProvider = RustCrypto;
    type RandProvider = RustCrypto;
    type StorageProvider = SqliteStorageProvider<BincodeCodec, Connection>;

    fn storage(&self) -> &Self::StorageProvider {
        &self.storage
    }

    fn crypto(&self) -> &Self::CryptoProvider {
        &self.crypto
    }

    fn rand(&self) -> &Self::RandProvider {
        &self.crypto
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_in_memory_provider() {
        let _provider = MlsProvider::new_in_memory().unwrap();
        // Provider created successfully with in-memory storage
    }

    #[test]
    fn test_create_file_based_provider() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let _provider = MlsProvider::new(&db_path).unwrap();
        // Provider created successfully with file-based storage
    }
}
