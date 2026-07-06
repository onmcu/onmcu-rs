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
    // Size each column to its longest entry so long names stay aligned.
    let mut w_board = "Board MPN".len();
    let mut w_mcu = "MCU MPN".len();
    let mut w_manufacturer = "Manufacturer".len();
    for board in &board_list {
        w_board = w_board.max(board.board_mpn.len());
        w_mcu = w_mcu.max(board.mcu_mpn.len());
        w_manufacturer = w_manufacturer.max(board.manufacturer_name.len());
    }
    // Two spaces between columns.
    let (w_board, w_mcu) = (w_board + 2, w_mcu + 2);
    println!("{:<w_board$}{:<w_mcu$}Manufacturer", "Board MPN", "MCU MPN");
    println!("{:-<width$}", "", width = w_board + w_mcu + w_manufacturer);
    for board in board_list {
        println!(
            "{:<w_board$}{:<w_mcu$}{}",
            board.board_mpn, board.mcu_mpn, board.manufacturer_name
        );
    }
    Ok(())
}
