/// KeyPackage pool storage and management for server-side tracking.
/// Handles storage, reservation, spend, and expiry of KeyPackages with double-spend prevention.
///
/// This module provides:
/// - KeyPackage storage with lifecycle status tracking (available, reserved, spent)
/// - Reservation system with TTL (60s timeout)
/// - Double-spend prevention via status validation
/// - Expiry-based garbage collection
/// - Pool health queries
use rusqlite::{params, OptionalExtension, Result as SqliteResult};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use super::DbPool;

/// Time-to-live for reservations (60 seconds)
const RESERVATION_TTL_SECONDS: i64 = 60;

/// KeyPackage lifecycle status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyPackageStatus {
    Available,
    Reserved,
    Spent,
}

impl KeyPackageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyPackageStatus::Available => "available",
            KeyPackageStatus::Reserved => "reserved",
            KeyPackageStatus::Spent => "spent",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "available" => Some(KeyPackageStatus::Available),
            "reserved" => Some(KeyPackageStatus::Reserved),
            "spent" => Some(KeyPackageStatus::Spent),
            _ => None,
        }
    }
}

/// Metadata for a stored KeyPackage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPackageMetadata {
    pub keypackage_ref: Vec<u8>,
    pub username: String,
    pub not_after: i64,
    pub status: KeyPackageStatus,
    pub credential_hash: Option<Vec<u8>>,
    pub ciphersuite: Option<i64>,
}

/// Complete KeyPackage data including the serialized bytes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPackageData {
    pub keypackage_ref: Vec<u8>,
    pub username: String,
    pub keypackage_bytes: Vec<u8>,
    pub uploaded_at: i64,
    pub status: KeyPackageStatus,
    pub not_after: i64,
    pub credential_hash: Option<Vec<u8>>,
    pub ciphersuite: Option<i64>,
}

/// Reserved KeyPackage returned to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReservedKeyPackage {
    pub keypackage_ref: Vec<u8>,
    pub keypackage_bytes: Vec<u8>,
    pub reservation_id: String,
    pub reservation_expires_at: i64,
    pub not_after: i64,
}

/// KeyPackage pool storage operations
pub struct KeyPackageStore;

impl KeyPackageStore {
    /// Initialize the keypackages table schema
    pub async fn initialize_schema(pool: &DbPool) -> SqliteResult<()> {
        let conn = pool.lock().await;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS keypackages (
                keypackage_ref BLOB NOT NULL,
                username TEXT NOT NULL,
                keypackage_bytes BLOB NOT NULL,
                uploaded_at INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'available',
                reservation_id TEXT UNIQUE,
                reservation_expires_at INTEGER,
                reserved_by TEXT,
                spent_at INTEGER,
                spent_by TEXT,
                group_id BLOB,
                not_after INTEGER NOT NULL,
                credential_hash BLOB,
                ciphersuite INTEGER,
                PRIMARY KEY (username, keypackage_ref)
            );

            CREATE INDEX IF NOT EXISTS idx_user_status ON keypackages(username, status);
            CREATE INDEX IF NOT EXISTS idx_user_expiry ON keypackages(username, not_after);
            CREATE INDEX IF NOT EXISTS idx_reservation ON keypackages(reservation_id);
            "#,
        )?;

