use keyring_core::{Entry, Error as KeyringError};
use secrecy::SecretString;
use thiserror::Error;
use url::Url;

use crate::api::generated::{self, prelude::*};

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("No API key found. Please run `onmcu login` to set one.")]
    NoApiKey,

    #[error("Could not access keyring: {0}")]
    KeyringAccess(#[from] KeyringError),

    #[error("Invalid API key value: {0}")]
    InvalidApiKey(#[from] Box<generated::Error>),

    #[error("No API key found in env var ONMCU_API_KEY")]
    NoApiKeyInEnv(#[from] std::env::VarError),
}

pub struct AuthenticatedClient {
    // pub reqwest_client: ReqwestClient,
    pub api_client: Client,
    pub api_key: SecretString,
}

impl AuthenticatedClient {
    /// Create a new authenticated client with API key from keyring
    pub fn new_with_api_key(server_url: &Url, api_key_from_env: bool) -> Result<Self, AuthError> {
        let entry = Entry::new("onmcu-cli", "api_key")?;

        let api_key = if api_key_from_env {
            get_api_key_from_env()?
        } else {
            get_api_key(&entry)?
        };

        let api_client = Client::new(server_url.as_str().trim_end_matches('/'));

        Ok(AuthenticatedClient {
            api_client,
            api_key,
        })
    }

    /// Get the API client for making requests
    pub fn api(&self) -> &Client {
        &self.api_client
    }

    // /// Get the raw reqwest client if needed for custom requests
    // pub fn reqwest(&self) -> &ReqwestClient {
    //     &self.reqwest_client
    // }
}

/// Helper function for API key retrieval from ENV
fn get_api_key_from_env() -> Result<SecretString, AuthError> {
    match std::env::var("ONMCU_API_KEY") {
        Ok(key) => Ok(SecretString::from(key)),
        Err(e) => Err(AuthError::NoApiKeyInEnv(e)),
    }
}

/// Helper function for API key retrieval from keyring
fn get_api_key(entry: &Entry) -> Result<SecretString, AuthError> {
    match entry.get_password() {
        Ok(key) => Ok(SecretString::from(key)),
        Err(KeyringError::NoEntry) => Err(AuthError::NoApiKey),
        Err(e) => Err(AuthError::KeyringAccess(e)),
    }
}
