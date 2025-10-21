/// MLS Chat Client Library
/// Provides MLS group messaging functionality with OpenMLS

pub mod api;
pub mod cli;
pub mod client;
pub mod crypto;
pub mod error;
pub mod identity;
pub mod models;
pub mod provider;
pub mod storage;
pub mod websocket;

pub use error::{ClientError, Result};
pub use identity::{IdentityManager, StoredIdentity};
pub use provider::MlsProvider;
