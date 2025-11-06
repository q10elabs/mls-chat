//! Server API client for REST endpoints
//!
//! Provides helpers for user registration, KeyPackage uploads, pool
//! reservation/spend flows, and health/status queries against the MLS
//! chat server.

use crate::error::{KeyPackageError, NetworkError, Result};
use base64::{engine::general_purpose, Engine as _};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Server API client
#[derive(Clone)]
pub struct ServerApi {
    client: Client,
    base_url: String,
}

/// Payload record for uploading a KeyPackage to the server
#[derive(Debug, Clone)]
pub struct KeyPackageUpload {
    pub keypackage_ref: Vec<u8>,
    pub keypackage: Vec<u8>,
    pub not_after: i64,
}

/// Response returned after uploading a batch of KeyPackages
#[derive(Debug, Clone, Deserialize)]
pub struct UploadKeyPackagesResponse {
    pub accepted: usize,
    pub rejected: Vec<String>,
    pub pool_size: usize,
}

/// Reserved KeyPackage details returned when the server grants a reservation
#[derive(Debug, Clone)]
pub struct ReservedKeyPackage {
    pub keypackage_ref: Vec<u8>,
    pub keypackage: Vec<u8>,
    pub reservation_id: String,
    pub reservation_expires_at: i64,
    pub not_after: i64,
}

/// Aggregate pool status information returned by the server
#[derive(Debug, Clone, Deserialize)]
pub struct KeyPackagePoolStatus {
    pub available: usize,
    pub reserved: usize,
    #[serde(default)]
    pub spent: usize,
    #[serde(default)]
    pub expired: usize,
    pub total: usize,
    #[serde(default)]
    pub expiring_soon: usize,
    #[serde(default)]
    pub pool_health: Option<String>,
    #[serde(default)]
    pub recommended_action: Option<String>,
    #[serde(default)]
    pub last_upload: Option<String>,
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

    /// Upload a batch of KeyPackages for the specified username
    pub async fn upload_key_packages(
        &self,
        username: &str,
        packages: &[KeyPackageUpload],
    ) -> Result<UploadKeyPackagesResponse> {
        if packages.is_empty() {
            return Err(NetworkError::Server("No keypackages to upload".to_string()).into());
        }

        #[derive(Serialize)]
        struct UploadRequest<'a> {
            username: &'a str,
            keypackages: Vec<UploadRequestItem>,
        }

        #[derive(Serialize)]
        struct UploadRequestItem {
            keypackage_ref: String,
            keypackage: String,
            not_after: i64,
        }

        let keypackages = packages
            .iter()
            .map(|pkg| UploadRequestItem {
                keypackage_ref: general_purpose::STANDARD.encode(&pkg.keypackage_ref),
                keypackage: general_purpose::STANDARD.encode(&pkg.keypackage),
                not_after: pkg.not_after,
            })
            .collect();

        let request = UploadRequest {
            username,
            keypackages,
        };

        let response = self
            .client
            .post(format!("{}/keypackages/upload", self.base_url))
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            let parsed: UploadKeyPackagesResponse = response.json().await?;
            Ok(parsed)
        } else {
            Err(NetworkError::Server(format!("Upload failed: {}", response.status())).into())
        }
    }

    /// Reserve a KeyPackage for inviting `target_username` into `group_id`
    pub async fn reserve_key_package(
        &self,
        target_username: &str,
        group_id: &[u8],
        reserved_by: &str,
    ) -> Result<ReservedKeyPackage> {
        #[derive(Serialize)]
        struct ReserveRequest<'a> {
            target_username: &'a str,
            reserved_by: &'a str,
            group_id: String,
        }

        #[derive(Deserialize)]
        struct ReserveResponse {
            keypackage_ref: String,
            keypackage: String,
            reservation_id: String,
            reservation_expires_at: i64,
            not_after: i64,
        }

        let request = ReserveRequest {
            target_username,
            reserved_by,
            group_id: general_purpose::STANDARD.encode(group_id),
        };

        let response = self
            .client
            .post(format!("{}/keypackages/reserve", self.base_url))
            .json(&request)
            .send()
            .await?;

        match response.status() {
            status if status.is_success() => {
                let payload: ReserveResponse = response.json().await.map_err(|e| {
                    NetworkError::KeyPackage(KeyPackageError::InvalidResponse {
                        message: format!("Failed to parse reserve response: {}", e),
                    })
                })?;

                let keypackage_ref = general_purpose::STANDARD
                    .decode(payload.keypackage_ref)
                    .map_err(|e| {
                        NetworkError::KeyPackage(KeyPackageError::InvalidResponse {
                            message: format!("Invalid keypackage_ref in reserve response: {}", e),
                        })
                    })?;

                let keypackage = general_purpose::STANDARD
                    .decode(payload.keypackage)
                    .map_err(|e| {
                        NetworkError::KeyPackage(KeyPackageError::InvalidResponse {
                            message: format!("Invalid keypackage payload: {}", e),
                        })
                    })?;

                Ok(ReservedKeyPackage {
                    keypackage_ref,
                    keypackage,
                    reservation_id: payload.reservation_id,
                    reservation_expires_at: payload.reservation_expires_at,
                    not_after: payload.not_after,
                })
            }
            StatusCode::NOT_FOUND => Err(NetworkError::KeyPackage(
                KeyPackageError::PoolExhausted {
                    username: target_username.to_string(),
                }
            )
            .into()),
            status => Err(NetworkError::KeyPackage(KeyPackageError::ServerError {
                message: format!("Failed to reserve key package: {}", status),
            })
            .into()),
        }
    }

    /// Mark a reserved KeyPackage as spent on the server
    pub async fn spend_key_package(
        &self,
        keypackage_ref: &[u8],
        group_id: &[u8],
        spent_by: &str,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct SpendRequest<'a> {
            keypackage_ref: String,
            group_id: String,
            spent_by: &'a str,
        }

        let request = SpendRequest {
            keypackage_ref: general_purpose::STANDARD.encode(keypackage_ref),
            group_id: general_purpose::STANDARD.encode(group_id),
            spent_by,
        };

        let response = self
            .client
            .post(format!("{}/keypackages/spend", self.base_url))
            .json(&request)
            .send()
            .await?;

        match response.status() {
            status if status.is_success() => Ok(()),
            StatusCode::CONFLICT => Err(NetworkError::KeyPackage(
                KeyPackageError::DoubleSpendAttempted {
                    keypackage_ref: keypackage_ref.to_vec(),
                }
            )
            .into()),
            StatusCode::NOT_FOUND => Err(NetworkError::KeyPackage(
                KeyPackageError::InvalidKeyPackageRef {
                    keypackage_ref: keypackage_ref.to_vec(),
                }
            )
            .into()),
            status => Err(NetworkError::KeyPackage(KeyPackageError::ServerError {
                message: format!("Failed to spend key package: {}", status),
            })
            .into()),
        }
    }

    /// Fetch aggregate KeyPackage pool status for `username`
    pub async fn get_key_package_status(&self, username: &str) -> Result<KeyPackagePoolStatus> {
        let response = self
            .client
            .get(format!("{}/keypackages/status/{}", self.base_url, username))
            .send()
            .await?;

        if response.status().is_success() {
            let status: KeyPackagePoolStatus = response.json().await?;
            Ok(status)
        } else {
            Err(NetworkError::Server(format!(
                "Failed to fetch pool status: {}",
                response.status()
            ))
            .into())
        }
    }
}
