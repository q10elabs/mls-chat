/// CLI interface for the MLS client

use crate::models::Command;
use crate::error::Result;
use std::io::{Write};

/// Parse a command from user input
pub fn parse_command(input: &str) -> Result<Command> {
    Command::parse(input).map_err(|e| crate::error::ClientError::InvalidCommand(e))
}

/// Format a message for display
pub fn format_message(group: &str, username: &str, text: &str) -> String {
    format!("#{} <{}> {}", group, username, text)
}

/// Format a control message for display
pub fn format_control(group: &str, action: &str) -> String {
    format!("#{} {}", group, action)
}

/// Run the input loop (placeholder for now)
pub async fn run_input_loop<F>(mut callback: F) -> Result<()>
where
    F: FnMut(Command) -> Result<()>,
{
    use std::io::{self, BufRead};
    
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    
    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        
        if let Some(line) = lines.next() {
            let input = line?;
            let command = parse_command(&input)?;
            
            match command {
                Command::Quit => break,
                _ => callback(command)?,
            }
        }
    }
    
    Ok(())
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
