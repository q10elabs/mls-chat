/// HTTP handlers module
/// Provides REST and WebSocket endpoints
pub mod rest;
pub mod websocket;

pub use rest::{
    get_backup, get_user_key, health, register_user, store_backup, upload_key_packages,
};
pub use websocket::{ws_connect, WsServer};
