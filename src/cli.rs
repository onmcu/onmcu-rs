use clap::{Parser, Subcommand, crate_description, crate_name, crate_version};
use std::path::PathBuf;

use crate::{
    commands::{list_boards, login, run},
    error::CliError,
    upload::UploadConfig,
};

#[derive(Parser, Debug)]
#[command(author)]
#[command(name = crate_name!())]
#[command(about = crate_description!())]
#[command(version = crate_version!())]
pub struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true, default_value_t = false)]
    pub verbose: bool,

    /// Path to the configuration file
    #[arg(short, long, env = "ONMCU_CLI_CONFIG_PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

impl Cli {
    pub async fn dispatch(self) -> Result<(), CliError> {
        // Get config from CLI argument path or construct default
        let mut cfg = match self.config {
            Some(ref path) => UploadConfig::from_file(path)?,
            None => UploadConfig::default(),
        };

        match self.command {
            Commands::Run {
                board,
                file,
                api_key_from_env,
                timeout,
                wait_timeout,
            } => {
                // Apply CLI argument timeout to config
                if let Some(timeout) = timeout {
                    cfg.timeout_seconds = timeout;
                }
                run::handle_run(cfg, board, file, api_key_from_env, wait_timeout).await
            }
            Commands::Login { relogin } => {
                login::handle_login(relogin).await.map_err(CliError::from)
            }
            Commands::ListBoards { api_key_from_env } => {
                list_boards::handle_list_boards(cfg, api_key_from_env).await
            }
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run and flash firmware to MCU
    Run {
        /// Target board name
        #[arg(long)]
        board: String,
        /// Path to the binary or package to flash
        #[arg(short, long)]
        file: PathBuf,
        /// Read API key from env var ONMCU_API_KEY
        #[arg(long)]
        api_key_from_env: bool,
        /// Job timeout in seconds (59-86400, default: 600)
        #[arg(short, long, value_parser = clap::value_parser!(u32).range(59..=86400))]
        timeout: Option<u32>,
        /// How long to wait for a device to become available (seconds, default: 300)
        #[arg(long, default_value_t = 300)]
        wait_timeout: u64,
    },
    /// Store the API Key into the OS keyring
    Login {
        /// Replace existing key even if one is already stored
        #[arg(short, long)]
        relogin: bool,
    },
    /// List the available boards
    ListBoards {
        /// Read API key from env var ONMCU_API_KEY
        #[arg(long)]
        api_key_from_env: bool,
    },
}

pub fn build() -> Cli {
    Cli::parse()
}
