use indicatif::{ProgressBar, ProgressStyle};
use secrecy::ExposeSecret as _;
use sha3::{Digest, Sha3_256};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::PathBuf,
};
use tokio::time::{Duration, sleep};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    api::generated::types::JobSubmit,
    upload::{UploadConfig, UploadError},
};

use crate::api::AuthenticatedClient;

/// Convert MiB to bytes
const fn mib_to_bytes(mib: usize) -> usize {
    mib * 1_048_576
}

/// Upload limits from the server
#[derive(Debug, Clone)]
pub struct UploadLimits {
    pub max_chunk_size: u64,
    pub max_file_size: u64,
}

/// Get upload limits from the server
pub async fn get_upload_limits(client: &AuthenticatedClient) -> Result<UploadLimits, UploadError> {
    let api = client.api();

    let response = api
        .get_upload_limits()
        .send()
        .await
        .map_err(|e| UploadError::Api(e.to_string()))?
        .into_inner();

    Ok(UploadLimits {
        max_chunk_size: response.max_chunk_size,
        max_file_size: response.max_file_size,
    })
}

/// Check file size
fn check_file_size(file: &File, max_size: u64) -> Result<(), UploadError> {
    let file_size = file.metadata()?.len();
    if file_size > max_size {
        return Err(UploadError::FileTooLarge { max_size });
    }
    Ok(())
}

/// Calculate SHA3 hash of a file and return as byte vector
pub fn calculate_sha3_bytes(file: &mut File) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;

    let mut hasher = Sha3_256::new();
    let mut reader = BufReader::new(file);
    let mut buffer = [0u8; 8192]; // 8KB chunks

    // Read in chunks to avoid loading large files into memory all at once
    while let Ok(bytes_read) = reader.read(&mut buffer) {
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash = hasher.finalize();
    let hash_bytes = hash.to_vec();
    info!("SHA3-256 hash: {:x?}", hash);
    Ok(hash_bytes)
}

/// File preparation result containing metadata and handles
struct PreparedFile {
    file: File,
    file_size: usize,
    file_hash: Vec<u8>,
    chunk_size: usize,
    total_chunks: u32,
}

/// Prepare file for upload: validate size, calculate hash, determine chunking
async fn prepare_file(
    file_path: &PathBuf,
    cfg: &UploadConfig,
    client: &AuthenticatedClient,
) -> Result<PreparedFile, UploadError> {
    let file_meta = std::fs::metadata(file_path)?;
    let file_size = file_meta.len() as usize;

    // Get upload limits and validate file size
    let limits = get_upload_limits(client).await?;
    let mut file = File::open(file_path)?;
    check_file_size(&file, limits.max_file_size)?;

    // Calculate optimal chunk size
    let chunk_size = std::cmp::min(limits.max_chunk_size as usize, mib_to_bytes(cfg.chunk_size));

    // Validate chunk size is within allowed range (1-10485760 bytes)
    if chunk_size < 1 || chunk_size > mib_to_bytes(10) {
        return Err(UploadError::IllegalChunkSize {
            chunk_size: cfg.chunk_size,
            max_size: limits.max_chunk_size,
        });
    }

    let total_chunks = file_size.div_ceil(chunk_size) as u32;

    // Calculate file hash for submission
    let file_hash = calculate_sha3_bytes(&mut file)?;

    Ok(PreparedFile {
        file,
        file_size,
        file_hash,
        chunk_size,
        total_chunks,
    })
}

/// Initialize job submission with the server
async fn initialize_job(
    board: String,
    file_hash: Vec<u8>,
    timeout_seconds: u32,
    client: &AuthenticatedClient,
) -> Result<Uuid, UploadError> {
    // Validate timeout is within allowed range (59-86400 seconds)
    // This is already checked by clap, but we might offload this to a library,
    // so we validate it here again to remember it.
    if !(59..=86400).contains(&timeout_seconds) {
        return Err(UploadError::IllegalTimeoutSeconds(timeout_seconds));
    }

    let api = client.api();

    let file_hash: [u8; 32] = file_hash
        .try_into()
        .map_err(|_| UploadError::Api("file_hash must be exactly 32 bytes".into()))?;

    // Hardcoded for now: the CLI only knows how to submit regular run
    // jobs. When a `cli debug` subcommand (or equivalent) lands, this
    // should become `JobType::Debug` on that path.
    let job_type = crate::api::generated::types::JobType::Run;

    // Hardcoded for now: RTT is the only log transport the edge
    // server actually implements. When Serial/UART or multi-channel
    // RTT support lands, expose a `--logging {rtt,serial}` flag (or
    // similar) and map it to the matching `LoggingConfig` variant
    // here.
    let logging_config = crate::api::generated::types::LoggingConfig::Rtt(
        crate::api::generated::types::RttConfig {},
    );

    let job = JobSubmit {
        account_id: None, // Optional, defaults to caller's personal account if omitted.
        board_name: board
            .try_into()
            .map_err(|_| UploadError::Api("board_name must be 1-255 characters".into()))?,
        timeout_seconds,
        file_hash,
        job_type,
        logging_config,
    };

    let job_id = api
        .submit_job()
        .body(job)
        .x_api_key(client.api_key.expose_secret())
        .send()
        .await
        .map_err(|e| UploadError::Api(e.to_string()))?
        .into_inner() // UploadSession
        .id;

    info!("New job ID: {job_id}");
    Ok(job_id)
}

