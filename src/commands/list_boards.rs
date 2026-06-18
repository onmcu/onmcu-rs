use tracing::info;

use crate::{
    api::get_authenticated_client, api::interface::fetch_all_boards, error::CliError,
    upload::UploadConfig,
};

/// Handle the `list-boards` command: get lists of available boards from server and print to stdout
pub async fn handle_list_boards(cfg: UploadConfig, api_key_from_env: bool) -> Result<(), CliError> {
    let client = get_authenticated_client(&cfg.server, api_key_from_env).await?;

    info!("Getting list of boards...");
    let board_list = fetch_all_boards(&client).await?;

    println!("Available Boards:");
    println!("{:<25}{:<25}{:<25}", "Board MPN", "MCU MPN", "Manufacturer");
    println!("{:-<75}", "");
    for board in board_list {
        println!(
            "{:<25}{:<25}{:<25}",
            board.board_mpn, board.mcu_mpn, board.manufacturer_name
        );
    }
    Ok(())
}
