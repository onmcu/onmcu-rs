use thiserror::Error;

use crate::api::ApiError;

#[derive(Error, Debug)]
pub enum UploadError {
    #[error("File system error: {0}")]
    FileSystem(#[from] std::io::Error),

    #[error(transparent)]
    Api(#[from] ApiError),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Upload failed after {attempts} attempts: {last_error}")]
    UploadRetryExhausted { attempts: u32, last_error: String },

    #[error("File exceeds maximum allowed size of {max_size} bytes")]
    FileTooLarge { max_size: u64 },

    #[error("File is empty, nothing to upload")]
    EmptyFile,

    #[error("Chunk size {chunk_size} bytes outside allowed range (1-{max_size} bytes)")]
    IllegalChunkSize { chunk_size: usize, max_size: u64 },

    #[error("Illegal job timeout: {0} seconds (allowed range 59-86400)")]
    IllegalTimeoutSeconds(u32),
}
