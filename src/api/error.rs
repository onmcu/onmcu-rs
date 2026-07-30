//! Classification of API call failures into user-facing errors.
//!
//! The progenitor client returns errors containing the raw HTTP status, headers,
//! and JSON body. [`verify_access`] classifies responses from the dedicated API
//! key check, while `From<ClientError>` classifies failures of authenticated
//! operations. A 401/403 therefore means a rejected key in the former and a
//! denied operation in the latter.
//!
//! Startup runs [`check_connectivity`], [`check_controller_version`] and
//! [`verify_access`] in that order, so each reports on exactly one cause of
//! failure: an unreachable server, an incompatible controller, a bad API key.

use secrecy::ExposeSecret as _;
use semver::{Version, VersionReq};
use thiserror::Error;

use crate::api::AuthenticatedClient;
use crate::api::generated::{self, ClientInfo as _, types};

/// A progenitor client error carrying the server's structured error body.
pub type ClientError = generated::Error<types::Error>;

#[derive(Error, Debug)]
pub enum ApiError {
    /// 403 for an operation made with an already validated API key.
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
        "Server version {server_version} is not supported. Supported version (according to semver): {supported_versions}. You may need to update this application."
    )]
    UnsupportedServerVersion {
        server_version: Version,
        supported_versions: VersionReq,
    },

    #[error(
        "Could not reach the OnMCU server at {server_url}.\n\
         Check your internet connection and the server URL.\n\
         Details: {message}"
    )]
    VerificationTransport { server_url: String, message: String },

    #[error(
        "The OnMCU server returned an unexpected error ({status}) while \
         verifying your API key. Please try again later."
    )]
    VerificationServer { status: u16 },
}

/// Supported versions that are not compatible from a semver point of view
const EXTRA_SUPPORTED_VERSIONS: &[Version] = &[
    //Version::new(0, 2, 0), // Temporarily supported e.g. during parallel rollout.
];

fn is_supported(
    controller_version: &Version,
    version_req: &VersionReq,
    extra: Option<&[Version]>,
) -> bool {
    version_req.matches(&controller_version)
        || extra.is_some_and(|extra| extra.iter().any(|version| version == controller_version))
}

/// Verify that the controller version is supported by this version of the CLI.
///
/// Supported means semver-compatible with the OpenAPI spec this client was
/// generated from, the same rule Cargo applies to crate dependencies.
pub async fn check_controller_version(client: &AuthenticatedClient) -> Result<(), ApiError> {
    // Retrieve controller version from version endpoint
    let controller_version_res =
        client
            .api()
            .get_version()
            .send()
            .await
            .map_err(|e| ApiError::VerificationTransport {
                server_url: client.api_client.baseurl.clone(),
                message: format!("Could not reach controller version endpoint. {e}"),
            })?;

    // Parse returned version
    let controller_version = Version::parse(&controller_version_res).map_err(|e| {
        ApiError::Other(format!(
            "Could not parse controller version response as a semver version: '{}'. {e}",
            controller_version_res.as_str()
        ))
    })?;

    // Obtain version that was used to generate this progenitor client
    let client_version = generated::Client::api_version();

    // Turn the client_version into a semver version requirement
    let version_req = VersionReq::parse(&format!("^{client_version}")).unwrap();

    if is_supported(
        &controller_version,
        &version_req,
        Some(EXTRA_SUPPORTED_VERSIONS),
    ) {
        Ok(())
    } else {
        Err(ApiError::UnsupportedServerVersion {
            server_version: controller_version,
            supported_versions: version_req,
        })
    }
}

/// Verify that the API key is accepted by the controller.
pub async fn verify_access(client: &AuthenticatedClient) -> Result<(), ApiError> {
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
            server_url: client.api_client.baseurl.clone(),
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
            Some(401) => ApiError::InvalidApiKey,
            Some(403) => ApiError::AccessDenied,
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

#[cfg(test)]
mod test {
    use semver::{Version, VersionReq};

    use crate::api::error::is_supported;

    #[test]
    fn check_supported_versions() {
        assert!(
            is_supported(
                &Version::new(0, 1, 2),
                &VersionReq::parse("^0.1.0").unwrap(),
                None
            ),
            "should support patch bump"
        );

        assert!(
            is_supported(
                &Version::new(1, 1, 2),
                &VersionReq::parse("^1.0.0").unwrap(),
                None
            ),
            "should support minor and patch bump with major >0"
        );

        assert!(
            !is_supported(
                &Version::new(0, 2, 0),
                &VersionReq::parse("^0.1.0").unwrap(),
                None
            ),
            "should not support minor bump"
        );

        assert!(
            is_supported(
                &Version::new(0, 2, 0),
                &VersionReq::parse("^0.1.0").unwrap(),
                Some(&[Version::new(0, 2, 0)])
            ),
            "should support extra version"
        );
    }
}
