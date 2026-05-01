pub mod generated;
pub mod interface;
pub mod types;

pub use types::AuthError;
pub use types::AuthenticatedClient;

pub fn get_authenticated_client(
    server_url: &url::Url,
    api_key_from_env: bool,
) -> anyhow::Result<AuthenticatedClient> {
    // Create authenticated client once
    match AuthenticatedClient::new_with_api_key(server_url, api_key_from_env) {
        Ok(client) => Ok(client),
        Err(AuthError::NoApiKey) => {
            tracing::error!("No API key found. Please run `onmcu login` to set one.");
            std::process::exit(1);
        }
        Err(AuthError::NoApiKeyInEnv(_)) => {
            if api_key_from_env {
                tracing::error!(
                    "Flag --api-key-from-env set, but env var ONMCU_API_KEY is missing"
                );
            }
            std::process::exit(1);
        }
        Err(e) => Err(e.into()),
    }
}
