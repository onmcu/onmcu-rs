use futures::{SinkExt, TryStreamExt as _};
use secrecy::ExposeSecret;
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    time::timeout,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Bytes, Message, protocol::Role},
};
use tracing::{debug, error, info};

use uuid::Uuid;

use crate::api::generated::types::JobStatusView;
use crate::api::interface::{fetch_all_boards, is_board_supported};
use crate::api::{ApiError, AuthenticatedClient, get_authenticated_client};
use crate::error::CliError;
use crate::upload::UploadConfig;
use crate::upload::submit_job;

/// How many times to poll for the final job status after the log stream ends,
/// at one-second intervals. The job status can lag the end-of-logs marker, so
/// allow a brief grace period before reporting the status as unknown.
const FINAL_STATUS_POLL_ATTEMPTS: u32 = 10;

/// Interval between job-status polls while waiting for a pending job to start.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Delay before the first status poll, kept short for quick initial feedback.
const INITIAL_POLL_DELAY: Duration = Duration::from_millis(100);

/// Result of prompting the user to cancel a job after Ctrl+C.
enum CancelOutcome {
    /// User confirmed; the cancellation request was sent successfully.
    Cancelled,
    /// User declined; keep waiting/streaming.
    Resumed,
}

/// Read a line from stdin and report whether it matches one of `affirmatives`.
///
/// Returns `false` if the line cannot be read, treating an unreadable prompt as
/// a non-confirmation rather than panicking.
async fn read_confirmation(affirmatives: &[&str]) -> bool {
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut response = String::new();
    if reader.read_line(&mut response).await.is_ok() {
        let response = response.trim().to_lowercase();
        return affirmatives.contains(&response.as_str());
    }
    false
}

/// Send the cancellation request for `job_id`.
///
/// Returns the [`ApiError`] on failure so callers never report the job as
/// cancelled while it may still be queued or running.
async fn cancel_job(client: &AuthenticatedClient, job_id: Uuid) -> Result<(), ApiError> {
    client
        .api()
        .cancel_job()
        .id(job_id)
        .x_api_key(client.api_key.expose_secret())
        .send()
        .await
        .map_err(ApiError::from)?;
    Ok(())
}

/// Prompt the user after Ctrl+C and cancel the job if they confirm.
async fn confirm_and_cancel(
    client: &AuthenticatedClient,
    job_id: Uuid,
) -> Result<CancelOutcome, CliError> {
    eprintln!();
    eprintln!("Received Ctrl+C. Do you want to cancel the job? [y/n]");
    if read_confirmation(&["y", "yes"]).await {
        info!("Cancelling job...");
        cancel_job(client, job_id).await?;
        Ok(CancelOutcome::Cancelled)
    } else {
        Ok(CancelOutcome::Resumed)
    }
}

/// Wait `delay`, then fetch the current job status.
async fn poll_job_status(
    client: &AuthenticatedClient,
    job_id: Uuid,
    delay: Duration,
) -> Result<JobStatusView, CliError> {
    tokio::time::sleep(delay).await;
    let job = client
        .api()
        .get_job()
        .id(job_id)
        .x_api_key(client.api_key.expose_secret())
        .send()
        .await
        .map_err(ApiError::from)
        .map_err(CliError::JobStatus)?;
    Ok(job.into_inner().status)
}

