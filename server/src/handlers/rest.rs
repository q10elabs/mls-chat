/// REST API handlers for HTTP endpoints.
/// Handles user registration, key retrieval, and backup management.
use crate::db::{
    keypackage_store::KeyPackageStatus, keypackage_store::KeyPackageStore, models::*, Database,
    DbPool,
};
use actix_web::{web, HttpResponse, Result as ActixResult};
use base64::{engine::general_purpose, Engine as _};
use serde_json::json;

#[derive(Debug, serde::Deserialize)]
pub struct UploadKeyPackageItem {
    keypackage_ref: String,
    keypackage: String,
    not_after: i64,
}

#[derive(Debug, serde::Deserialize)]
pub struct UploadKeyPackagesRequest {
    username: String,
    keypackages: Vec<UploadKeyPackageItem>,
}

#[derive(Debug, serde::Serialize)]
struct UploadKeyPackagesResponse {
    accepted: usize,
    rejected: Vec<String>,
    pool_size: usize,
}

/// Register a new user with their key package
/// POST /users
pub async fn register_user(
    pool: web::Data<DbPool>,
    req: web::Json<RegisterUserRequest>,
) -> ActixResult<HttpResponse> {
    match Database::register_user(&pool, &req.username, &req.key_package).await {
        Ok(user) => {
            let response = RegisterUserResponse {
                id: user.id,
                username: user.username,
                created_at: user.created_at,
            };
            Ok(HttpResponse::Created().json(response))
        }
        Err(e) => {
            log::error!("Failed to register user: {}", e);
            if e.to_string().contains("UNIQUE constraint failed") {
                Ok(HttpResponse::Conflict().json(json!({
                    "error": "Username already exists"
                })))
            } else {
                Ok(HttpResponse::InternalServerError().json(json!({
                    "error": "Failed to register user"
                })))
            }
        }
    }
}

/// Get a user's key package
/// GET /users/:username
pub async fn get_user_key(
    pool: web::Data<DbPool>,
    username: web::Path<String>,
) -> ActixResult<HttpResponse> {
    match Database::get_user(&pool, &username).await {
        Ok(Some(user)) => {
            let response = UserKeyResponse {
                username: user.username,
                key_package: user.key_package,
            };
            Ok(HttpResponse::Ok().json(response))
        }
        Ok(None) => Ok(HttpResponse::NotFound().json(json!({
            "error": "User not found"
        }))),
        Err(e) => {
            log::error!("Database error: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to retrieve user"
            })))
        }
    }
}

/// Store encrypted state backup
/// POST /backup/:username
pub async fn store_backup(
    pool: web::Data<DbPool>,
    username: web::Path<String>,
    req: web::Json<StoreBackupRequest>,
) -> ActixResult<HttpResponse> {
    // Verify user exists
    match Database::get_user(&pool, &username).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Ok(HttpResponse::NotFound().json(json!({
                "error": "User not found"
            })))
        }
        Err(e) => {
            log::error!("Database error: {}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to check user"
            })));
        }
    }

    match Database::store_backup(&pool, &username, &req.encrypted_state).await {
        Ok(backup) => {
            let response = BackupResponse {
                username: backup.username,
                encrypted_state: backup.encrypted_state,
                timestamp: backup.timestamp,
            };
            Ok(HttpResponse::Ok().json(response))
        }
        Err(e) => {
            log::error!("Failed to store backup: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to store backup"
            })))
        }
    }
}

/// Retrieve encrypted state backup
/// GET /backup/:username
pub async fn get_backup(
    pool: web::Data<DbPool>,
    username: web::Path<String>,
) -> ActixResult<HttpResponse> {
    match Database::get_backup(&pool, &username).await {
        Ok(Some(backup)) => {
            let response = BackupResponse {
                username: backup.username,
                encrypted_state: backup.encrypted_state,
                timestamp: backup.timestamp,
            };
            Ok(HttpResponse::Ok().json(response))
        }
        Ok(None) => Ok(HttpResponse::NotFound().json(json!({
            "error": "No backup found"
        }))),
        Err(e) => {
            log::error!("Database error: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to retrieve backup"
            })))
        }
    }
}

/// Health check endpoint
/// GET /health
pub async fn health() -> ActixResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "ok"
    })))
}

/// Upload a batch of KeyPackages for a user
/// POST /keypackages/upload
pub async fn upload_key_packages(
    pool: web::Data<DbPool>,
    req: web::Json<UploadKeyPackagesRequest>,
) -> ActixResult<HttpResponse> {
    if req.keypackages.is_empty() {
        return Ok(HttpResponse::BadRequest().json(json!({
            "error": "No keypackages provided"
        })));
    }

    let mut accepted = 0usize;
    let mut rejected = Vec::new();

    for item in &req.keypackages {
        let ref_bytes: Vec<u8> = match general_purpose::STANDARD.decode(&item.keypackage_ref) {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!("Invalid keypackage_ref for user {}: {}", req.username, err);
                rejected.push(item.keypackage_ref.clone());
                continue;
            }
        };

        let package_bytes: Vec<u8> = match general_purpose::STANDARD.decode(&item.keypackage) {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!(
                    "Invalid keypackage bytes for user {}: {}",
                    req.username,
                    err
                );
                rejected.push(item.keypackage_ref.clone());
                continue;
            }
        };

        match KeyPackageStore::save_key_package(
            &pool,
            &req.username,
            &ref_bytes,
            &package_bytes,
            item.not_after,
            None,
            None,
        )
        .await
        {
            Ok(_) => {
                accepted += 1;
                log::debug!(
                    "Stored keypackage for user {} (ref={})",
                    req.username,
                    item.keypackage_ref
                );
            }
            Err(err) => {
                log::warn!(
                    "Failed to store keypackage for user {}: {}",
                    req.username,
                    err
                );
                rejected.push(item.keypackage_ref.clone());
            }
        }
    }

    let pool_size =
        KeyPackageStore::count_by_status(&pool, &req.username, KeyPackageStatus::Available)
            .await
            .unwrap_or(0);

    Ok(HttpResponse::Ok().json(UploadKeyPackagesResponse {
        accepted,
        rejected,
        pool_size,
    }))
}
