/// Main MLS client orchestrator
///
/// Coordinates MLS operations using OpenMlsProvider for automatic group state persistence.

use crate::api::ServerApi;
use crate::cli::{format_control, format_message, run_input_loop};
use crate::error::{Result, ClientError};
use crate::models::{Command, Identity};
use crate::provider::MlsProvider;
use crate::storage::LocalStore;
use crate::websocket::MessageHandler;
use directories::BaseDirs;

/// Main MLS client
pub struct MlsClient {
    server_url: String,
    username: String,
    group_name: String,
    metadata_store: LocalStore,
    mls_provider: MlsProvider,
    api: ServerApi,
    websocket: Option<MessageHandler>,
    identity: Option<Identity>,
}

impl MlsClient {
    /// Create a new MLS client
    ///
    /// # Arguments
    /// * `server_url` - URL of the MLS server
    /// * `username` - Username for this client instance
    /// * `group_name` - Name of the group to create/join
    ///
    /// # Errors
    /// * File system errors when creating storage directories
    /// * Database initialization errors
    pub async fn new(server_url: &str, username: &str, group_name: &str) -> Result<Self> {
        // Get storage paths
        let base_dirs = BaseDirs::new()
            .ok_or_else(|| ClientError::Config("Failed to get home directory".to_string()))?;
        let mlschat_dir = base_dirs.home_dir().join(".mlschat");

        // Ensure directory exists
        std::fs::create_dir_all(&mlschat_dir)?;

        // Metadata storage (application-level only)
        let metadata_db_path = mlschat_dir.join("metadata.db");
        let metadata_store = LocalStore::new(&metadata_db_path)?;

        // MLS provider storage (handles all OpenMLS group state)
        let mls_db_path = mlschat_dir.join("mls.db");
        let mls_provider = MlsProvider::new(&mls_db_path)?;

        let api = ServerApi::new(server_url);

        Ok(Self {
            server_url: server_url.to_string(),
            username: username.to_string(),
            group_name: group_name.to_string(),
            metadata_store,
            mls_provider,
            api,
            websocket: None,
            identity: None,
        })
    }

    /// Initialize the client (load or create identity, register with server)
    ///
    /// # Errors
    /// * Storage errors when loading/saving identity
    /// * Network errors when registering with server
    pub async fn initialize(&mut self) -> Result<()> {
        // Try to load existing identity
        if let Some((keypair_blob, credential_blob)) = self.metadata_store.load_identity(&self.username)? {
            self.identity = Some(Identity {
                username: self.username.clone(),
                keypair_blob,
                credential_blob,
            });
            log::info!("Loaded existing identity for {}", self.username);
        } else {
            // Create new identity - In a real implementation, generate actual MLS credential
            // For now, we'll create placeholder data
            log::info!("Creating new identity for {}", self.username);
            let keypair_blob = b"placeholder_keypair".to_vec();
            let credential_blob = b"placeholder_credential".to_vec();

            self.metadata_store.save_identity(&self.username, &keypair_blob, &credential_blob)?;
            self.identity = Some(Identity {
                username: self.username.clone(),
                keypair_blob,
                credential_blob,
            });
        }

        // Register with server (idempotent)
        let public_key = "placeholder_public_key"; // Extract from keypair in real implementation
        self.api.register_user(&self.username, public_key).await?;

        Ok(())
    }

    /// Connect to group (create or load existing)
    ///
    /// Group state is automatically managed by the OpenMlsProvider.
    /// This method just connects the WebSocket for real-time messaging.
    ///
    /// # Errors
    /// * WebSocket connection errors
    pub async fn connect_to_group(&mut self) -> Result<()> {
        // Group creation/loading is handled by the MLS provider transparently.
        // Here we just ensure WebSocket is connected for real-time messaging.
        log::info!("Connecting to group: {}", self.group_name);

        // Connect WebSocket
        self.websocket = Some(MessageHandler::connect(&self.server_url, &self.username).await?);
        self.websocket.as_ref().unwrap().subscribe_to_group(&self.group_name).await?;

        Ok(())
    }

