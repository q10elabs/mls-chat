/// Server API client for REST endpoints

use crate::error::{Result, NetworkError};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Server API client
pub struct ServerApi {
    client: Client,
    base_url: String,
}

#[derive(Serialize)]
struct RegisterUserRequest {
    username: String,
    public_key: String,
}

#[derive(Deserialize)]
struct RegisterUserResponse {
    id: i64,
    username: String,
    created_at: String,
}

#[derive(Deserialize)]
struct UserKeyResponse {
    username: String,
    public_key: String,
}

impl ServerApi {
    /// Create a new server API client
    pub fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            client,
            base_url: base_url.to_string(),
        }
    }

    /// Register a user with the server
    pub async fn register_user(&self, username: &str, public_key: &str) -> Result<()> {
        let request = RegisterUserRequest {
            username: username.to_string(),
            public_key: public_key.to_string(),
        };

        let response = self.client
            .post(&format!("{}/users", self.base_url))
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(NetworkError::Server(format!("Registration failed: {}", response.status())).into())
        }
    }

    /// Get a user's public key from the server
    pub async fn get_user_key(&self, username: &str) -> Result<String> {
        let response = self.client
            .get(&format!("{}/users/{}", self.base_url, username))
            .send()
            .await?;

        if response.status().is_success() {
            let user_key: UserKeyResponse = response.json().await?;
            Ok(user_key.public_key)
        } else if response.status() == 404 {
            Err(NetworkError::Server("User not found".to_string()).into())
        } else {
            Err(NetworkError::Server(format!("Failed to get user key: {}", response.status())).into())
        }
    }

    /// Check if the server is healthy
    pub async fn health_check(&self) -> Result<()> {
        let response = self.client
            .get(&format!("{}/health", self.base_url))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(NetworkError::Server(format!("Health check failed: {}", response.status())).into())
        }
    }
}
