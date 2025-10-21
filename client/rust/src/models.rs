/// Data models and DTOs for the MLS client

use serde::{Deserialize, Serialize};

/// User identity information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub username: String,
    pub keypair_blob: Vec<u8>,
    pub credential_blob: Vec<u8>,
}

/// Group state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupState {
    pub group_id: String,
    pub mls_group_blob: Vec<u8>,
    pub members: Vec<String>,
}

/// Incoming message from WebSocket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    pub sender: String,
    pub group_id: String,
    pub encrypted_content: String,
}

/// Command types for CLI
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Invite(String),
    List,
    Message(String),
    Quit,
}

impl Command {
    /// Parse a command string
    pub fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        
        if input == "/quit" || input == "/exit" {
            return Ok(Command::Quit);
        }
        
        if input == "/list" {
            return Ok(Command::List);
        }
        
        if let Some(invitee) = input.strip_prefix("/invite ") {
            if invitee.is_empty() {
                return Err("Usage: /invite <username>".to_string());
            }
            return Ok(Command::Invite(invitee.to_string()));
        }
        
        if input.starts_with('/') {
            return Err(format!("Unknown command: {}", input));
        }
        
        Ok(Command::Message(input.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_parsing() {
        assert_eq!(Command::parse("/invite alice"), Ok(Command::Invite("alice".to_string())));
        assert_eq!(Command::parse("/list"), Ok(Command::List));
        assert_eq!(Command::parse("Hello world"), Ok(Command::Message("Hello world".to_string())));
        assert_eq!(Command::parse("/quit"), Ok(Command::Quit));
        assert_eq!(Command::parse("/exit"), Ok(Command::Quit));
        
        assert!(Command::parse("/unknown").is_err());
        assert!(Command::parse("/invite").is_err());
    }

    #[test]
    fn test_message_serialization() {
        let identity = Identity {
            username: "alice".to_string(),
            keypair_blob: vec![1, 2, 3],
            credential_blob: vec![4, 5, 6],
        };
        
        let json = serde_json::to_string(&identity).unwrap();
        let deserialized: Identity = serde_json::from_str(&json).unwrap();
        
        assert_eq!(identity.username, deserialized.username);
        assert_eq!(identity.keypair_blob, deserialized.keypair_blob);
        assert_eq!(identity.credential_blob, deserialized.credential_blob);
    }
}
