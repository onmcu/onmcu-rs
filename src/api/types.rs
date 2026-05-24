use keyring_core::{Entry, Error as KeyringError};
use secrecy::SecretString;
use thiserror::Error;
use url::Url;

use crate::api::generated::{self, prelude::*};

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("No API key found. Please run `onmcu login` to set one.")]
    NoApiKey,

    #[error("{}", crate::keyring::unavailable_hint())]
    KeyringUnavailable(KeyringError),

    #[error("{}", crate::keyring::locked_hint())]
    KeyringLocked(KeyringError),

    #[error("Could not access keyring: {0}")]
    KeyringAccess(KeyringError),

    #[error("Invalid API key value: {0}")]
    InvalidApiKey(#[from] Box<generated::Error>),

    #[error("No API key found in env var ONMCU_API_KEY")]
    NoApiKeyInEnv(#[from] std::env::VarError),
}

impl From<KeyringError> for AuthError {
    fn from(e: KeyringError) -> Self {
        match e {
            KeyringError::NoEntry => AuthError::NoApiKey,
            e if crate::keyring::is_unavailable(&e) => AuthError::KeyringUnavailable(e),
            e if crate::keyring::is_locked(&e) => AuthError::KeyringLocked(e),
            e => AuthError::KeyringAccess(e),
        }
    }
}

pub struct AuthenticatedClient {
    // pub reqwest_client: ReqwestClient,
    pub api_client: Client,
    pub api_key: SecretString,
}

impl AuthenticatedClient {
    /// Create a new authenticated client with API key from keyring
    pub fn new_with_api_key(server_url: &Url, api_key_from_env: bool) -> Result<Self, AuthError> {
        // Only touch the keyring when not reading the key from the environment,
        // so ONMCU_API_KEY works even when no keyring backend is available.
        let api_key = if api_key_from_env {
            get_api_key_from_env()?
        } else {
            let entry = Entry::new("onmcu-cli", "api_key")?;
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

/// Read the API key from the keyring; `?` maps keyring errors to `AuthError`.
fn get_api_key(entry: &Entry) -> Result<SecretString, AuthError> {
    Ok(SecretString::from(entry.get_password()?))
}
