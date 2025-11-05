/// Configuration management for the MLS chat server.
/// Handles command-line argument parsing and config structure.
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "MLS Chat Server")]
#[command(about = "OpenMLS-based group chat server", long_about = None)]
pub struct Config {
    /// Server port (default: 4000)
    #[arg(long, default_value = "4000")]
    pub port: u16,

    /// SQLite database file path (default: chatserver.db)
    #[arg(long, default_value = "chatserver.db")]
    pub database: PathBuf,

    /// PID file path (optional) - write server PID to this file on startup
    #[arg(long)]
    pub pidfile: Option<PathBuf>,
}

impl Config {
    /// Parse command-line arguments into Config
    pub fn from_args() -> Self {
        Config::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config {
            port: 4000,
            database: PathBuf::from("chatserver.db"),
            pidfile: None,
        };
        assert_eq!(config.port, 4000);
        assert_eq!(config.database.to_str().unwrap(), "chatserver.db");
    }

    #[test]
    fn test_custom_port() {
        let config = Config {
            port: 8080,
            database: PathBuf::from("chatserver.db"),
            pidfile: None,
        };
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_custom_database() {
        let config = Config {
            port: 4000,
            database: PathBuf::from("/tmp/custom.db"),
            pidfile: None,
        };
        assert_eq!(config.database.to_str().unwrap(), "/tmp/custom.db");
    }
}