/// Upload all file chunks with progress tracking and retry logic
async fn upload_chunks(
    prepared_file: &mut PreparedFile,
    job_id: Uuid,
    cfg: &UploadConfig,
    client: &AuthenticatedClient,
) -> Result<(), UploadError> {
    // Setup progress bar
    let pb = ProgressBar::new(prepared_file.total_chunks as u64);
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} chunks")
            .unwrap()
            .progress_chars("##-"),
    );

    // Reset file position and prepare buffer
    prepared_file.file.seek(SeekFrom::Start(0))?;
    let mut buffer = vec![0u8; prepared_file.chunk_size];

    for chunk_idx in 0..prepared_file.total_chunks {
        let offset = (chunk_idx as usize) * prepared_file.chunk_size;
        let to_read = std::cmp::min(prepared_file.chunk_size, prepared_file.file_size - offset);

        prepared_file.file.seek(SeekFrom::Start(offset as u64))?;
        prepared_file.file.read_exact(&mut buffer[..to_read])?;
        let chunk_data = buffer[..to_read].to_vec();

        // Retry loop with exponential backoff
        upload_chunk_with_retry(
            client,
            job_id,
            chunk_idx,
            prepared_file.total_chunks,
            chunk_data,
            cfg,
        )
        .await?;

        pb.inc(1);
    }

    pb.finish_with_message("Chunks uploaded");
    Ok(())
}

/// Upload a single chunk with retry logic
async fn upload_chunk_with_retry(
    client: &AuthenticatedClient,
    job_id: Uuid,
    chunk_idx: u32,
    total_chunks: u32,
    chunk_data: Vec<u8>,
    cfg: &UploadConfig,
) -> Result<(), UploadError> {
    let mut attempts = 0;

    loop {
        attempts += 1;
        match try_upload_job_chunk(client, job_id, chunk_idx, total_chunks, chunk_data.clone())
            .await
        {
            Ok(_) => break,
            Err(e) if attempts <= cfg.retries.into() => {
                warn!(
                    "Chunk {chunk_idx} failed (attempt {attempts}/{}) – {e}. Retrying...",
                    cfg.retries
                );
                // Exponential backoff: 1s, 2s, 4s, ...
                sleep(Duration::from_secs(1 << (attempts - 1))).await;
            }
            Err(e) => {
                return Err(UploadError::UploadRetryExhausted {
                    attempts,
                    last_error: e.to_string(),
                });
            }
        }
    }

    Ok(())
}

/// Finalize the upload process
async fn finalize_upload(job_id: Uuid, client: &AuthenticatedClient) -> Result<(), UploadError> {
    let api = client.api();

    api.finalize_job_upload()
        .x_api_key(client.api_key.expose_secret())
        .id(job_id)
        .send()
        .await
        .map_err(|e| UploadError::Api(e.to_string()))?
        .into_inner();

    println!("✅ Upload complete!");
    Ok(())
}

/// Submit a new job to the OnMCU server
pub async fn submit_job(
    file_path: PathBuf,
    board: String,
    cfg: &UploadConfig,
    client: &AuthenticatedClient,
) -> Result<Uuid, UploadError> {
    // 1. Prepare file: validate, hash, determine chunking strategy
    let mut prepared_file = prepare_file(&file_path, cfg, client).await?;

    // 2. Initialize job with server
    let job_id = initialize_job(
        board,
        prepared_file.file_hash.clone(),
        cfg.timeout_seconds,
        client,
    )
    .await?;

    // 3. Upload all chunks with progress tracking
    upload_chunks(&mut prepared_file, job_id, cfg, client).await?;

    // 4. Finalize upload
    finalize_upload(job_id, client).await?;

    Ok(job_id)
}

/// Upload one chunk using the _regenerated_ client.
///
/// The OpenAPI spec now models the query parameters (`idx`,
/// `total`), so we can call the generated method directly.
/// Any error bubbles up and will be handled by the caller's retry logic.
async fn try_upload_job_chunk(
    client: &AuthenticatedClient,
    job_id: Uuid,
    chunk_number: u32,
    total_chunks: u32,
    bytes: Vec<u8>,
) -> Result<(), UploadError> {
    // ← single generated-client call
    client
        .api()
        .upload_job_chunk()
        .x_api_key(client.api_key.expose_secret())
        .id(job_id)
        .idx(chunk_number)
        .total(total_chunks)
        .body(bytes)
        .send()
        .await
        .map_err(|e| UploadError::Api(e.to_string()))?;

    Ok(())
}
