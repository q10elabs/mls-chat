//! Server API client for REST endpoints

use crate::error::{NetworkError, Result};
use reqwest::{Client, StatusCode};
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
    ///
    /// Idempotent: 409 Conflict (user already exists) is treated as success only if
    /// the stored key package matches the local one. If they differ, returns an error
    /// indicating identity compromise or key material mismatch.
    pub async fn register_user(&self, username: &str, key_package: &[u8]) -> Result<()> {
        let request = RegisterUserRequest {
            username: username.to_string(),
            key_package: key_package.to_vec(),
        };

        let response = self
            .client
            .post(format!("{}/users", self.base_url))
            .json(&request)
            .send()
            .await?;

        match response.status() {
            status if status.is_success() => {
                log::info!("User {} registered with server", username);
                Ok(())
            }
            StatusCode::CONFLICT => {
                // User already registered - verify the stored key package matches
                log::info!(
                    "User {} already registered, validating key package",
                    username
                );

                match self.get_user_key(username).await {
                    Ok(remote_key_package) => {
                        if remote_key_package == key_package {
                            log::info!("User {} identity verified - key packages match", username);
                            Ok(())
                        } else {
                            log::error!(
                                "SECURITY: Key package mismatch for user {}. Local key differs from server.",
                                username
                            );
                            Err(NetworkError::Server(
                                format!(
                                    "Key package mismatch for user '{}': local key differs from stored key on server. \
                                    This may indicate identity compromise. Please use a different username.",
                                    username
                                )
                            ).into())
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to verify key package for user {} on conflict: {}",
                            username,
                            e
                        );
                        Err(NetworkError::Server(format!(
                            "Cannot verify existing user {}: {}",
                            username, e
                        ))
                        .into())
                    }
                }
            }
            status => Err(NetworkError::Server(format!("Registration failed: {}", status)).into()),
        }
    }

    /// Get a user's KeyPackage from the server
    pub async fn get_user_key(&self, username: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(format!("{}/users/{}", self.base_url, username))
            .send()
            .await?;

        if response.status().is_success() {
            let user_key: UserKeyResponse = response.json().await?;
            Ok(user_key.key_package)
        } else if response.status() == 404 {
            Err(NetworkError::Server("User not found".to_string()).into())
        } else {
            Err(
                NetworkError::Server(format!("Failed to get user key: {}", response.status()))
                    .into(),
            )
        }
    }

    /// Check if the server is healthy
    pub async fn health_check(&self) -> Result<()> {
        let response = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(NetworkError::Server(format!("Health check failed: {}", response.status())).into())
        }
    }
}
