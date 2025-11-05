//! Local storage for MLS client metadata
//!
//! This module handles only application-level metadata storage.
//! MLS group state is automatically managed by the OpenMlsProvider.
//! Member lists are derived from MlsGroup state, not stored separately.
//!
//! The KeyPackage pool metadata tracks lifecycle state and expiry information
//! for KeyPackages whose actual cryptographic material is stored by OpenMLS.

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

/// Metadata for a KeyPackage in the pool
///
/// Tracks lifecycle state, timestamps, and server synchronization info.
/// The actual KeyPackageBundle is stored by OpenMLS StorageProvider.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyPackageMetadata {
    /// Reference to the KeyPackage (hash of the KeyPackage)
    pub keypackage_ref: Vec<u8>,
    /// Lifecycle status: created, uploaded, available, reserved, spent, expired, failed
    pub status: String,
    /// Unix timestamp when the KeyPackage was created
    pub created_at: i64,
    /// Unix timestamp when uploaded to server (if applicable)
    pub uploaded_at: Option<i64>,
    /// Unix timestamp when reserved (if applicable)
    pub reserved_at: Option<i64>,
    /// Unix timestamp when spent (if applicable)
    pub spent_at: Option<i64>,
    /// Unix timestamp when the KeyPackage expires (from OpenMLS lifetime extension)
    pub not_after: i64,
    /// Server-assigned reservation ID (if reserved)
    pub reservation_id: Option<String>,
    /// Unix timestamp when reservation expires (if reserved)
    pub reservation_expires_at: Option<i64>,
    /// Username who reserved the key (if reserved)
    pub reserved_by: Option<String>,
    /// Group ID where the key was spent (if spent)
    pub spent_group_id: Option<Vec<u8>>,
    /// Username who spent the key (if spent)
    pub spent_by: Option<String>,
}

/// Local storage manager for SQLite database
///
/// Stores only application metadata (identities).
/// MLS group state is persisted transparently by the OpenMlsProvider.
pub struct LocalStore {
    conn: Connection,
}

