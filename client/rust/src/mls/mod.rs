//! MLS architecture components
//!
//! This module contains the refactored MLS architecture:
//! - `user`: User identity and credential management
//! - `membership`: Group session state and operations
//! - `connection`: Infrastructure and message routing

pub mod connection;
pub mod keypackage_pool;
pub mod membership;
pub mod user;

// Re-export for convenience
pub use connection::MlsConnection;
pub use keypackage_pool::{KeyPackagePool, KeyPackagePoolConfig};
pub use membership::MlsMembership;
pub use user::MlsUser;
