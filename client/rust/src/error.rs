/// Error types for the MLS chat client.
/// Provides comprehensive error handling for all client operations.

use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Server communication error: {0}")]
    ServerError(String),

    #[error("OpenMLS error: {0}")]
    MlsError(String),

    #[error("Invalid group: {0}")]
    InvalidGroup(String),

    #[error("Invalid user: {0}")]
    InvalidUser(String),

    #[error("Message error: {0}")]
    MessageError(String),

    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("Database error: {0}")]
    DbError(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("State error: {0}")]
    StateError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

pub type Result<T> = std::result::Result<T, ClientError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ClientError::StorageError("database locked".to_string());
        assert!(err.to_string().contains("Storage error"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let client_err: ClientError = io_err.into();
        assert!(client_err.to_string().contains("IO error"));
    }

    #[test]
    fn test_result_type() {
        let ok_result: Result<i32> = Ok(42);
        let err_result: Result<i32> = Err(ClientError::StateError("invalid state".to_string()));

        assert!(ok_result.is_ok());
        assert!(err_result.is_err());
    }
}
