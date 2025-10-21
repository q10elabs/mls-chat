/// Database layer for persistent storage.
/// Handles all database operations for users, groups, messages, and backups.

pub mod init;
pub mod models;

use chrono::Utc;
use models::{Backup, Group, Message, User};
use rusqlite::{params, Connection, OptionalExtension, Result as SqliteResult};
use std::sync::Arc;
use tokio::sync::Mutex;

pub type DbPool = Arc<Mutex<Connection>>;

/// Create a connection pool (simplified for single-threaded SQLite)
pub fn create_pool(db_path: &str) -> SqliteResult<DbPool> {
    let conn = Connection::open(db_path)?;
    init::initialize_database(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

/// Create an in-memory database for testing
pub fn create_test_pool() -> DbPool {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory DB");
    init::initialize_database(&conn).expect("Failed to initialize DB");
    Arc::new(Mutex::new(conn))
}

/// Database operations
pub struct Database;

impl Database {
    /// Register a new user with their key package
    pub async fn register_user(pool: &DbPool, username: &str, key_package: &[u8]) -> SqliteResult<User> {
        let conn = pool.lock().await;
        let created_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO users (username, key_package, created_at) VALUES (?1, ?2, ?3)",
            params![username, key_package, &created_at],
        )?;

        // Retrieve the inserted user
        let mut stmt = conn.prepare("SELECT id, username, key_package, created_at FROM users WHERE username = ?1")?;
        let user = stmt.query_row(params![username], |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                key_package: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;

        Ok(user)
    }

    /// Get user by username
    pub async fn get_user(pool: &DbPool, username: &str) -> SqliteResult<Option<User>> {
        let conn = pool.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, username, key_package, created_at FROM users WHERE username = ?1",
        )?;

        let user = stmt
            .query_row(params![username], |row| {
                Ok(User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    key_package: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .optional()?;

        Ok(user)
    }

    /// Get user by ID
    pub async fn get_user_by_id(pool: &DbPool, user_id: i64) -> SqliteResult<Option<User>> {
        let conn = pool.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, username, key_package, created_at FROM users WHERE id = ?1",
        )?;

        let user = stmt
            .query_row(params![user_id], |row| {
                Ok(User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    key_package: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .optional()?;

        Ok(user)
    }

    /// Create a new group
    pub async fn create_group(pool: &DbPool, group_id: &str, name: &str) -> SqliteResult<Group> {
        let conn = pool.lock().await;
        let created_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO groups (group_id, name, created_at) VALUES (?1, ?2, ?3)",
            params![group_id, name, &created_at],
        )?;

        let mut stmt =
            conn.prepare("SELECT id, group_id, name, created_at FROM groups WHERE group_id = ?1")?;
        let group = stmt.query_row(params![group_id], |row| {
            Ok(Group {
                id: row.get(0)?,
                group_id: row.get(1)?,
                name: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;

        Ok(group)
    }

    /// Get group by group_id
    pub async fn get_group(pool: &DbPool, group_id: &str) -> SqliteResult<Option<Group>> {
        let conn = pool.lock().await;

        let mut stmt =
            conn.prepare("SELECT id, group_id, name, created_at FROM groups WHERE group_id = ?1")?;

        let group = stmt
            .query_row(params![group_id], |row| {
                Ok(Group {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    name: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .optional()?;

        Ok(group)
    }

    /// Store an encrypted message
    pub async fn store_message(
        pool: &DbPool,
        group_id: i64,
        sender_id: i64,
        encrypted_content: &str,
    ) -> SqliteResult<Message> {
        let conn = pool.lock().await;
        let timestamp = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO messages (group_id, sender_id, encrypted_content, timestamp) VALUES (?1, ?2, ?3, ?4)",
            params![group_id, sender_id, encrypted_content, &timestamp],
        )?;

        let mut stmt = conn.prepare(
            "SELECT id, group_id, sender_id, encrypted_content, timestamp FROM messages ORDER BY id DESC LIMIT 1",
        )?;
        let message = stmt.query_row([], |row| {
            Ok(Message {
                id: row.get(0)?,
                group_id: row.get(1)?,
                sender_id: row.get(2)?,
                encrypted_content: row.get(3)?,
                timestamp: row.get(4)?,
            })
        })?;

        Ok(message)
    }

    /// Get messages for a group
    pub async fn get_group_messages(pool: &DbPool, group_id: i64, limit: i64) -> SqliteResult<Vec<Message>> {
        let conn = pool.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, group_id, sender_id, encrypted_content, timestamp FROM messages WHERE group_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;

        let messages = stmt
            .query_map(params![group_id, limit], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    sender_id: row.get(2)?,
                    encrypted_content: row.get(3)?,
                    timestamp: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Store encrypted state backup
    pub async fn store_backup(
        pool: &DbPool,
        username: &str,
        encrypted_state: &str,
    ) -> SqliteResult<Backup> {
        let conn = pool.lock().await;
        let timestamp = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO backups (username, encrypted_state, timestamp) VALUES (?1, ?2, ?3)",
            params![username, encrypted_state, &timestamp],
        )?;

        let mut stmt = conn.prepare(
            "SELECT id, username, encrypted_state, timestamp FROM backups WHERE username = ?1 ORDER BY timestamp DESC LIMIT 1",
        )?;
        let backup = stmt.query_row(params![username], |row| {
            Ok(Backup {
                id: row.get(0)?,
                username: row.get(1)?,
                encrypted_state: row.get(2)?,
                timestamp: row.get(3)?,
            })
        })?;

        Ok(backup)
    }

    /// Get latest backup for a user
    pub async fn get_backup(pool: &DbPool, username: &str) -> SqliteResult<Option<Backup>> {
        let conn = pool.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, username, encrypted_state, timestamp FROM backups WHERE username = ?1 ORDER BY timestamp DESC LIMIT 1",
        )?;

        let backup = stmt
            .query_row(params![username], |row| {
                Ok(Backup {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    encrypted_state: row.get(2)?,
                    timestamp: row.get(3)?,
                })
            })
            .optional()?;

        Ok(backup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_user() {
        let pool = create_test_pool();
        let key_package = vec![0x01, 0x02, 0x03, 0x04];
        let user = Database::register_user(&pool, "alice", &key_package)
            .await
            .expect("Failed to register user");

        assert_eq!(user.username, "alice");
        assert_eq!(user.key_package, key_package);
        assert!(user.id > 0);
    }

    #[tokio::test]
    async fn test_get_user() {
        let pool = create_test_pool();
        let key_package = vec![0x05, 0x06, 0x07, 0x08];
        Database::register_user(&pool, "bob", &key_package).await.expect("Failed to register");

        let user = Database::get_user(&pool, "bob")
            .await
            .expect("Failed to get user")
            .expect("User not found");

        assert_eq!(user.username, "bob");
        assert_eq!(user.key_package, key_package);
    }

    #[tokio::test]
    async fn test_get_nonexistent_user() {
        let pool = create_test_pool();
        let user = Database::get_user(&pool, "nonexistent")
            .await
            .expect("Query failed");

        assert!(user.is_none());
    }

    #[tokio::test]
    async fn test_create_group() {
        let pool = create_test_pool();
        let group = Database::create_group(&pool, "group_001", "test_group")
            .await
            .expect("Failed to create group");

        assert_eq!(group.group_id, "group_001");
        assert_eq!(group.name, "test_group");
        assert!(group.id > 0);
    }

    #[tokio::test]
    async fn test_store_message() {
        let pool = create_test_pool();
        let key_package = vec![0x09, 0x0a, 0x0b, 0x0c];
        let user = Database::register_user(&pool, "alice", &key_package)
            .await
            .expect("Failed to register user");
        let group = Database::create_group(&pool, "group_001", "test")
            .await
            .expect("Failed to create group");

        let message = Database::store_message(&pool, group.id, user.id, "encrypted_content")
            .await
            .expect("Failed to store message");

        assert_eq!(message.group_id, group.id);
        assert_eq!(message.sender_id, user.id);
        assert_eq!(message.encrypted_content, "encrypted_content");
    }

    #[tokio::test]
    async fn test_get_group_messages() {
        let pool = create_test_pool();
        let key_package = vec![0x0d, 0x0e, 0x0f, 0x10];
        let user = Database::register_user(&pool, "alice", &key_package)
            .await
            .expect("Failed to register user");
        let group = Database::create_group(&pool, "group_001", "test")
            .await
            .expect("Failed to create group");

        Database::store_message(&pool, group.id, user.id, "msg1")
            .await
            .expect("Failed to store");
        Database::store_message(&pool, group.id, user.id, "msg2")
            .await
            .expect("Failed to store");

        let messages = Database::get_group_messages(&pool, group.id, 10)
            .await
            .expect("Failed to get messages");

        assert_eq!(messages.len(), 2);
    }

    #[tokio::test]
    async fn test_store_and_get_backup() {
        let pool = create_test_pool();
        let key_package = vec![0x11, 0x12, 0x13, 0x14];
        Database::register_user(&pool, "alice", &key_package)
            .await
            .expect("Failed to register user");

        let backup = Database::store_backup(&pool, "alice", "encrypted_state_data")
            .await
            .expect("Failed to store backup");

        assert_eq!(backup.username, "alice");
        assert_eq!(backup.encrypted_state, "encrypted_state_data");

        let retrieved = Database::get_backup(&pool, "alice")
            .await
            .expect("Failed to get backup")
            .expect("Backup not found");

        assert_eq!(retrieved.username, "alice");
        assert_eq!(retrieved.encrypted_state, "encrypted_state_data");
    }

    #[tokio::test]
    async fn test_backup_replacement() {
        let pool = create_test_pool();
        let key_package = vec![0x15, 0x16, 0x17, 0x18];
        Database::register_user(&pool, "alice", &key_package)
            .await
            .expect("Failed to register user");

        Database::store_backup(&pool, "alice", "state1")
            .await
            .expect("Failed to store");
        let backup2 = Database::store_backup(&pool, "alice", "state2")
            .await
            .expect("Failed to store");

        let retrieved = Database::get_backup(&pool, "alice")
            .await
            .expect("Failed to get backup")
            .expect("Backup not found");

        assert_eq!(retrieved.encrypted_state, "state2");
        assert_eq!(retrieved.id, backup2.id);
    }
}
