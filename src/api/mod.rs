pub mod error;
pub mod generated;
pub mod interface;
pub mod types;

pub use error::{ApiError, verify_access};
pub use types::AuthError;
pub use types::AuthenticatedClient;

/// Build the authenticated client and verify, against the controller, that the
/// key works and the server is reachable. Exits with guidance on any failure.
pub async fn get_authenticated_client(
    server_url: &url::Url,
    api_key_from_env: bool,
) -> anyhow::Result<AuthenticatedClient> {
    let client = match AuthenticatedClient::new_with_api_key(server_url, api_key_from_env) {
        Ok(client) => client,
        Err(AuthError::NoApiKey) => anyhow::bail!(
            "No API key found. Get one at https://app.onmcu.com/settings, then run `onmcu login`."
        ),
        Err(AuthError::NoApiKeyInEnv(_)) => anyhow::bail!(
            "--api-key-from-env was set, but ONMCU_API_KEY is missing from the environment."
        ),
        Err(e) => return Err(e.into()),
    };

    verify_access(&client, server_url).await?;
    Ok(client)
}
