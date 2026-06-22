use std::process::ExitCode;

use tracing_subscriber::FmtSubscriber;

use onmcu::*;

#[tokio::main]
async fn main() -> ExitCode {
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
    if let Err(e) = tracing::subscriber::set_global_default(sub) {
        eprintln!("Error: Could not set up logging: {e}");
        return ExitCode::FAILURE;
    }

    // Install the OS keyring as the default store. Non-fatal: commands that need
    // it report a clear error later; ONMCU_API_KEY works without it.
    keyring::init_default_store();

    let result = cli.dispatch().await;

    keyring::shutdown();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            e.exit_code()
        }
    }
}
