//! Classification of API call failures into user-facing errors.
//!
//! The progenitor client returns errors that stringify to the raw HTTP status,
//! headers, and JSON body. [`verify_access`] checks the key and connectivity
//! once at startup; `From<ClientError>` then maps any later call failure onto a
//! clean [`ApiError`] (use `.map_err(ApiError::from)?` at call sites). Because
//! the key is already known good, a post-startup 401/403 means the operation is
//! forbidden, not that the key is bad.

use secrecy::ExposeSecret as _;
use thiserror::Error;

use crate::api::AuthenticatedClient;
use crate::api::generated::{self, types};

/// A progenitor client error carrying the server's structured error body.
pub type ClientError = generated::Error<types::Error>;

#[derive(Error, Debug)]
pub enum ApiError {
    /// 401/403 after the key was verified at startup: this operation is forbidden.
    #[error(
        "Access denied. Your API key is valid but is not allowed to perform \
         this operation (check your plan or whether your account has access to \
         this board)."
    )]
    AccessDenied,

    /// The key was rejected outright (used by the startup check).
    #[error(
        "Your API key is invalid or expired.\n\
         Get a new key at https://app.onmcu.com/settings, then run \
         `onmcu login --relogin` (or set the ONMCU_API_KEY environment variable)."
    )]
    InvalidApiKey,

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Server error ({status}): {message} (request id: {request_id})")]
    Server {
        status: u16,
        message: String,
        request_id: String,
    },

    #[error("Could not reach the OnMCU server: {0}")]
    Transport(String),

    #[error("Unexpected API error: {0}")]
    Other(String),
}

/// Verify, once at startup, that the API key is accepted and the controller is
/// reachable. Returns a friendly error on failure so the caller can run teardown
/// before exiting, rather than letting a command fail confusingly mid-operation.
pub async fn verify_access(
    client: &AuthenticatedClient,
    server_url: &url::Url,
) -> anyhow::Result<()> {
    let result = client
        .api()
        .get_user()
        .x_api_key(client.api_key.expose_secret())
        .send()
        .await;

    let Err(err) = result else { return Ok(()) };

    Err(match err.status().map(|s| s.as_u16()) {
        Some(401) | Some(403) => ApiError::InvalidApiKey.into(),
        None => anyhow::anyhow!(
            "Could not reach the OnMCU server at {server_url}.\n\
             Check your internet connection and the server URL. ({err})"
        ),
        Some(status) => anyhow::anyhow!(
            "The OnMCU server returned an unexpected error ({status}) while \
             verifying your API key. Please try again later."
        ),
    })
}

/// Pull the server's `message` and `request_id` out of a documented error body,
/// falling back to the error's own `Display` for undocumented responses.
fn extract_body(err: ClientError) -> (String, String) {
    match err {
        generated::Error::ErrorResponse(rv) => {
            let body = rv.into_inner();
            (body.message, body.request_id)
        }
        other => (other.to_string(), String::new()),
    }
}

/// Map a client error to an [`ApiError`] by HTTP status. Use at call sites via
/// `.map_err(ApiError::from)?`.
impl From<ClientError> for ApiError {
    fn from(err: ClientError) -> Self {
        match err.status().map(|s| s.as_u16()) {
            Some(401) | Some(403) => ApiError::AccessDenied,
            Some(404) => ApiError::NotFound(extract_body(err).0),
            Some(status) => {
                let (message, request_id) = extract_body(err);
                ApiError::Server {
                    status,
                    message,
                    request_id,
                }
            }
            None => match err {
                generated::Error::CommunicationError(e)
                | generated::Error::InvalidUpgrade(e)
                | generated::Error::ResponseBodyError(e) => ApiError::Transport(e.to_string()),
                other => ApiError::Other(other.to_string()),
            },
        }
    }
}
