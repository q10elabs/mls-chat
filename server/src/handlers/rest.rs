/// REST API handlers for HTTP endpoints.
/// Handles user registration, key retrieval, and backup management.

use crate::db::{models::*, Database, DbPool};
use actix_web::{web, HttpResponse, Result as ActixResult};
use serde_json::json;

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
        Ok(Some(_)) => {},
        Ok(None) => return Ok(HttpResponse::NotFound().json(json!({
            "error": "User not found"
        }))),
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

