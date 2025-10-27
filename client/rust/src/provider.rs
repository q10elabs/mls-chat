//! OpenMLS provider implementation using SQLite for persistence
//!
//! This module implements the OpenMlsProvider trait, integrating:
//! - RustCrypto for cryptographic operations
//! - SqliteStorageProvider for persistent group state
//! - Automatic serialization/deserialization of MLS state

use crate::error::{Result, ClientError};
use openmls::prelude::*;
use openmls_rust_crypto::RustCrypto;
use openmls_sqlite_storage::SqliteStorageProvider;
use rusqlite::{Connection, OptionalExtension};
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
    conn: Connection,
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
        let path_buf = db_path.as_ref().to_path_buf();

        let connection = Connection::open(&path_buf)?;
        let mut storage = SqliteStorageProvider::<BincodeCodec, Connection>::new(connection);

        // Run migrations to initialize schema
        storage
            .run_migrations()
            .map_err(|e| ClientError::Config(format!("Migration error: {}", e)))?;

        // Initialize metadata tables for group name mapping
        let conn = Connection::open(&path_buf)?;
        Self::initialize_metadata_tables(&conn)?;

        Ok(Self {
            crypto: RustCrypto::default(),
            storage,
            conn,
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

        let conn = Connection::open_in_memory()?;
        Self::initialize_metadata_tables(&conn)?;

        Ok(Self {
            crypto: RustCrypto::default(),
            storage,
            conn,
        })
    }

    /// Initialize metadata tables for group name mappings
    fn initialize_metadata_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS group_names (
                group_name_key TEXT PRIMARY KEY,
                group_id BLOB NOT NULL,
                created_at TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    /// Save a mapping from group name key to group ID
    pub fn save_group_name(&self, group_name_key: &str, group_id: &[u8]) -> Result<()> {
        let created_at = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO group_names (group_name_key, group_id, created_at) VALUES (?1, ?2, ?3)",
            (group_name_key, group_id, created_at),
        )?;
        Ok(())
    }

    /// Check if a group name mapping exists
    pub fn group_exists(&self, group_name_key: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT 1 FROM group_names WHERE group_name_key = ?1 LIMIT 1"
        )?;

        let exists = stmt.exists((group_name_key,))?;
        Ok(exists)
    }

    /// Load a group by its name key
    /// Note: This just checks if a group ID mapping exists; the actual group state
    /// is managed by the OpenMLS provider's storage
    pub fn load_group_by_name(&self, group_name_key: &str) -> Result<Option<Vec<u8>>> {
        let mut stmt = self.conn.prepare(
            "SELECT group_id FROM group_names WHERE group_name_key = ?1"
        )?;

        let group_id_opt = stmt.query_row((group_name_key,), |row| {
            row.get::<_, Vec<u8>>(0)
        }).optional()?;

        Ok(group_id_opt)
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
