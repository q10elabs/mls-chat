/// HTTP handlers module
/// Provides REST and WebSocket endpoints
pub mod rest;
pub mod websocket;

pub use rest::{
    get_backup, get_keypackage_status, get_user_key, health, register_user, reserve_key_package,
    spend_key_package, store_backup, upload_key_packages,
};
pub use websocket::{ws_connect, WsServer};

/// Server configuration shared across handlers
#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub reservation_timeout_seconds: i64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            reservation_timeout_seconds: 60,
        }
    }
}
