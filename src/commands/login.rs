use keyring_core::{Entry, Error as KeyringError};
use secrecy::zeroize::Zeroize as _;
use secrecy::{ExposeSecret as _, SecretString};
use std::io::{self, IsTerminal as _, Write};
use thiserror::Error;

use crate::api::{ApiKeyFormatError, AuthError, validate_api_key};

#[derive(Error, Debug)]
pub enum LoginError {
    #[error(transparent)]
    Keyring(#[from] AuthError),

    #[error("Failed to save API key to your OS keyring: {0}")]
    SaveKeyring(AuthError),

    #[error("Terminal input or output failed: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    InvalidKey(#[from] ApiKeyFormatError),
}

/// `onmcu login [--relogin]`
pub fn handle_login(relogin: bool) -> Result<(), LoginError> {
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
    let prompt = "Enter your API key, it can be retrieved at https://app.onmcu.com/settings: ";
    let mut raw = if io::stdin().is_terminal() {
        let config = rpassword::ConfigBuilder::new()
            .password_feedback_partial_mask('*', 5)
            .build();
        rpassword::prompt_password_with_config(prompt, config)?
    } else {
        // Piped input (scripts) has no terminal to hide; read a line as before.
        print!("{prompt}");
        io::stdout().flush()?;
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        buf
    };

    let key = SecretString::from(raw.trim().to_owned());
    raw.zeroize();
    validate_api_key(key.expose_secret())?;

    // Store it
    entry
        .set_password(key.expose_secret())
        .map_err(AuthError::from)
        .map_err(LoginError::SaveKeyring)?;
    println!("✅  API key saved.");

    Ok(())
}
