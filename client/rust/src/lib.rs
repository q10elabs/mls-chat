/// MLS Chat Client Library
/// Provides a comprehensive API for building OpenMLS-based chat applications.

pub mod error;
pub mod models;
pub mod services;

pub use error::{ClientError, Result};
pub use models::{Group, GroupId, Message, MessageId, User, UserId};
pub use services::{ClientManager, GroupService, MessageService, MlsService, ServerClient, StorageService};
