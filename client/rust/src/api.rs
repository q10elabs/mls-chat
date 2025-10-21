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
    key_package: Vec<u8>,
}

#[derive(Deserialize)]
struct RegisterUserResponse {
    #[allow(dead_code)]
    id: i64,
    #[allow(dead_code)]
    username: String,
    #[allow(dead_code)]
    created_at: String,
}

#[derive(Deserialize)]
struct UserKeyResponse {
    #[allow(dead_code)]
    username: String,
    key_package: Vec<u8>,
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

    /// Register a user with the server, sending their KeyPackage
    pub async fn register_user(&self, username: &str, key_package: &[u8]) -> Result<()> {
        let request = RegisterUserRequest {
            username: username.to_string(),
            key_package: key_package.to_vec(),
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

    /// Get a user's KeyPackage from the server
    pub async fn get_user_key(&self, username: &str) -> Result<Vec<u8>> {
        let response = self.client
            .get(&format!("{}/users/{}", self.base_url, username))
            .send()
            .await?;

        if response.status().is_success() {
            let user_key: UserKeyResponse = response.json().await?;
            Ok(user_key.key_package)
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