impl LocalStore {
    /// Returns the current Unix timestamp in seconds.
    fn current_timestamp() -> Result<i64> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| crate::error::ClientError::Io(std::io::Error::other(
                format!("System time error: {}", e),
            )))
            .map(|d| d.as_secs() as i64)
    }

    /// Create a new local store with the given database path
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Self::initialize(&conn)?;
        Ok(Self { conn })
    }

    /// Initialize the database schema for application metadata
    fn initialize(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS identities (
                username TEXT PRIMARY KEY,
                public_key_blob BLOB NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS keypackage_pool_metadata (
                keypackage_ref BLOB PRIMARY KEY,
                status TEXT NOT NULL DEFAULT 'created',
                created_at INTEGER NOT NULL,
                uploaded_at INTEGER,
                reserved_at INTEGER,
                spent_at INTEGER,
                not_after INTEGER NOT NULL,
                reservation_id TEXT,
                reservation_expires_at INTEGER,
                reserved_by TEXT,
                spent_group_id BLOB,
                spent_by TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_status
                ON keypackage_pool_metadata(status);
            CREATE INDEX IF NOT EXISTS idx_expiry
                ON keypackage_pool_metadata(not_after);
            CREATE INDEX IF NOT EXISTS idx_created
                ON keypackage_pool_metadata(created_at);
            "#,
        )?;
        Ok(())
    }

    /// Save identity for a username with their public key
    ///
    /// The public key is used to look up the actual signature key in the OpenMLS provider storage.
    pub fn save_identity(&self, username: &str, public_key_blob: &[u8]) -> Result<()> {
        let created_at = chrono::Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT OR REPLACE INTO identities (username, public_key_blob, created_at) VALUES (?1, ?2, ?3)",
            (username, public_key_blob, created_at),
        )?;

        Ok(())
    }

    /// Load public key for a username (used for looking up stored signatures in OpenMLS provider)
    pub fn load_public_key(&self, username: &str) -> Result<Option<Vec<u8>>> {
        use rusqlite::OptionalExtension;
        let mut stmt = self.conn.prepare(
            "SELECT public_key_blob FROM identities WHERE username = ?1"
        )?;

        let result = stmt.query_row((username,), |row| {
            row.get::<_, Vec<u8>>(0)
        }).optional()?;

        Ok(result)
    }

    // ===== KeyPackage Pool Metadata Methods =====

    /// Create a new metadata entry for a KeyPackage
    ///
    /// The keypackage_ref is the hash reference from OpenMLS.
    /// Status is set to 'created' by default.
    pub fn create_pool_metadata(&self, keypackage_ref: &[u8], not_after: i64) -> Result<()> {
        let created_at = Self::current_timestamp()?;

        self.conn.execute(
            "INSERT INTO keypackage_pool_metadata (keypackage_ref, status, created_at, not_after)
             VALUES (?1, 'created', ?2, ?3)",
            (keypackage_ref, created_at, not_after),
        )?;

        Ok(())
    }

    /// Update the status of a KeyPackage in the pool
    ///
    /// Valid statuses: created, uploaded, available, reserved, spent, expired, failed
    pub fn update_pool_metadata_status(&self, keypackage_ref: &[u8], status: &str) -> Result<()> {
        let now = Self::current_timestamp()?;

        // Update the appropriate timestamp based on status
        let (timestamp_col, query) = match status {
            "uploaded" => ("uploaded_at", "UPDATE keypackage_pool_metadata SET status = ?1, uploaded_at = ?2 WHERE keypackage_ref = ?3"),
            "reserved" => ("reserved_at", "UPDATE keypackage_pool_metadata SET status = ?1, reserved_at = ?2 WHERE keypackage_ref = ?3"),
            "spent" => ("spent_at", "UPDATE keypackage_pool_metadata SET status = ?1, spent_at = ?2 WHERE keypackage_ref = ?3"),
            _ => ("", "UPDATE keypackage_pool_metadata SET status = ?1 WHERE keypackage_ref = ?2"),
        };

        if timestamp_col.is_empty() {
            self.conn.execute(query, (status, keypackage_ref))?;
        } else {
            self.conn.execute(query, (status, now, keypackage_ref))?;
        }

        Ok(())
    }

    /// Count KeyPackages by status
    pub fn count_by_status(&self, status: &str) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) FROM keypackage_pool_metadata WHERE status = ?1"
        )?;

        let count = stmt.query_row((status,), |row| row.get(0))?;
        Ok(count)
    }

    /// Get references to expired KeyPackages
    ///
    /// Returns KeyPackage refs where not_after < current_time
    pub fn get_expired_refs(&self, current_time: i64) -> Result<Vec<Vec<u8>>> {
        let mut stmt = self.conn.prepare(
            "SELECT keypackage_ref FROM keypackage_pool_metadata WHERE not_after < ?1"
        )?;

        let refs = stmt.query_map((current_time,), |row| {
            row.get::<_, Vec<u8>>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }

    /// Get all metadata entries for a given status
    pub fn get_metadata_by_status(&self, status: &str) -> Result<Vec<KeyPackageMetadata>> {
        let mut stmt = self.conn.prepare(
            "SELECT keypackage_ref, status, created_at, uploaded_at, reserved_at, spent_at,
                    not_after, reservation_id, reservation_expires_at, reserved_by,
                    spent_group_id, spent_by
             FROM keypackage_pool_metadata
             WHERE status = ?1"
        )?;

        let metadata = stmt.query_map((status,), |row| {
            Ok(KeyPackageMetadata {
                keypackage_ref: row.get(0)?,
                status: row.get(1)?,
                created_at: row.get(2)?,
                uploaded_at: row.get(3)?,
                reserved_at: row.get(4)?,
                spent_at: row.get(5)?,
                not_after: row.get(6)?,
                reservation_id: row.get(7)?,
                reservation_expires_at: row.get(8)?,
                reserved_by: row.get(9)?,
                spent_group_id: row.get(10)?,
                spent_by: row.get(11)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(metadata)
    }

    /// Delete a metadata entry for a KeyPackage
    pub fn delete_pool_metadata(&self, keypackage_ref: &[u8]) -> Result<()> {
        self.conn.execute(
            "DELETE FROM keypackage_pool_metadata WHERE keypackage_ref = ?1",
            (keypackage_ref,),
        )?;
        Ok(())
    }

    /// Update reservation information for a KeyPackage
    ///
    /// Called when server confirms a reservation
    pub fn update_reservation_info(
        &self,
        keypackage_ref: &[u8],
        reservation_id: &str,
        reserved_by: &str,
        expires_at: i64,
    ) -> Result<()> {
        let now = Self::current_timestamp()?;

        self.conn.execute(
            "UPDATE keypackage_pool_metadata
             SET status = 'reserved',
                 reserved_at = ?1,
                 reservation_id = ?2,
                 reserved_by = ?3,
                 reservation_expires_at = ?4
             WHERE keypackage_ref = ?5",
            (now, reservation_id, reserved_by, expires_at, keypackage_ref),
        )?;

        Ok(())
    }

    /// Mark a KeyPackage as spent
    ///
    /// Called when server confirms the key was consumed
    pub fn mark_spent(
        &self,
        keypackage_ref: &[u8],
        spent_by: &str,
        group_id: &[u8],
    ) -> Result<()> {
        let now = Self::current_timestamp()?;

        self.conn.execute(
            "UPDATE keypackage_pool_metadata
             SET status = 'spent',
                 spent_at = ?1,
                 spent_by = ?2,
                 spent_group_id = ?3
             WHERE keypackage_ref = ?4",
            (now, spent_by, group_id, keypackage_ref),
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_initialize_creates_tables() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = LocalStore::new(&db_path).unwrap();

        // Verify tables exist by querying them
        let mut stmt = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"identities".to_string()));
    }

    #[test]
    fn test_save_and_load_public_key() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = LocalStore::new(&db_path).unwrap();

        let public_key_blob = b"test_public_key";

        store.save_identity("alice", public_key_blob).unwrap();

        let loaded = store.load_public_key("alice").unwrap().unwrap();
        assert_eq!(loaded, public_key_blob);
    }

    #[test]
    fn test_load_nonexistent_public_key_returns_none() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = LocalStore::new(&db_path).unwrap();

        let result = store.load_public_key("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_multiple_users_same_db() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = LocalStore::new(&db_path).unwrap();

        // Save identities for multiple users
        store.save_identity("alice", b"alice_pubkey").unwrap();
        store.save_identity("bob", b"bob_pubkey").unwrap();

        // Verify both can be loaded
        let alice = store.load_public_key("alice").unwrap().unwrap();
        let bob = store.load_public_key("bob").unwrap().unwrap();

        assert_eq!(alice, b"alice_pubkey");
        assert_eq!(bob, b"bob_pubkey");
    }
}
