//! CLI interface for the MLS client
//!
//! Provides command parsing and async stdin reading for concurrent I/O
//! in the main message loop.

use crate::client::MlsClient;
use crate::error::Result;
use crate::models::Command;
use std::io::Write;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Run the main client control loop
///
/// Implements the concurrent I/O event loop:
/// - Reads user commands from stdin (parse and execute)
/// - Processes incoming messages from WebSocket
/// - Delegates MLS operations to client (which delegates to connection/membership)
/// - Displays messages per approved architecture (membership returns data, cli displays)
///
/// # Errors
/// * WebSocket connection errors
/// * I/O errors
/// * Command execution errors
pub async fn run_client_loop(client: &mut MlsClient) -> Result<()> {
    let group_name = client.get_current_group_name()?;

    println!("Connected to group: {}", group_name);
    println!("Commands: /invite <username>, /list, /quit");
    println!("Type messages to send to the group");

    // Initialize async stdin reader
    let stdin = tokio::io::stdin();
    let mut stdin_reader = BufReader::new(stdin);

    // Main concurrent I/O loop
    loop {
        tokio::select! {
            // === Handle user input ===
            user_input = read_line_async(&mut stdin_reader) => {
                match user_input {
                    Ok(Some(input)) => {
                        // Parse and process the command
                        match parse_command(&input) {
                            Ok(command) => {
                                match command {
                                    Command::Invite(invitee) => {
                                        match client.invite_user(&invitee).await {
                                            Ok(()) => {
                                                log::info!("Invited {} to the group", invitee);
                                            }
                                            Err(e) => {
                                                log::error!("Failed to invite {}: {}", invitee, e);
                                                eprintln!("Error: Failed to invite {}: {}", invitee, e);
                                            }
                                        }
                                    }
                                    Command::List => {
                                        let members = client.list_members();
                                        if members.is_empty() {
                                            println!("{}", format_control(&group_name, "no members yet"));
                                        } else {
                                            println!("{}", format_control(
                                                &group_name,
                                                &format!("members: {}", members.join(", "))
                                            ));
                                        }
                                    }
                                    Command::Message(text) => {
                                        match client.send_message(&text).await {
                                            Ok(()) => {
                                                log::debug!("Message sent successfully");
                                            }
                                            Err(e) => {
                                                log::error!("Failed to send message: {}", e);
                                                eprintln!("Error: Failed to send message: {}", e);
                                            }
                                        }
                                    }
                                    Command::Quit => {
                                        println!("Goodbye!");
                                        return Ok(());
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!("Invalid command: {}", e);
                                eprintln!("Error: Invalid command: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        // EOF (Ctrl+D)
                        log::info!("EOF received, exiting");
                        println!("Goodbye!");
                        return Ok(());
                    }
                    Err(e) => {
                        log::error!("Error reading input: {}", e);
                        eprintln!("Error reading input: {}", e);
                        return Err(e);
                    }
                }
            }

            // === Handle incoming messages ===
            incoming = client.get_connection_mut().next_envelope() => {
                match incoming {
                    Ok(Some(envelope)) => {
                        // Process the incoming envelope via connection's routing hub
                        // (membership will handle display internally for now)
                        match client.get_connection_mut().process_incoming_envelope(envelope).await {
                            Ok(()) => {
                                // Message processed successfully (display handled by membership)
                            }
                            Err(e) => {
                                log::error!("Failed to process incoming message: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        log::info!("WebSocket connection closed by server");
                        eprintln!("WebSocket connection closed");
                        return Ok(());
                    }
                    Err(e) => {
                        log::error!("WebSocket error: {}", e);
                        eprintln!("WebSocket error: {}", e);
                        return Err(e);
                    }
                }
            }
        }
    }
}

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
