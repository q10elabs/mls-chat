//! MLS architecture components
//!
//! This module contains the refactored MLS architecture:
//! - `user`: User identity and credential management
//! - Future: `membership` for group session state
//! - Future: `connection` for infrastructure and message routing

pub mod user;

// Re-export for convenience
pub use user::MlsUser;
