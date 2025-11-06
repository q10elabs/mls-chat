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

#[derive(Debug, serde::Deserialize)]
pub struct ReserveKeyPackageRequest {
    target_username: String,
    reserved_by: String,
    group_id: String,
}

#[derive(Debug, serde::Serialize)]
struct ReserveKeyPackageResponse {
    keypackage_ref: String,
    keypackage: String,
    reservation_id: String,
    reservation_expires_at: i64,
    not_after: i64,
}

#[derive(Debug, serde::Deserialize)]
pub struct SpendKeyPackageRequest {
    keypackage_ref: String,
    group_id: String,
    spent_by: String,
}

#[derive(Debug, serde::Serialize)]
struct SpendKeyPackageResponse {
    spent: bool,
}

#[derive(Debug, serde::Serialize)]
struct KeyPackageStatusResponse {
    username: String,
    available: usize,
    reserved: usize,
    spent: usize,
    expired: usize,
    total: usize,
    expiring_soon: usize,
    pool_health: String,
    recommended_action: Option<String>,
    last_upload: Option<String>,
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

/// Reserve a KeyPackage for an invitation
/// POST /keypackages/reserve
pub async fn reserve_key_package(
    pool: web::Data<DbPool>,
    config: web::Data<crate::handlers::ServerConfig>,
    req: web::Json<ReserveKeyPackageRequest>,
) -> ActixResult<HttpResponse> {
    let group_id_bytes = match general_purpose::STANDARD.decode(&req.group_id) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Ok(HttpResponse::BadRequest().json(json!({
                "error": format!("Invalid group_id: {}", err)
            })));
        }
    };

    match KeyPackageStore::reserve_key_package_with_timeout(
        &pool,
        &req.target_username,
        &group_id_bytes,
        &req.reserved_by,
        config.reservation_timeout_seconds,
    )
    .await
    {
        Ok(Some(reserved)) => {
            let response = ReserveKeyPackageResponse {
                keypackage_ref: general_purpose::STANDARD.encode(&reserved.keypackage_ref),
                keypackage: general_purpose::STANDARD.encode(&reserved.keypackage_bytes),
                reservation_id: reserved.reservation_id,
                reservation_expires_at: reserved.reservation_expires_at,
                not_after: reserved.not_after,
            };

            Ok(HttpResponse::Ok().json(response))
        }
        Ok(None) => Ok(HttpResponse::NotFound().json(json!({
            "error": "No available KeyPackage for target user"
        }))),
        Err(err) => {
            log::error!(
                "Failed to reserve keypackage for {}: {}",
                req.target_username,
                err
            );
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to reserve keypackage"
            })))
        }
    }
}

/// Mark a reserved KeyPackage as spent
/// POST /keypackages/spend
pub async fn spend_key_package(
    pool: web::Data<DbPool>,
    req: web::Json<SpendKeyPackageRequest>,
) -> ActixResult<HttpResponse> {
    let keypackage_ref = match general_purpose::STANDARD.decode(&req.keypackage_ref) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Ok(HttpResponse::BadRequest().json(json!({
                "error": format!("Invalid keypackage_ref: {}", err)
            })));
        }
    };

    let group_id = match general_purpose::STANDARD.decode(&req.group_id) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Ok(HttpResponse::BadRequest().json(json!({
                "error": format!("Invalid group_id: {}", err)
            })));
        }
    };

    match KeyPackageStore::spend_key_package(&pool, &keypackage_ref, &group_id, &req.spent_by).await
    {
        Ok(()) => Ok(HttpResponse::Ok().json(SpendKeyPackageResponse { spent: true })),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(HttpResponse::NotFound().json(json!({
            "error": "KeyPackage not found"
        }))),
        Err(rusqlite::Error::ExecuteReturnedResults) => Ok(HttpResponse::Conflict().json(json!({
            "error": "KeyPackage already spent"
        }))),
        Err(err) => {
            log::error!("Failed to spend keypackage: {}", err);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to spend keypackage"
            })))
        }
    }
}

/// Get aggregate status for a user's KeyPackage pool
/// GET /keypackages/status/{username}
pub async fn get_keypackage_status(
    pool: web::Data<DbPool>,
    username: web::Path<String>,
) -> ActixResult<HttpResponse> {
    use rusqlite::OptionalExtension;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let expiring_threshold = now + 172_800; // 48 hours

    let conn = pool.lock().await;

    let available: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM keypackages WHERE username = ?1 AND status = 'available' AND not_after > ?2",
            (&username.as_str(), now),
            |row| row.get(0),
        )
        .unwrap_or(0);

    let reserved: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM keypackages WHERE username = ?1 AND status = 'reserved'",
            (&username.as_str(),),
            |row| row.get(0),
        )
        .unwrap_or(0);

    let spent: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM keypackages WHERE username = ?1 AND status = 'spent'",
            (&username.as_str(),),
            |row| row.get(0),
        )
        .unwrap_or(0);

    let expired: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM keypackages WHERE username = ?1 AND not_after <= ?2",
            (&username.as_str(), now),
            |row| row.get(0),
        )
        .unwrap_or(0);

    let expiring_soon: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM keypackages WHERE username = ?1 AND status = 'available' AND not_after BETWEEN ?2 AND ?3",
            (&username.as_str(), now, expiring_threshold),
            |row| row.get(0),
        )
        .unwrap_or(0);

    let last_upload: Option<i64> = conn
        .query_row(
            "SELECT MAX(uploaded_at) FROM keypackages WHERE username = ?1",
            (&username.as_str(),),
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    drop(conn);

    let total = (available + reserved + spent) as usize;

    let pool_health = if available == 0 {
        "empty"
    } else if available < 8 {
        "low"
    } else {
        "healthy"
    };

    let recommended_action = if available == 0 {
        Some("Upload new key packages immediately".to_string())
    } else if available < 8 {
        Some("Replenish key package pool".to_string())
    } else {
        None
    };

    let last_upload_str = last_upload
        .and_then(|ts| chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0))
        .map(|dt| dt.to_rfc3339());

    let response = KeyPackageStatusResponse {
        username: username.into_inner(),
        available: available as usize,
        reserved: reserved as usize,
        spent: spent as usize,
        expired: expired as usize,
        total,
        expiring_soon: expiring_soon as usize,
        pool_health: pool_health.to_string(),
        recommended_action,
        last_upload: last_upload_str,
    };

    Ok(HttpResponse::Ok().json(response))
}
