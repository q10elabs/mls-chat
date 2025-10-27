//! CLI interface for the MLS client
//!
//! Provides command parsing and async stdin reading for concurrent I/O
//! in the main message loop.

use crate::models::Command;
use crate::error::Result;
use std::io::Write;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Parse a command from user input
pub fn parse_command(input: &str) -> Result<Command> {
    Command::parse(input).map_err(crate::error::ClientError::InvalidCommand)
}

/// Format a message for display
pub fn format_message(group: &str, username: &str, text: &str) -> String {
    format!("#{} <{}> {}", group, username, text)
}

/// Format a control message for display
pub fn format_control(group: &str, action: &str) -> String {
    format!("#{} {}", group, action)
}

/// Async stdin reader that yields one line at a time
///
/// Uses tokio's async stdin to enable concurrent I/O with WebSocket messages.
/// Prints the prompt and flushes stdout before blocking on input.
///
/// # Returns
/// - `Ok(Some(line))` - User entered a line
/// - `Ok(None)` - EOF reached (Ctrl+D)
/// - `Err(e)` - I/O error
pub async fn read_line_async(reader: &mut BufReader<tokio::io::Stdin>) -> Result<Option<String>> {
    use std::io::stdout;

    // Print prompt and flush
    print!("> ");
    stdout().flush().unwrap();

    // Wait for a line asynchronously
    let mut line = String::new();
    match reader.read_line(&mut line).await {
        Ok(0) => Ok(None), // EOF
        Ok(_) => {
            // Remove trailing newline
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(Some(line))
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_invite_command() {
        let result = parse_command("/invite alice");
        assert!(matches!(result, Ok(Command::Invite(username)) if username == "alice"));
    }

    #[test]
    fn test_parse_list_command() {
        let result = parse_command("/list");
        assert!(matches!(result, Ok(Command::List)));
    }

    #[test]
    fn test_parse_regular_message() {
        let result = parse_command("Hello world");
        assert!(matches!(result, Ok(Command::Message(msg)) if msg == "Hello world"));
    }

    #[test]
    fn test_format_message() {
        let formatted = format_message("testgroup", "alice", "Hello!");
        assert_eq!(formatted, "#testgroup <alice> Hello!");
    }

    #[test]
    fn test_format_control_message() {
        let formatted = format_control("testgroup", "alice joined the group");
        assert_eq!(formatted, "#testgroup alice joined the group");
    }

    #[test]
    fn test_invalid_command() {
        let result = parse_command("/unknown");
        assert!(result.is_err());
    }
}
