//! Errors shown by the `onmcu` command.
//!
//! Each command returns a [`CliError`]. The application prints the error and
//! uses [`CliError::exit_code`] to choose an exit code for that kind of error.

use std::process::ExitCode;

use thiserror::Error;

use crate::api::{ApiError, AuthError};
use crate::commands::login::LoginError;
use crate::update_check::UpdateError;
use crate::upload::{ConfigError, UploadError};

#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Auth(#[from] AuthError),

    #[error(transparent)]
    Api(#[from] ApiError),

    #[error(transparent)]
    Upload(#[from] UploadError),

    #[error(transparent)]
    Login(#[from] LoginError),

    #[error(transparent)]
    Update(#[from] UpdateError),

    #[error(
        "No matching board found for {0}\nGet supported boards using the `list-boards` command"
    )]
    BoardNotFound(String),

    #[error("Cancelled — no device available")]
    NoDeviceAvailable,

    #[error("Failed to get job status: {0}")]
    JobStatus(#[source] ApiError),

    #[error("Failed to connect to log stream: {0}")]
    LogStream(#[source] ApiError),

    #[error("Job failed")]
    JobFailed,

    #[error("Job was cancelled")]
    JobCancelled,

    #[error("Job timed out")]
    JobTimedOut,

    #[error("Timed out waiting for final job status")]
    StatusUnknown,

    #[error("Interrupted")]
    Interrupted,

    #[error(
        "Unexpected arguments: {}\nIf a development tool added them, pass `--ignore-trailing-args` before the extra arguments to ignore them.",
        .0.join(" ")
    )]
    UnexpectedArgs(Vec<String>),
}

/// Exit code for a missing or rejected authentication credential.
const EXIT_AUTH: u8 = 4;
/// Exit code for a failed API request.
const EXIT_API: u8 = 5;
/// Exit code for a request denied to an authenticated account.
const EXIT_FORBIDDEN: u8 = 14;

impl CliError {
    /// Return the exit code for this kind of error.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        let code: u8 = match self {
            Self::Config(_) => 3,
            Self::Api(e)
            | Self::Upload(UploadError::Api(e))
            | Self::JobStatus(e)
            | Self::LogStream(e) => api_exit_code(e),
            Self::Upload(_) => 6,
            Self::Auth(_) | Self::Login(LoginError::Keyring(_) | LoginError::SaveKeyring(_)) => {
                EXIT_AUTH
            }
            Self::Login(_) => 7,
            Self::BoardNotFound(_) => 8,
            Self::NoDeviceAvailable => 9,
            Self::JobFailed => 10,
            Self::JobCancelled => 11,
            Self::JobTimedOut => 12,
            Self::StatusUnknown => 13,
            Self::Update(_) => 15,
            Self::UnexpectedArgs(_) => 2,
            // 128 + SIGINT, the conventional exit code for Ctrl+C.
            Self::Interrupted => 130,
        };
        ExitCode::from(code)
    }
}

/// Return the exit code assigned to an [`ApiError`] variant.
const fn api_exit_code(err: &ApiError) -> u8 {
    match err {
        ApiError::InvalidApiKey => EXIT_AUTH,
        ApiError::AccessDenied => EXIT_FORBIDDEN,
        _ => EXIT_API,
    }
}
