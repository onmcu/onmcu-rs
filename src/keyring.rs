//! OS keyring setup and diagnostics for when the backend is unavailable.
//!
//! Uses the native OS keyring per platform. On Linux this is the freedesktop
//! Secret Service, which needs a D-Bus session and a keyring daemon (GNOME
//! Keyring, KWallet, KeePassXC); when it's missing we surface a clear hint.

use keyring_core::Error as KeyringError;

#[cfg(target_os = "linux")]
use dbus_secret_service_keyring_store::Store;

#[cfg(target_os = "macos")]
use apple_native_keyring_store::keychain::Store;

#[cfg(target_os = "windows")]
use windows_native_keyring_store::Store;

/// Install the OS keyring as the default store. Non-fatal: on failure the
/// store is left unset and callers surface [`unavailable_hint`] when needed.
pub fn init_default_store() {
    match Store::new() {
        Ok(store) => keyring_core::set_default_store(store),
        Err(e) => tracing::debug!("keyring backend unavailable at startup: {e}"),
    }
}

/// Release the default store on shutdown.
pub fn shutdown() {
    keyring_core::unset_default_store();
}

/// Whether a keyring error means no usable backend is reachable (none running
/// or none configured), as opposed to one that exists but is locked.
pub fn is_unavailable(err: &KeyringError) -> bool {
    matches!(
        err,
        KeyringError::NoDefaultStore | KeyringError::PlatformFailure(_)
    )
}

/// Whether a keyring error means the backend is present but access was denied,
/// typically because the keyring is locked.
pub fn is_locked(err: &KeyringError) -> bool {
    matches!(err, KeyringError::NoStorageAccess(_))
}

/// A user-facing explanation for a missing or unreachable keyring backend.
pub fn unavailable_hint() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "The OS keyring is not available.\n\
         onmcu stores your API key in the freedesktop Secret Service, which needs \
         a running D-Bus session and a keyring daemon such as GNOME Keyring, \
         KWallet, or KeePassXC.\n\
         On a headless machine, set the ONMCU_API_KEY environment variable and \
         pass --api-key-from-env instead of running `onmcu login`."
    }
    #[cfg(not(target_os = "linux"))]
    {
        "The OS keyring is not available.\n\
         Set the ONMCU_API_KEY environment variable and pass --api-key-from-env \
         instead of running `onmcu login`."
    }
}

/// A user-facing explanation for a locked keyring.
pub fn locked_hint() -> &'static str {
    "The OS keyring is locked. Unlock it (e.g. log in to your desktop session or \
     unlock your login keyring) and try again.\n\
     Alternatively, set the ONMCU_API_KEY environment variable and pass \
     --api-key-from-env."
}