        Ok(())
    }

    /// Save a new KeyPackage to the pool
    pub async fn save_key_package(
        pool: &DbPool,
        username: &str,
        keypackage_ref: &[u8],
        keypackage_bytes: &[u8],
        not_after: i64,
        credential_hash: Option<&[u8]>,
        ciphersuite: Option<i64>,
    ) -> SqliteResult<()> {
        let conn = pool.lock().await;
        let uploaded_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO keypackages (keypackage_ref, username, keypackage_bytes, uploaded_at, status, not_after, credential_hash, ciphersuite)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                keypackage_ref,
                username,
                keypackage_bytes,
                uploaded_at,
                KeyPackageStatus::Available.as_str(),
                not_after,
                credential_hash,
                ciphersuite,
            ],
        )?;

        Ok(())
    }

    /// Get a KeyPackage by its reference hash
    pub async fn get_key_package(
        pool: &DbPool,
        keypackage_ref: &[u8],
    ) -> SqliteResult<Option<KeyPackageData>> {
        let conn = pool.lock().await;

        let mut stmt = conn.prepare(
            "SELECT keypackage_ref, username, keypackage_bytes, uploaded_at, status, not_after, credential_hash, ciphersuite
             FROM keypackages
             WHERE keypackage_ref = ?1",
        )?;

        let result = stmt
            .query_row(params![keypackage_ref], |row| {
                let status_str: String = row.get(4)?;
                let status =
                    KeyPackageStatus::from_str(&status_str).unwrap_or(KeyPackageStatus::Available);

                Ok(KeyPackageData {
                    keypackage_ref: row.get(0)?,
                    username: row.get(1)?,
                    keypackage_bytes: row.get(2)?,
                    uploaded_at: row.get(3)?,
                    status,
                    not_after: row.get(5)?,
                    credential_hash: row.get(6)?,
                    ciphersuite: row.get(7)?,
                })
            })
            .optional()?;

        Ok(result)
    }

    /// List available KeyPackages for a user (filters by status and expiry)
    pub async fn list_available_for_user(
        pool: &DbPool,
        username: &str,
    ) -> SqliteResult<Vec<KeyPackageMetadata>> {
        let conn = pool.lock().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut stmt = conn.prepare(
            "SELECT keypackage_ref, username, not_after, status, credential_hash, ciphersuite
             FROM keypackages
             WHERE username = ?1 AND status = ?2 AND not_after > ?3
             ORDER BY uploaded_at ASC",
        )?;

        let rows = stmt.query_map(
            params![username, KeyPackageStatus::Available.as_str(), now],
            |row| {
                let status_str: String = row.get(3)?;
                let status =
                    KeyPackageStatus::from_str(&status_str).unwrap_or(KeyPackageStatus::Available);

                Ok(KeyPackageMetadata {
                    keypackage_ref: row.get(0)?,
                    username: row.get(1)?,
                    not_after: row.get(2)?,
                    status,
                    credential_hash: row.get(4)?,
                    ciphersuite: row.get(5)?,
                })
            },
        )?;

        let metadata: Vec<KeyPackageMetadata> = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(metadata)
    }

    /// Reserve a KeyPackage for use (with TTL)
    /// Returns ReservedKeyPackage or None if no available keys
    pub async fn reserve_key_package(
        pool: &DbPool,
        target_username: &str,
        group_id: &[u8],
        reserved_by: &str,
    ) -> SqliteResult<Option<ReservedKeyPackage>> {
        Self::reserve_key_package_with_timeout(pool, target_username, group_id, reserved_by, RESERVATION_TTL_SECONDS).await
    }

    /// Reserve a KeyPackage with a custom timeout (in seconds)
    /// Returns ReservedKeyPackage or None if no available keys
    pub async fn reserve_key_package_with_timeout(
        pool: &DbPool,
        target_username: &str,
        group_id: &[u8],
        reserved_by: &str,
        timeout_seconds: i64,
    ) -> SqliteResult<Option<ReservedKeyPackage>> {
        let conn = pool.lock().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // First, release any expired reservations for this user
        Self::release_expired_reservations_sync(&conn, Some(target_username))?;

        // Find first available, non-expired key
        let mut stmt = conn.prepare(
            "SELECT keypackage_ref, keypackage_bytes, not_after
             FROM keypackages
             WHERE username = ?1 AND status = ?2 AND not_after > ?3
             ORDER BY uploaded_at ASC
             LIMIT 1",
        )?;

        let result = stmt
            .query_row(
                params![target_username, KeyPackageStatus::Available.as_str(), now],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;

        if let Some((keypackage_ref, keypackage_bytes, not_after)) = result {
            // Generate reservation ID and expiry
            let reservation_id = Uuid::new_v4().to_string();
            let reservation_expires_at = now + timeout_seconds;

            // Update to reserved status
            conn.execute(
                "UPDATE keypackages
                 SET status = ?1, reservation_id = ?2, reservation_expires_at = ?3, reserved_by = ?4, group_id = ?5
                 WHERE keypackage_ref = ?6 AND username = ?7",
                params![
                    KeyPackageStatus::Reserved.as_str(),
                    &reservation_id,
                    reservation_expires_at,
                    reserved_by,
                    group_id,
                    &keypackage_ref,
                    target_username,
                ],
            )?;

            Ok(Some(ReservedKeyPackage {
                keypackage_ref,
                keypackage_bytes,
                reservation_id,
                reservation_expires_at,
                not_after,
            }))
        } else {
            Ok(None)
        }
    }

    /// Spend a KeyPackage (mark as used)
    /// Returns error if key is already spent or doesn't exist
    pub async fn spend_key_package(
        pool: &DbPool,
        keypackage_ref: &[u8],
        group_id: &[u8],
        spent_by: &str,
    ) -> SqliteResult<()> {
        let conn = pool.lock().await;

        // First check current status
        let mut stmt = conn.prepare("SELECT status FROM keypackages WHERE keypackage_ref = ?1")?;

        let current_status: Option<String> = stmt
            .query_row(params![keypackage_ref], |row| row.get(0))
            .optional()?;

        match current_status {
            None => {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Some(status) if status == KeyPackageStatus::Spent.as_str() => {
                // Already spent - this is an error (double-spend attempt)
                return Err(rusqlite::Error::ExecuteReturnedResults);
            }
            _ => {
                // Available or Reserved - OK to spend
            }
        }

        let spent_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "UPDATE keypackages
             SET status = ?1, spent_at = ?2, spent_by = ?3, group_id = ?4
             WHERE keypackage_ref = ?5",
            params![
                KeyPackageStatus::Spent.as_str(),
                spent_at,
                spent_by,
                group_id,
                keypackage_ref,
            ],
        )?;

        Ok(())
    }

    /// Cleanup expired KeyPackages (based on not_after timestamp)
    /// Returns the number of keys removed
    pub async fn cleanup_expired(pool: &DbPool) -> SqliteResult<usize> {
        let conn = pool.lock().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let deleted = conn.execute(
            "DELETE FROM keypackages WHERE not_after <= ?1",
            params![now],
        )?;

        Ok(deleted)
    }

    /// Release expired reservations (convert back to available)
    /// Returns the number of reservations released
    pub async fn release_expired_reservations(pool: &DbPool) -> SqliteResult<usize> {
        let conn = pool.lock().await;
        Self::release_expired_reservations_sync(&conn, None)
    }

    /// Internal helper to release expired reservations (synchronous, optionally filtered by username)
    fn release_expired_reservations_sync(
        conn: &rusqlite::Connection,
        username: Option<&str>,
    ) -> SqliteResult<usize> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let updated = if let Some(user) = username {
            conn.execute(
                "UPDATE keypackages
                 SET status = ?1, reservation_id = NULL, reservation_expires_at = NULL, reserved_by = NULL, group_id = NULL
                 WHERE status = ?2 AND reservation_expires_at <= ?3 AND username = ?4",
                params![
                    KeyPackageStatus::Available.as_str(),
                    KeyPackageStatus::Reserved.as_str(),
                    now,
                    user,
                ],
            )?
        } else {
            conn.execute(
                "UPDATE keypackages
                 SET status = ?1, reservation_id = NULL, reservation_expires_at = NULL, reserved_by = NULL, group_id = NULL
                 WHERE status = ?2 AND reservation_expires_at <= ?3",
                params![
                    KeyPackageStatus::Available.as_str(),
                    KeyPackageStatus::Reserved.as_str(),
                    now,
                ],
            )?
        };

        Ok(updated)
    }

    /// Count KeyPackages by status for a user
    pub async fn count_by_status(
        pool: &DbPool,
        username: &str,
        status: KeyPackageStatus,
    ) -> SqliteResult<usize> {
        let conn = pool.lock().await;

        let mut stmt =
            conn.prepare("SELECT COUNT(*) FROM keypackages WHERE username = ?1 AND status = ?2")?;

        let count: i64 = stmt.query_row(params![username, status.as_str()], |row| row.get(0))?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[tokio::test]
    async fn test_save_and_retrieve_keypackage() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let keypackage_ref = vec![0x01, 0x02, 0x03, 0x04];
        let keypackage_bytes = vec![0x10, 0x20, 0x30, 0x40];
        let not_after = 9999999999; // Far future

        KeyPackageStore::save_key_package(
            &pool,
            "alice",
            &keypackage_ref,
            &keypackage_bytes,
            not_after,
            None,
            None,
        )
        .await
        .unwrap();

        let retrieved = KeyPackageStore::get_key_package(&pool, &keypackage_ref)
            .await
            .unwrap()
            .expect("KeyPackage not found");

        assert_eq!(retrieved.username, "alice");
        assert_eq!(retrieved.keypackage_bytes, keypackage_bytes);
        assert_eq!(retrieved.status, KeyPackageStatus::Available);
    }

    #[tokio::test]
    async fn test_double_spend_prevention() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let keypackage_ref = vec![0x05, 0x06, 0x07, 0x08];
        let keypackage_bytes = vec![0x50, 0x60, 0x70, 0x80];
        let group_id = vec![0xaa, 0xbb];
        let not_after = 9999999999;

        KeyPackageStore::save_key_package(
            &pool,
            "bob",
            &keypackage_ref,
            &keypackage_bytes,
            not_after,
            None,
            None,
        )
        .await
        .unwrap();

        // First spend should succeed
        KeyPackageStore::spend_key_package(&pool, &keypackage_ref, &group_id, "alice")
            .await
            .unwrap();

        // Second spend should fail (double-spend)
        let result =
            KeyPackageStore::spend_key_package(&pool, &keypackage_ref, &group_id, "charlie").await;

        assert!(result.is_err());

        // Verify status is still spent
        let data = KeyPackageStore::get_key_package(&pool, &keypackage_ref)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(data.status, KeyPackageStatus::Spent);
    }

    #[tokio::test]
    async fn test_reservation_ttl_enforcement() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let keypackage_ref = vec![0x09, 0x0a, 0x0b, 0x0c];
        let keypackage_bytes = vec![0x90, 0xa0, 0xb0, 0xc0];
        let group_id = vec![0xcc, 0xdd];
        let not_after = 9999999999;

        KeyPackageStore::save_key_package(
            &pool,
            "charlie",
            &keypackage_ref,
            &keypackage_bytes,
            not_after,
            None,
            None,
        )
        .await
        .unwrap();

        // Reserve the key
        let reserved = KeyPackageStore::reserve_key_package(&pool, "charlie", &group_id, "alice")
            .await
            .unwrap()
            .expect("Should reserve successfully");

        assert_eq!(reserved.keypackage_ref, keypackage_ref);
        assert!(reserved.reservation_expires_at > 0);

        // Manually expire the reservation by setting it to the past
        let conn = pool.lock().await;
        conn.execute(
            "UPDATE keypackages SET reservation_expires_at = 0 WHERE keypackage_ref = ?1",
            params![&keypackage_ref],
        )
        .unwrap();
        drop(conn);

        // Release expired reservations
        let released = KeyPackageStore::release_expired_reservations(&pool)
            .await
            .unwrap();
        assert_eq!(released, 1);

        // Verify key is available again
        let data = KeyPackageStore::get_key_package(&pool, &keypackage_ref)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(data.status, KeyPackageStatus::Available);
    }

    #[tokio::test]
    async fn test_expiry_cleanup() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Add expired key
        let expired_ref = vec![0x0d, 0x0e, 0x0f, 0x10];
        let expired_bytes = vec![0xd0, 0xe0, 0xf0, 0x00];
        KeyPackageStore::save_key_package(
            &pool,
            "david",
            &expired_ref,
            &expired_bytes,
            now - 100, // Already expired
            None,
            None,
        )
        .await
        .unwrap();

        // Add valid key
        let valid_ref = vec![0x11, 0x12, 0x13, 0x14];
        let valid_bytes = vec![0x11, 0x22, 0x33, 0x44];
        KeyPackageStore::save_key_package(
            &pool,
            "david",
            &valid_ref,
            &valid_bytes,
            now + 1000, // Future expiry
            None,
            None,
        )
        .await
        .unwrap();

        // Cleanup expired
        let removed = KeyPackageStore::cleanup_expired(&pool).await.unwrap();
        assert_eq!(removed, 1);

        // Verify expired key is gone
        let expired_data = KeyPackageStore::get_key_package(&pool, &expired_ref)
            .await
            .unwrap();
        assert!(expired_data.is_none());

        // Verify valid key still exists
        let valid_data = KeyPackageStore::get_key_package(&pool, &valid_ref)
            .await
            .unwrap();
        assert!(valid_data.is_some());
    }

    #[tokio::test]
    async fn test_list_available_filters_correctly() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Add available, non-expired key
        let available_ref = vec![0x15, 0x16, 0x17, 0x18];
        KeyPackageStore::save_key_package(
            &pool,
            "eve",
            &available_ref,
            &vec![0x01],
            now + 1000,
            None,
            None,
        )
        .await
        .unwrap();

        // Add expired key
        let expired_ref = vec![0x19, 0x1a, 0x1b, 0x1c];
        KeyPackageStore::save_key_package(
            &pool,
            "eve",
            &expired_ref,
            &vec![0x02],
            now - 100,
            None,
            None,
        )
        .await
        .unwrap();

        // Add spent key
        let spent_ref = vec![0x1d, 0x1e, 0x1f, 0x20];
        KeyPackageStore::save_key_package(
            &pool,
            "eve",
            &spent_ref,
            &vec![0x03],
            now + 1000,
            None,
            None,
        )
        .await
        .unwrap();
        KeyPackageStore::spend_key_package(&pool, &spent_ref, &vec![0xff], "alice")
            .await
            .unwrap();

        // List available keys
        let available = KeyPackageStore::list_available_for_user(&pool, "eve")
            .await
            .unwrap();

        // Should only return the available, non-expired key
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].keypackage_ref, available_ref);
        assert_eq!(available[0].status, KeyPackageStatus::Available);
    }

    #[tokio::test]
    async fn test_concurrent_reservations() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Add multiple keys for the same user
        for i in 0..3 {
            let keypackage_ref = vec![0x20 + i, 0x21 + i, 0x22 + i, 0x23 + i];
            KeyPackageStore::save_key_package(
                &pool,
                "frank",
                &keypackage_ref,
                &vec![0x10 + i],
                now + 1000,
                None,
                None,
            )
            .await
            .unwrap();
        }

        // Reserve keys concurrently (simulated)
        let reserved1 = KeyPackageStore::reserve_key_package(&pool, "frank", &vec![0xaa], "alice")
            .await
            .unwrap()
            .expect("First reservation should succeed");

        let reserved2 = KeyPackageStore::reserve_key_package(&pool, "frank", &vec![0xbb], "bob")
            .await
            .unwrap()
            .expect("Second reservation should succeed");

        // Verify different keys were reserved
        assert_ne!(reserved1.keypackage_ref, reserved2.keypackage_ref);
        assert_ne!(reserved1.reservation_id, reserved2.reservation_id);
    }

    #[tokio::test]
    async fn test_spend_updates_status_and_details() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let keypackage_ref = vec![0x24, 0x25, 0x26, 0x27];
        let group_id = vec![0xaa, 0xbb, 0xcc];
        let not_after = 9999999999;

        KeyPackageStore::save_key_package(
            &pool,
            "grace",
            &keypackage_ref,
            &vec![0x99],
            not_after,
            None,
            None,
        )
        .await
        .unwrap();

        // Spend the key
        KeyPackageStore::spend_key_package(&pool, &keypackage_ref, &group_id, "alice")
            .await
            .unwrap();

        // Verify spend details
        let conn = pool.lock().await;
        let mut stmt = conn.prepare(
            "SELECT status, spent_at, spent_by, group_id FROM keypackages WHERE keypackage_ref = ?1",
        ).unwrap();

        let (status, spent_at, spent_by, stored_group_id): (String, i64, String, Vec<u8>) = stmt
            .query_row(params![&keypackage_ref], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap();

        assert_eq!(status, "spent");
        assert!(spent_at > 0);
        assert_eq!(spent_by, "alice");
        assert_eq!(stored_group_id, group_id);
    }

    #[tokio::test]
    async fn test_reservation_timeout_releases_key() {
        let pool = create_test_pool();
        KeyPackageStore::initialize_schema(&pool).await.unwrap();

        let keypackage_ref = vec![0x28, 0x29, 0x2a, 0x2b];
        let group_id = vec![0xdd, 0xee];
        let not_after = 9999999999;

        KeyPackageStore::save_key_package(
            &pool,
            "heidi",
            &keypackage_ref,
            &vec![0xaa],
            not_after,
            None,
            None,
        )
        .await
        .unwrap();

        // Reserve the key
        let _reserved = KeyPackageStore::reserve_key_package(&pool, "heidi", &group_id, "alice")
            .await
            .unwrap()
            .expect("Should reserve");

        // Manually expire the reservation
        let conn = pool.lock().await;
        conn.execute(
            "UPDATE keypackages SET reservation_expires_at = 0 WHERE keypackage_ref = ?1",
            params![&keypackage_ref],
        )
        .unwrap();
        drop(conn);

        // Release expired reservations
        KeyPackageStore::release_expired_reservations(&pool)
            .await
            .unwrap();

        // Key should be available for reuse
        let reserved_again = KeyPackageStore::reserve_key_package(&pool, "heidi", &group_id, "bob")
            .await
            .unwrap()
            .expect("Should reserve after timeout");

        assert_eq!(reserved_again.keypackage_ref, keypackage_ref);
    }
}