/// Handle the `run` command: check board and delegate to upload
pub async fn handle_run(
    cfg: UploadConfig,
    board: String,
    file_path: PathBuf,
    api_key_from_env: bool,
    wait_timeout: u64,
    logging_config: crate::api::generated::types::LoggingConfig,
) -> Result<(), CliError> {
    let client = get_authenticated_client(&cfg.server, api_key_from_env).await?;

    info!("Getting list of boards...");
    let board_list = fetch_all_boards(&client).await?;

    debug!("Got list of boards {:?}", board_list);

    // Check if board is supported
    if !is_board_supported(&board, board_list.iter()) {
        info!("Available boards:");
        board_list
            .iter()
            .for_each(|board| info!("  {}", board.board_mpn));

        return Err(CliError::BoardNotFound(board));
    }

    info!("Running upload for board: {}", board);

    let job_id = submit_job(file_path, board, &cfg, &client, logging_config).await?;

    info!("Submitted file for Job ID {job_id}");

    // Wait for the job to start running before streaming logs.
    // The server rejects WebSocket upgrades for non-running jobs (409 Conflict).
    eprint!("⏳ Waiting for job to start...");
    let max_wait = Duration::from_secs(wait_timeout);
    'wait: loop {
        let deadline = tokio::time::Instant::now() + max_wait;
        let mut poll_delay = INITIAL_POLL_DELAY;
        loop {
            // Race Ctrl+C, the wait deadline, and the next status poll so the
            // whole pending-wait state stays responsive — including while the
            // poll request is in flight.
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    match confirm_and_cancel(&client, job_id).await? {
                        CancelOutcome::Cancelled => return Err(CliError::JobCancelled),
                        CancelOutcome::Resumed => {
                            eprint!("⏳ Waiting for job to start...");
                            continue;
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    eprintln!();
                    eprintln!(
                        "No device available after {}s. Wait another {}s or cancel? [w/c]: ",
                        max_wait.as_secs(),
                        max_wait.as_secs()
                    );

                    if read_confirmation(&["w", "wait"]).await {
                        eprint!("⏳ Waiting for job to start...");
                        continue 'wait;
                    }

                    // Cancel the pending job so it doesn't sit in the queue
                    info!("Cancelling pending job...");
                    cancel_job(&client, job_id).await?;
                    return Err(CliError::NoDeviceAvailable);
                }
                status = poll_job_status(&client, job_id, poll_delay) => {
                    poll_delay = POLL_INTERVAL;
                    match status? {
                        JobStatusView::Running => {
                            eprintln!(" started!");
                            break 'wait;
                        }
                        JobStatusView::Completed => {
                            eprintln!(" completed before log streaming could start");
                            return Ok(());
                        }
                        JobStatusView::Failed => return Err(CliError::JobFailed),
                        JobStatusView::Cancelled => return Err(CliError::JobCancelled),
                        JobStatusView::Timeout => return Err(CliError::JobTimedOut),
                        // Still pending/dispatched — keep waiting
                        _ => eprint!("."),
                    }
                }
            }
        }
    }

    // Creates a GET request, upgrades and sends it.
    let response = client
        .api()
        .stream_job_logs()
        .id(job_id)
        .x_api_key(client.api_key.expose_secret())
        .send()
        .await
        .map_err(ApiError::from)
        .map_err(CliError::LogStream)?;

    // Turns the response into a WebSocket stream.
    let mut websocket =
        WebSocketStream::from_raw_socket(response.into_inner(), Role::Client, None).await;

    // The WebSocket is also a `TryStream` over `Message`s.
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                match confirm_and_cancel(&client, job_id).await? {
                    // Let the final-status poll below confirm the outcome.
                    CancelOutcome::Cancelled => break,
                    CancelOutcome::Resumed => {
                        info!("Job cancellation aborted, continuing to stream logs...");
                        continue;
                    }
                }
            }
            // Wait for the next message or timeout
            res = timeout(Duration::from_secs(30), websocket.try_next()) => {
                match res {
                    Ok(message_result) => {
                        // A stream error is not necessarily a job failure, so
                        // stop reading logs and let the final-status poll decide
                        // the outcome rather than failing outright here.
                        let message = match message_result {
                            Ok(message) => message,
                            Err(e) => {
                                error!(%e, "Log stream error; checking final job status");
                                break;
                            }
                        };
                        match message {
                            Some(Message::Text(text)) => {
                                println!("received: {text}");
                            }
                            Some(Message::Ping(_)) => {
                                debug!("Received Ping");
                            }
                            Some(Message::Pong(_)) => {
                                debug!("Received Pong");
                            }
                            Some(Message::Close(frame)) => {
                                if let Some(frame) = frame {
                                    eprintln!("Connection closed: {}", frame.reason);
                                }
                                break;
                            }
                            Some(_) => {}   // Handle other message types if needed
                            None => break,  // WebSocket stream ended
                        }
                    }
                    Err(_) => {
                        debug!("No message received for 30 seconds, sending ping...");
                        let res = websocket.send(Message::Ping(Bytes::from_static(b""))).await;
                        if let Err(e) = res {
                            error!(%e, "Failed to send ping, websocket likely closed unexpectedly");
                            break;
                        }
                    }
                }
            }
        }
    }

    // Fetch final job status so the user knows the outcome and the process exit
    // code reflects it: success only on `Completed`, error otherwise.
    // The DB update may lag behind the EndOfLogs sentinel, so poll briefly.
    eprint!("Waiting for final job status...");
    for _ in 0..FINAL_STATUS_POLL_ATTEMPTS {
        if let Ok(job) = client
            .api()
            .get_job()
            .id(job_id)
            .x_api_key(client.api_key.expose_secret())
            .send()
            .await
        {
            let status = job.into_inner().status;
            if status != JobStatusView::Running {
                eprintln!(" Status: {status}");
                return match status {
                    JobStatusView::Completed => Ok(()),
                    JobStatusView::Failed => Err(CliError::JobFailed),
                    JobStatusView::Cancelled => Err(CliError::JobCancelled),
                    JobStatusView::Timeout => Err(CliError::JobTimedOut),
                    _ => Err(CliError::StatusUnknown),
                };
            } else {
                eprint!(".");
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    eprintln!("Job status: unknown (timed out waiting for final status)");

    Err(CliError::StatusUnknown)
}
