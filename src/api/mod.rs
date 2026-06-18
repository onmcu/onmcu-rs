pub mod error;
pub mod generated;
pub mod interface;
pub mod types;

pub use error::{ApiError, verify_access};
pub use types::AuthError;
pub use types::AuthenticatedClient;

use crate::error::CliError;

/// Build the authenticated client and verify, against the controller, that the
/// key works and the server is reachable.
pub async fn get_authenticated_client(
    server_url: &url::Url,
    api_key_from_env: bool,
) -> Result<AuthenticatedClient, CliError> {
    let client = AuthenticatedClient::new_with_api_key(server_url, api_key_from_env)?;
    verify_access(&client, server_url).await?;
    Ok(client)
}
