use keyring_core::{Entry, Error as KeyringError};
use std::io::{self, Write};
use thiserror::Error;
use tracing::info;

use crate::api::AuthError;

#[derive(Error, Debug)]
pub enum LoginError {
    #[error(transparent)]
    Keyring(#[from] AuthError),

    #[error("Failed to save API key to your OS keyring: {0}")]
    SaveKeyring(AuthError),

    #[error("Terminal input or output failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("No API key entered")]
    EmptyKey,

    #[error("Invalid API key format. Expected format: <version>_<uuid>_<secret>")]
    InvalidFormat,

    #[error("Invalid API key: version '{0}' is not a number")]
    InvalidVersion(String),

    #[error("Invalid API key: '{0}' is not a valid UUID")]
    InvalidUuid(String),

    #[error("Invalid API key: secret part is empty")]
    EmptySecret,
}

/// `onmcu login [--relogin]`
pub async fn handle_login(relogin: bool) -> Result<(), LoginError> {
    let entry = Entry::new("onmcu-cli", "api_key").map_err(AuthError::from)?;
    match entry.get_password() {
        Ok(_) if !relogin => {
            eprintln!("Already logged in. To overwrite, run `onmcu login --relogin`.");
            return Ok(());
        }
        Ok(_) | Err(KeyringError::NoEntry) => { /* fall through to prompt */ }
        Err(e) => return Err(AuthError::from(e).into()),
    }
    // Prompt for new API key
    print!("Enter your API key, it can be retrieved at https://app.onmcu.com/settings: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;

    let key = buf.trim();
    validate_api_key(key)?;

    // Store it
    entry
        .set_password(key)
        .map_err(AuthError::from)
        .map_err(LoginError::SaveKeyring)?;
    info!("✅  API key saved.");

    Ok(())
}

/// Validate API key format: `<version>_<uuid>_<base64-secret>`
fn validate_api_key(key: &str) -> Result<(), LoginError> {
    if key.is_empty() {
        return Err(LoginError::EmptyKey);
    }

    let parts: Vec<&str> = key.splitn(3, '_').collect();
    if parts.len() != 3 {
        return Err(LoginError::InvalidFormat);
    }

    let [version, uuid, secret] = [parts[0], parts[1], parts[2]];

    if version.parse::<u16>().is_err() {
        return Err(LoginError::InvalidVersion(version.to_owned()));
    }

    if uuid::Uuid::try_parse(uuid).is_err() {
        return Err(LoginError::InvalidUuid(uuid.to_owned()));
    }

    if secret.is_empty() {
        return Err(LoginError::EmptySecret);
    }

    Ok(())
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
