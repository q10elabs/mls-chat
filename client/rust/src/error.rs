//! Error types for the MLS chat client

use thiserror::Error;

/// Main error type for the MLS chat client
#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    #[error("MLS error: {0}")]
    Mls(#[from] MlsError),

    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Channel error: {0}")]
    Channel(#[from] futures::channel::mpsc::TrySendError<tokio_tungstenite::tungstenite::Message>),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Storage-related errors
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Identity not found for username: {0}")]
    IdentityNotFound(String),

    #[error("Group state not found for {username} in {group_id}")]
    GroupStateNotFound { username: String, group_id: String },

    #[error("No group members found: {0}")]
    NoGroupMembers(String),
}

/// Network-related errors
#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Server error: {0}")]
    Server(String),

    #[error("Connection timeout")]
    Timeout,

    #[error("KeyPackage error: {0}")]
    KeyPackage(#[from] KeyPackageError),
}

/// KeyPackage pool operation errors
#[derive(Error, Debug, Clone, PartialEq)]
pub enum KeyPackageError {
    #[error("No available KeyPackage for user '{username}'")]
    PoolExhausted { username: String },

    #[error("KeyPackage has expired (ref: {keypackage_ref:?})")]
    KeyPackageExpired { keypackage_ref: Vec<u8> },

    #[error("KeyPackage already spent (ref: {keypackage_ref:?})")]
    DoubleSpendAttempted { keypackage_ref: Vec<u8> },

    #[error("Reservation has expired (reservation_id: {reservation_id})")]
    ReservationExpired { reservation_id: String },

    #[error("KeyPackage not found (ref: {keypackage_ref:?})")]
    InvalidKeyPackageRef { keypackage_ref: Vec<u8> },

    #[error("Server error: {message}")]
    ServerError { message: String },

    #[error("Invalid response from server: {message}")]
    InvalidResponse { message: String },
}

/// MLS protocol errors
#[derive(Error, Debug)]
pub enum MlsError {
    #[error("OpenMLS error: {0}")]
    OpenMls(String),

    #[error("Invalid credential")]
    InvalidCredential,

    #[error("Invalid key package")]
    InvalidKeyPackage,

    #[error("Group not found")]
    GroupNotFound,

    #[error("Member not found")]
    MemberNotFound,

    #[error("Encryption failed")]
    EncryptionFailed,

    #[error("Decryption failed")]
    DecryptionFailed,

    #[error("Key package pool capacity exceeded (needed {needed}, available {available})")]
    PoolCapacityExceeded { needed: usize, available: usize },
}

/// Result type alias for the client
pub type Result<T> = std::result::Result<T, ClientError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let storage_err = StorageError::IdentityNotFound("alice".to_string());
        let client_err = ClientError::Storage(storage_err);

        assert!(client_err.to_string().contains("Storage error"));
        assert!(client_err.to_string().contains("alice"));
    }

    #[test]
    fn test_error_conversion() {
        let sqlite_err = rusqlite::Error::InvalidColumnType(
            0,
            "test".to_string(),
            rusqlite::types::Type::Integer,
        );
        let storage_err: StorageError = sqlite_err.into();
        let client_err: ClientError = storage_err.into();

        assert!(matches!(
            client_err,
            ClientError::Storage(StorageError::Database(_))
        ));
    }
}