    /// Send a message to the group
    ///
    /// # Errors
    /// * WebSocket send errors
    /// * Encryption errors (in real implementation)
    pub async fn send_message(&mut self, text: &str) -> Result<()> {
        if let Some(websocket) = &self.websocket {
            // In a real implementation:
            // 1. Get group from provider: MlsGroup::load(self.mls_provider.storage(), &group_id)?
            // 2. Encrypt message: crypto::create_application_message(&mut group, &self.mls_provider, ...)?
            // 3. Send encrypted bytes
            // For now, placeholder encryption
            let encrypted_content = format!("encrypted:{}", text);
            websocket.send_message(&self.group_name, &encrypted_content).await?;
            println!("{}", format_message(&self.group_name, &self.username, text));
        }
        Ok(())
    }

    /// Process incoming messages
    ///
    /// # Errors
    /// * WebSocket receive errors
    /// * Decryption errors (in real implementation)
    pub async fn process_incoming(&mut self) -> Result<()> {
        if let Some(websocket) = &mut self.websocket {
            if let Some(msg) = websocket.next_message().await? {
                // In a real implementation:
                // 1. Get group from provider: MlsGroup::load(self.mls_provider.storage(), &msg.group_id)?
                // 2. Process message: crypto::process_message(&mut group, &self.mls_provider, ...)?
                // 3. Decrypt and display
                // For now, placeholder decryption
                let decrypted = format!("decrypted:{}", msg.encrypted_content);
                println!("{}", format_message(&msg.group_id, &msg.sender, &decrypted));
            }
        }
        Ok(())
    }

    /// Invite a user to the group
    ///
    /// In a real implementation, this would:
    /// 1. Get invitee's key package from server
    /// 2. Load group: MlsGroup::load(self.mls_provider.storage(), &group_id)?
    /// 3. Add member: crypto::add_members(&mut group, &self.mls_provider, ...)?
    /// 4. Send Welcome message via WebSocket
    /// 5. Save group state (automatic via provider)
    ///
    /// # Errors
    /// * Server communication errors
    /// * MLS operation errors (in real implementation)
    pub async fn invite_user(&mut self, invitee_username: &str) -> Result<()> {
        log::info!("Inviting {} to group {}", invitee_username, self.group_name);
        println!(
            "{}",
            format_control(
                &self.group_name,
                &format!("invited {} to the group", invitee_username)
            )
        );
        Ok(())
    }

    /// List group members
    ///
    /// Returns the members list stored in metadata. In a real implementation,
    /// this would come from the actual group state in the MLS provider.
    pub fn list_members(&self) -> Vec<String> {
        // Load from metadata store (or reconstruct from MLS group state)
        self.metadata_store
            .get_group_members(&self.username, &self.group_name)
            .unwrap_or_else(|_| vec![self.username.clone()])
    }

    /// Run the main client loop
    pub async fn run(&mut self) -> Result<()> {
        println!("Connected to group: {}", self.group_name);
        println!("Commands: /invite <username>, /list, /quit");
        println!("Type messages to send to the group");
        
        // Spawn task for incoming messages
        let mut websocket = self.websocket.take().unwrap();
        
        tokio::spawn(async move {
            loop {
                if let Some(msg) = websocket.next_message().await.unwrap_or(None) {
                    let decrypted = format!("decrypted:{}", msg.encrypted_content);
                    println!("{}", format_message(&msg.group_id, &msg.sender, &decrypted));
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        });
        
        // Run input loop
        let group_name = self.group_name.clone();
        let username = self.username.clone();
        let members = self.list_members();
        
        run_input_loop(|command| {
            match command {
                Command::Invite(username) => {
                    // For now, just print a message
                    println!("{}", format_control(&group_name, &format!("invited {} to the group", username)));
                }
                Command::List => {
                    println!("Group members: {}", members.join(", "));
                }
                Command::Message(text) => {
                    // For now, just print the message
                    println!("{}", format_message(&group_name, &username, &text));
                }
                Command::Quit => {
                    println!("Goodbye!");
                    std::process::exit(0);
                }
            }
            Ok(())
        }).await?;
        
        Ok(())
    }
}
