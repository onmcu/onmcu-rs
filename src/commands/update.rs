use std::cmp::Ordering;

use crate::update_check::{self, UpdateError};

/// Check for a newer release and report the result.
///
/// Blocks on the lookup and reports a failed one, unlike the check that runs in
/// the background of every other command.
pub async fn handle_update() -> Result<(), UpdateError> {
    let latest = update_check::check_now().await?;
    let current = update_check::current_version();

    match latest.cmp(&current) {
        Ordering::Greater => print!("{}", update_check::notice(&latest)),
        // A build from source or a prerelease, ahead of anything published.
        Ordering::Less => println!("onmcu {current} is ahead of the latest release ({latest})."),
        Ordering::Equal => println!("onmcu {current} is up to date."),
    }

    Ok(())
}
