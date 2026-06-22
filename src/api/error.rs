//! Classification of API call failures into user-facing errors.
//!
//! The progenitor client returns errors containing the raw HTTP status, headers,
//! and JSON body. [`verify_access`] classifies responses from the dedicated API
//! key check, while `From<ClientError>` classifies failures of authenticated
//! operations. A 401/403 therefore means a rejected key in the former and a
//! denied operation in the latter.

use secrecy::ExposeSecret as _;
use thiserror::Error;

use crate::api::AuthenticatedClient;
use crate::api::generated::{self, types};

/// A progenitor client error carrying the server's structured error body.
pub type ClientError = generated::Error<types::Error>;

#[derive(Error, Debug)]
pub enum ApiError {
    /// 401/403 for an operation made with an already validated API key.
    #[error(
        "Access denied. Your API key is valid but is not allowed to perform \
         this operation (check your plan or whether your account has access to \
         this board)."
    )]
    AccessDenied,

    /// The API-key validation request was rejected.
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

    #[error(
        "Could not reach the OnMCU server at {server_url}.\n\
         Check your internet connection and the server URL. ({message})"
    )]
    VerificationTransport {
        server_url: url::Url,
        message: String,
    },

    #[error(
        "The OnMCU server returned an unexpected error ({status}) while \
         verifying your API key. Please try again later."
    )]
    VerificationServer { status: u16 },
}

/// Verify that the API key is accepted and the controller is reachable.
pub async fn verify_access(
    client: &AuthenticatedClient,
    server_url: &url::Url,
) -> Result<(), ApiError> {
    let result = client
        .api()
        .get_user()
        .x_api_key(client.api_key.expose_secret())
        .send()
        .await;

    let Err(err) = result else { return Ok(()) };

    Err(match err.status().map(|s| s.as_u16()) {
        Some(401) | Some(403) => ApiError::InvalidApiKey,
        None => ApiError::VerificationTransport {
            server_url: server_url.clone(),
            message: err.to_string(),
        },
        Some(status) => ApiError::VerificationServer { status },
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

/// Map a client error to an [`ApiError`] by HTTP status.
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
