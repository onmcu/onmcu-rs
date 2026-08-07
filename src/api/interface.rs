use secrecy::ExposeSecret as _;

use crate::api::generated::types::BoardInfo;
use crate::api::{ApiError, AuthenticatedClient};

/// Find a board by MPN, ignoring case so users don't have to match the
/// server's spelling exactly. Returns the server's canonical entry.
pub fn find_board<'a>(
    board_name: &str,
    mut board_list: impl Iterator<Item = &'a BoardInfo>,
) -> Option<&'a BoardInfo> {
    board_list.find(|board| board.board_mpn.eq_ignore_ascii_case(board_name))
}

const PAGE_SIZE: u32 = 100;

/// Fetch all boards from the server, paginating automatically.
///
/// # Errors
/// Returns an [`ApiError`] when the server could not be reached or returned unexpected status.
pub async fn fetch_all_boards(client: &AuthenticatedClient) -> Result<Vec<BoardInfo>, ApiError> {
    let mut all_boards = Vec::new();
    let mut offset: usize = 0;

    loop {
        let response = client
            .api()
            .list_boards()
            .limit(PAGE_SIZE)
            .offset(offset)
            .x_api_key(client.api_key.expose_secret())
            .send()
            .await
            .map_err(ApiError::from)?;
        let page = response.into_inner();
        let received = page.items.len();
        all_boards.extend(page.items);

        if received == 0 || all_boards.len() >= page.total_count as usize {
            break;
        }
        // Advance by what the server actually sent; it may return fewer
        // items than requested even when more pages remain.
        offset += received;
    }

    Ok(all_boards)
}
