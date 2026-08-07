use std::process::ExitCode;

use tracing_subscriber::FmtSubscriber;

use onmcu::{cli, keyring, update_check};

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

    // Runs alongside the command so the lookup costs no extra wall-clock time.
    let update_check = update_check::spawn(cli.checks_for_updates_itself());

    let result = cli.dispatch().await;

    keyring::shutdown();

    let exit_code = match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            e.exit_code()
        }
    };

    // Reported after any error, so the notice is the last thing on screen and
    // never pushes the actual failure out of view.
    update_check.report().await;

    exit_code
}
