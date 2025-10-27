//! MLS architecture components
//!
//! This module contains the refactored MLS architecture:
//! - `user`: User identity and credential management
//! - `membership`: Group session state and operations
//! - `connection`: Infrastructure and message routing

pub mod user;
pub mod membership;
pub mod connection;

// Re-export for convenience
pub use user::MlsUser;
pub use membership::MlsMembership;
pub use connection::MlsConnection;
