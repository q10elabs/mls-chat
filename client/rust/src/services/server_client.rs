/// Server communication layer (HTTP/WebSocket).
/// Abstracts communication with the MLS chat server.

use crate::error::{ClientError, Result};
use crate::models::{MessagePayload, RegisterUserRequest, UserKeyResponse};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ServerClient {
    base_url: String,
    client: reqwest::Client,
}

impl ServerClient {
    pub fn new(server_url: String) -> Self {
        ServerClient {
            base_url: server_url,
            client: reqwest::Client::new(),
        }
    }

    /// Register a new user with the server
    pub async fn register_user(&self, username: String, public_key: String) -> Result<String> {
        let url = format!("{}/users", self.base_url);
        let req = RegisterUserRequest { username, public_key };

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to register user: {}", e)))?;

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "Server returned status: {}",
                response.status()
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to read response: {}", e)))?;

        Ok(body)
    }

    /// Get a user's public key from the server
    pub async fn get_user_key(&self, username: &str) -> Result<UserKeyResponse> {
        let url = format!("{}/users/{}", self.base_url, username);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to get user key: {}", e)))?;

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "User not found: {}",
                username
            )));
        }

        let key_response = response
            .json::<UserKeyResponse>()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to parse response: {}", e)))?;

        Ok(key_response)
    }

    /// Send a message to a group
    pub async fn send_message(
        &self,
        group_id: String,
        sender: String,
        encrypted_content: String,
    ) -> Result<()> {
        let url = format!("{}/groups/{}/messages", self.base_url, group_id);
        let payload = MessagePayload {
            group_id,
            sender,
            encrypted_content,
        };

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to send message: {}", e)))?;

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "Failed to send message: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Poll for messages in a group (HTTP fallback when WebSocket unavailable)
    pub async fn poll_group_messages(&self, group_id: &str) -> Result<Vec<MessagePayload>> {
        let url = format!("{}/groups/{}/messages", self.base_url, group_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to poll messages: {}", e)))?;

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "Failed to poll messages: {}",
                response.status()
            )));
        }

        let messages = response
            .json::<Vec<MessagePayload>>()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to parse messages: {}", e)))?;

        Ok(messages)
    }

    /// Store encrypted backup state on server
    pub async fn store_backup(&self, username: &str, encrypted_state: String) -> Result<()> {
        let url = format!("{}/backup/{}", self.base_url, username);
        let body = serde_json::json!({ "encrypted_state": encrypted_state });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to store backup: {}", e)))?;

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "Failed to store backup: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Retrieve encrypted backup state from server
    pub async fn get_backup(&self, username: &str) -> Result<Option<String>> {
        let url = format!("{}/backup/{}", self.base_url, username);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to get backup: {}", e)))?;

        if response.status() == 404 {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(ClientError::ServerError(format!(
                "Failed to get backup: {}",
                response.status()
            )));
        }

        let body = response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| ClientError::HttpError(format!("Failed to parse backup: {}", e)))?;

        let encrypted_state = body["encrypted_state"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ClientError::ServerError("Invalid backup format".to_string()))?;

        Ok(Some(encrypted_state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_client_creation() {
        let client = ServerClient::new("http://localhost:4000".to_string());
        assert_eq!(client.base_url, "http://localhost:4000");
    }

    // Note: Async tests for ServerClient have been disabled because they can hang
    // when attempting to connect to invalid hosts. Integration tests will verify
    // actual server communication.
}
