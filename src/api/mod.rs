pub mod error;
pub mod generated;
pub mod interface;
pub mod types;

pub use error::{ApiError, check_connectivity, check_controller_version, verify_access};
pub use types::AuthError;
pub use types::AuthenticatedClient;
pub use types::{ApiKeyFormatError, validate_api_key};

use crate::error::CliError;

/// Build the authenticated client and verify, against the controller, that the
/// server is reachable, runs a supported version and accepts the key.
pub async fn get_authenticated_client(
    server_url: &url::Url,
    api_key_from_env: bool,
) -> Result<AuthenticatedClient, CliError> {
    let client = AuthenticatedClient::new_with_api_key(server_url, api_key_from_env)?;
    check_connectivity(&client).await?;
    check_controller_version(&client).await?;
    verify_access(&client).await?;
    Ok(client)
}
