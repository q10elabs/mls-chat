//! MLS architecture components
//!
//! This module contains the refactored MLS architecture:
//! - `user`: User identity and credential management
//! - `membership`: Group session state and operations
//! - Future: `connection` for infrastructure and message routing

pub mod user;
pub mod membership;

// Re-export for convenience
pub use user::MlsUser;
pub use membership::MlsMembership;
