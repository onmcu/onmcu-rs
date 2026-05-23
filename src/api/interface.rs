use secrecy::ExposeSecret as _;

use crate::api::generated::types::BoardInfo;
use crate::api::{ApiError, AuthenticatedClient};

pub fn is_board_supported<'a>(
    board_name: &str,
    mut board_list: impl Iterator<Item = &'a BoardInfo>,
) -> bool {
    board_list.any(|board| board.board_mpn == board_name)
}

const PAGE_SIZE: u32 = 100;

/// Fetch all boards from the server, paginating automatically.
pub async fn fetch_all_boards(client: &AuthenticatedClient) -> anyhow::Result<Vec<BoardInfo>> {
    let mut all_boards = Vec::new();
    let mut offset: u32 = 0;

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

        if received == 0 || (all_boards.len() as u32) >= page.total_count {
            break;
        }
        offset += PAGE_SIZE;
    }

    Ok(all_boards)
}
