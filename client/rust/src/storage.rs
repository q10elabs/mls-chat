/// Local storage for MLS client metadata
///
/// This module handles only application-level metadata storage.
/// MLS group state is automatically managed by the OpenMlsProvider.

use crate::error::Result;
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

/// Local storage manager for SQLite database
///
/// Stores only application metadata (identities, group members).
/// MLS group state is persisted transparently by the OpenMlsProvider.
pub struct LocalStore {
    conn: Connection,
}

impl LocalStore {
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

            CREATE TABLE IF NOT EXISTS group_members (
                username TEXT NOT NULL,
                group_id TEXT NOT NULL,
                members_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (username, group_id)
            );

            CREATE INDEX IF NOT EXISTS idx_group_members_user ON group_members(username);
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
        let mut stmt = self.conn.prepare(
            "SELECT public_key_blob FROM identities WHERE username = ?1"
        )?;

        let result = stmt.query_row((username,), |row| {
            row.get::<_, Vec<u8>>(0)
        }).optional()?;

        Ok(result)
    }

    /// Save group members for a username and group
    pub fn save_group_members(&self, username: &str, group_id: &str, members: &[String]) -> Result<()> {
        let updated_at = chrono::Utc::now().to_rfc3339();
        let members_json = serde_json::to_string(members)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO group_members (username, group_id, members_json, updated_at) VALUES (?1, ?2, ?3, ?4)",
            (username, group_id, members_json, updated_at),
        )?;

        Ok(())
    }

    /// Get group members for a username and group
    pub fn get_group_members(&self, username: &str, group_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT members_json FROM group_members WHERE username = ?1 AND group_id = ?2"
        )?;

        let result = stmt.query_row((username, group_id), |row| {
            let json: String = row.get(0)?;
            let members: Vec<String> = serde_json::from_str(&json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
            Ok(members)
        }).optional()?;

        Ok(result.unwrap_or_default())
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
        assert!(tables.contains(&"group_members".to_string()));
        // Note: group_states table is no longer used (OpenMlsProvider handles it)
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
    fn test_save_and_get_group_members() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = LocalStore::new(&db_path).unwrap();

        let members = vec!["alice".to_string(), "bob".to_string()];
        store.save_group_members("alice", "group1", &members).unwrap();

        let loaded = store.get_group_members("alice", "group1").unwrap();
        assert_eq!(loaded, members);
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
