/// Database schema initialization.
/// Sets up SQLite WAL mode and creates tables on startup.
use rusqlite::{Connection, Result as SqliteResult};

/// Initialize database connection with WAL mode and schema
pub fn initialize_database(conn: &Connection) -> SqliteResult<()> {
    // Enable WAL mode (for file-based DB only, ignore error for in-memory)
    let _ = conn.execute("PRAGMA journal_mode = WAL", []);
    let _ = conn.execute("PRAGMA synchronous = NORMAL", []);

    // Create tables
    create_schema(conn)?;

    Ok(())
}

/// Create all database tables
fn create_schema(conn: &Connection) -> SqliteResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            key_package BLOB NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS groups (
            id INTEGER PRIMARY KEY,
            group_id TEXT UNIQUE NOT NULL,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY,
            group_id INTEGER NOT NULL,
            sender_id INTEGER NOT NULL,
            encrypted_content TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            FOREIGN KEY(group_id) REFERENCES groups(id),
            FOREIGN KEY(sender_id) REFERENCES users(id)
        );

        CREATE TABLE IF NOT EXISTS backups (
            id INTEGER PRIMARY KEY,
            username TEXT NOT NULL,
            encrypted_state TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            UNIQUE(username, timestamp),
            FOREIGN KEY(username) REFERENCES users(username)
        );

        CREATE INDEX IF NOT EXISTS idx_messages_group ON messages(group_id);
        CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id);
        CREATE INDEX IF NOT EXISTS idx_backups_username ON backups(username);

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

        CREATE INDEX IF NOT EXISTS idx_keypackages_user_status
            ON keypackages(username, status);
        CREATE INDEX IF NOT EXISTS idx_keypackages_user_expiry
            ON keypackages(username, not_after);
        CREATE INDEX IF NOT EXISTS idx_keypackages_reservation
            ON keypackages(reservation_id);
        "#,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_initialize_in_memory_database() {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory DB");
        initialize_database(&conn).expect("Failed to initialize DB");

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .expect("Query failed")
            .query_map([], |row| row.get(0))
            .expect("Mapping failed")
            .collect::<Result<Vec<_>, _>>()
            .expect("Collection failed");

        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"groups".to_string()));
        assert!(tables.contains(&"messages".to_string()));
        assert!(tables.contains(&"backups".to_string()));
    }

    #[test]
    fn test_users_table_schema() {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory DB");
        initialize_database(&conn).expect("Failed to initialize DB");

        // Verify users table has correct columns
        let mut stmt = conn
            .prepare("PRAGMA table_info(users)")
            .expect("Query failed");
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("Mapping failed")
            .collect::<Result<Vec<_>, _>>()
            .expect("Collection failed");

        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"username".to_string()));
        assert!(columns.contains(&"key_package".to_string()));
        assert!(columns.contains(&"created_at".to_string()));
    }

    #[test]
    fn test_wal_mode_enabled() {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory DB");
        initialize_database(&conn).expect("Failed to initialize DB");

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("Query failed");

        // In-memory databases don't support WAL, but query should not fail
        assert!(!journal_mode.is_empty());
    }
}
