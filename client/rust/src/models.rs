//! Data models and DTOs for the MLS client

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

/// Envelope discriminator for WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum MlsMessageEnvelope {
    /// Application message: encrypted plaintext from group member
    #[serde(rename = "application")]
    ApplicationMessage {
        sender: String,
        group_id: String,
        encrypted_content: String,
    },
    /// Welcome message: new member joining the group
    /// Includes the Welcome message and ratchet tree in one envelope
    /// Note: No group_id field - group name is in encrypted GroupContext extensions
    #[serde(rename = "welcome")]
    WelcomeMessage {
        inviter: String,
        invitee: String, // Username of the person being invited (for server routing)
        welcome_blob: String, // TLS-serialized Welcome message (base64)
        ratchet_tree_blob: String, // Exported ratchet tree (base64)
    },
    /// Commit message: group state change notification
    #[serde(rename = "commit")]
    CommitMessage {
        group_id: String,
        sender: String,
        commit_blob: String, // TLS-serialized Commit message (base64)
    },
}

/// Incoming message from WebSocket (legacy, for compatibility)
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
        assert_eq!(
            Command::parse("/invite alice"),
            Ok(Command::Invite("alice".to_string()))
        );
        assert_eq!(Command::parse("/list"), Ok(Command::List));
        assert_eq!(
            Command::parse("Hello world"),
            Ok(Command::Message("Hello world".to_string()))
        );
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

    #[test]
    fn test_application_message_envelope_serialization() {
        let envelope = MlsMessageEnvelope::ApplicationMessage {
            sender: "alice".to_string(),
            group_id: "testgroup".to_string(),
            encrypted_content: "base64encrypteddata".to_string(),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("\"type\":\"application\""));
        assert!(json.contains("\"sender\":\"alice\""));

        let deserialized: MlsMessageEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope, deserialized);
    }

    #[test]
    fn test_welcome_message_envelope_serialization() {
        let envelope = MlsMessageEnvelope::WelcomeMessage {
            inviter: "alice".to_string(),
            invitee: "bob".to_string(),
            welcome_blob: "base64welcomeblob".to_string(),
            ratchet_tree_blob: "base64ratchettree".to_string(),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("\"type\":\"welcome\""));
        assert!(json.contains("\"inviter\":\"alice\""));
        assert!(json.contains("\"invitee\":\"bob\""));
        // Verify no group_id in the serialized form
        assert!(!json.contains("group_id"));

        let deserialized: MlsMessageEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope, deserialized);
    }

    #[test]
    fn test_commit_message_envelope_serialization() {
        let envelope = MlsMessageEnvelope::CommitMessage {
            group_id: "testgroup".to_string(),
            sender: "alice".to_string(),
            commit_blob: "base64commitblob".to_string(),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("\"type\":\"commit\""));
        assert!(json.contains("\"sender\":\"alice\""));

        let deserialized: MlsMessageEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope, deserialized);
    }

    #[test]
    fn test_envelope_type_discrimination() {
        let app_json =
            r#"{"type":"application","sender":"alice","group_id":"g1","encrypted_content":"data"}"#;
        let welcome_json = r#"{"type":"welcome","inviter":"alice","invitee":"bob","welcome_blob":"w","ratchet_tree_blob":"rt"}"#;
        let commit_json = r#"{"type":"commit","group_id":"g1","sender":"alice","commit_blob":"c"}"#;

        let app: MlsMessageEnvelope = serde_json::from_str(app_json).unwrap();
        let welcome: MlsMessageEnvelope = serde_json::from_str(welcome_json).unwrap();
        let commit: MlsMessageEnvelope = serde_json::from_str(commit_json).unwrap();

        assert!(matches!(app, MlsMessageEnvelope::ApplicationMessage { .. }));
        assert!(matches!(welcome, MlsMessageEnvelope::WelcomeMessage { .. }));
        assert!(matches!(commit, MlsMessageEnvelope::CommitMessage { .. }));
    }
}
