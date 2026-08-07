use keyring_core::{Entry, Error as KeyringError};
use secrecy::SecretString;
use thiserror::Error;
use url::Url;

use crate::api::generated::prelude::*;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("No API key found. Get one at https://app.onmcu.com/settings, then run `onmcu login`.")]
    NoApiKey,

    #[error("{}", crate::keyring::unavailable_hint())]
    KeyringUnavailable(KeyringError),

    #[error("{}", crate::keyring::locked_hint())]
    KeyringLocked(KeyringError),

    #[error("Could not access the OS keyring: {0}")]
    KeyringAccess(KeyringError),

    #[error("--api-key-from-env was set, but ONMCU_API_KEY is missing from the environment.")]
    NoApiKeyInEnv,

    #[error("ONMCU_API_KEY is set but does not contain valid UTF-8.")]
    EnvKeyNotUnicode,

    #[error("ONMCU_API_KEY is set but malformed: {0}")]
    InvalidEnvKey(#[from] ApiKeyFormatError),
}

/// Why an API key failed the `<version>_<uuid>_<secret>` format check.
#[derive(Error, Debug)]
pub enum ApiKeyFormatError {
    #[error("API key is empty")]
    Empty,

    #[error("Invalid API key format. Expected format: <version>_<uuid>_<secret>")]
    MissingParts,

    #[error("Invalid API key: version '{0}' is not a number")]
    Version(String),

    #[error("Invalid API key: '{0}' is not a valid UUID")]
    Uuid(String),

    #[error("Invalid API key: secret part is empty")]
    EmptySecret,
}

/// Validate API key format: `<version>_<uuid>_<base64-secret>`
///
/// # Errors
/// Returns an [`ApiKeyFormatError`] explaining what the exact format issue was.
pub fn validate_api_key(key: &str) -> Result<(), ApiKeyFormatError> {
    if key.is_empty() {
        return Err(ApiKeyFormatError::Empty);
    }

    let parts: Vec<&str> = key.splitn(3, '_').collect();
    if parts.len() != 3 {
        return Err(ApiKeyFormatError::MissingParts);
    }

    let [version, uuid, secret] = [parts[0], parts[1], parts[2]];

    if version.parse::<u16>().is_err() {
        return Err(ApiKeyFormatError::Version(version.to_owned()));
    }

    if uuid::Uuid::try_parse(uuid).is_err() {
        return Err(ApiKeyFormatError::Uuid(uuid.to_owned()));
    }

    if secret.is_empty() {
        return Err(ApiKeyFormatError::EmptySecret);
    }

    Ok(())
}

impl From<KeyringError> for AuthError {
    fn from(e: KeyringError) -> Self {
        match e {
            KeyringError::NoEntry => Self::NoApiKey,
            e if crate::keyring::is_unavailable(&e) => Self::KeyringUnavailable(e),
            e if crate::keyring::is_locked(&e) => Self::KeyringLocked(e),
            e => Self::KeyringAccess(e),
        }
    }
}

pub struct AuthenticatedClient {
    pub api_client: Client,
    pub api_key: SecretString,
}

impl AuthenticatedClient {
    /// Create a new authenticated client with API key from keyring
    ///
    /// # Errors
    /// Returns an [`AuthError`] if the API key could not be retrieved from the keyring.
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

        Ok(Self {
            api_client,
            api_key,
        })
    }

    /// Get the API client for making requests
    #[must_use]
    pub const fn api(&self) -> &Client {
        &self.api_client
    }
}

/// Helper function for API key retrieval from ENV
fn get_api_key_from_env() -> Result<SecretString, AuthError> {
    let key = match std::env::var("ONMCU_API_KEY") {
        Ok(key) => key,
        Err(std::env::VarError::NotPresent) => return Err(AuthError::NoApiKeyInEnv),
        Err(std::env::VarError::NotUnicode(_)) => return Err(AuthError::EnvKeyNotUnicode),
    };
    // Catch empty or malformed keys here instead of sending them to the
    // server and reporting a generic rejection.
    validate_api_key(&key)?;
    Ok(SecretString::from(key))
}

/// Read the API key from the keyring; `?` maps keyring errors to `AuthError`.
fn get_api_key(entry: &Entry) -> Result<SecretString, AuthError> {
    Ok(SecretString::from(entry.get_password()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_api_key() {
        let key =
            "1_1234abcd-ef10-1112-1314-1516171819aa_CDrt-jdp8r9FOpxj7dF7G9jwp5nTdlBUIQrAsD9oPLM=";
        assert!(validate_api_key(key).is_ok());
    }

    #[test]
    fn empty_key_rejected() {
        assert!(validate_api_key("").is_err());
    }

    #[test]
    fn missing_parts_rejected() {
        assert!(validate_api_key("just-a-string").is_err());
        assert!(validate_api_key("1_no-secret-part").is_err());
    }

    #[test]
    fn invalid_version_rejected() {
        assert!(validate_api_key("abc_1234abcd-ef10-1112-1314-1516171819aa_secret").is_err());
    }

    #[test]
    fn invalid_uuid_rejected() {
        assert!(validate_api_key("1_not-a-uuid_secret").is_err());
    }
}
