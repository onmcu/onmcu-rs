#[cfg(target_os = "linux")]
use linux_keyutils_keyring_store::Store;

#[cfg(target_os = "macos")]
use apple_native_keyring_store::keychain::Store;

#[cfg(target_os = "windows")]
use windows_native_keyring_store::Store;

use tracing_subscriber::FmtSubscriber;

use onmcu::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::build();

    // Initialize logging based on verbose flag
    let log_level = if cli.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::ERROR
    };

    let sub = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_file(cli.verbose)
        .with_line_number(cli.verbose)
        .with_target(cli.verbose)
        .finish();
    tracing::subscriber::set_global_default(sub)?;

    // Set keyutils backend as the default store
    keyring_core::set_default_store(Store::new().unwrap());

    let res = cli.dispatch().await;

    keyring_core::unset_default_store();

    res
}
