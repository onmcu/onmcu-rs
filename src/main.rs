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

    cli.dispatch().await
}
