/// Service layer for the MLS chat client.
/// Provides business logic abstraction over lower-level operations.

pub mod storage;
pub mod server_client;
pub mod mls_service;
pub mod group_service;
pub mod message_service;
pub mod client_manager;
pub mod websocket_manager;

pub use storage::StorageService;
pub use server_client::ServerClient;
pub use mls_service::MlsService;
pub use group_service::GroupService;
pub use message_service::MessageService;
pub use client_manager::ClientManager;
pub use websocket_manager::{ConnectionState, WebSocketManager};
