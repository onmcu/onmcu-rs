use futures::{SinkExt, TryStreamExt as _};
use secrecy::ExposeSecret;
use std::{io::IsTerminal as _, path::PathBuf, time::Duration};
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
use crate::api::interface::{fetch_all_boards, find_board};
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

/// Idle time on the log stream before sending a keep-alive ping.
const PING_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

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
    // Without a terminal there is nobody to answer the prompt. Cancel right
    // away so SIGINT still stops the CLI in scripts and CI, where the
    // installed signal handler would otherwise make the process unkillable.
    if !std::io::stdin().is_terminal() {
        eprintln!("Received SIGINT. Cancelling job...");
        cancel_job(client, job_id).await?;
        return Ok(CancelOutcome::Cancelled);
    }
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

/// Map a terminal failure status to its CLI error; `None` for anything else.
const fn job_failure(status: JobStatusView) -> Option<CliError> {
    match status {
        JobStatusView::Failed => Some(CliError::JobFailed),
        JobStatusView::Cancelled => Some(CliError::JobCancelled),
        JobStatusView::Timeout => Some(CliError::JobTimedOut),
        _ => None,
    }
}

/// Resolve the requested board name to its MPN.
///
/// On failure, list the available boards so the user can correct the name.
async fn resolve_board(
    client: &AuthenticatedClient,
    requested_board: String,
) -> Result<String, CliError> {
    info!("Getting list of boards...");
    let board_list = fetch_all_boards(client).await?;
    debug!("Got list of boards {:?}", board_list);

    if let Some(board) = find_board(&requested_board, board_list.iter()) {
        return Ok(board.board_mpn.clone());
    }

    eprintln!("Available boards:");
    for board in board_list {
        eprintln!("  {}", board.board_mpn);
    }
    Err(CliError::BoardNotFound(requested_board))
}

/// Result of waiting for a submitted job to leave the queue.
enum JobStartOutcome {
    /// The job is running; logs can be streamed.
    Running,
    /// The job finished before log streaming could start.
    FinishedEarly,
}

/// Wait for the job to start running before streaming logs.
///
/// The server rejects WebSocket upgrades for non-running jobs (409 Conflict).
/// On timeout, prompt to keep waiting or cancel the pending job.
async fn wait_for_job_start(
    client: &AuthenticatedClient,
    job_id: Uuid,
    max_wait: Duration,
) -> Result<JobStartOutcome, CliError> {
    eprint!("⏳ Waiting for job to start...");
    'wait: loop {
        let deadline = tokio::time::Instant::now() + max_wait;
        let mut poll_delay = INITIAL_POLL_DELAY;
        loop {
            // Race Ctrl+C, the wait deadline, and the next status poll so the
            // whole pending-wait state stays responsive — including while the
            // poll request is in flight.
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    match confirm_and_cancel(client, job_id).await? {
                        CancelOutcome::Cancelled => return Err(CliError::JobCancelled),
                        CancelOutcome::Resumed => {
                            eprint!("⏳ Waiting for job to start...");
                        }
                    }
                }
                () = tokio::time::sleep_until(deadline) => {
                    eprintln!();
                    // Only prompt when someone can answer; scripts and CI
                    // fall through to cancelling the pending job.
                    if std::io::stdin().is_terminal() {
                        eprintln!(
                            "No device available after {}s. Wait another {}s or cancel? [w/c]: ",
                            max_wait.as_secs(),
                            max_wait.as_secs()
                        );

                        if read_confirmation(&["w", "wait"]).await {
                            eprint!("⏳ Waiting for job to start...");
                            continue 'wait;
                        }
                    }

                    // Cancel the pending job so it doesn't sit in the queue
                    info!("Cancelling pending job...");
                    cancel_job(client, job_id).await?;
                    return Err(CliError::NoDeviceAvailable);
                }
                status = poll_job_status(client, job_id, poll_delay) => {
                    poll_delay = POLL_INTERVAL;
                    match status? {
                        JobStatusView::Running => {
                            eprintln!(" started!");
                            return Ok(JobStartOutcome::Running);
                        }
                        JobStatusView::Completed => {
                            eprintln!(" completed before log streaming could start");
                            return Ok(JobStartOutcome::FinishedEarly);
                        }
                        status => match job_failure(status) {
                            Some(e) => return Err(e),
                            // Still pending/dispatched: keep waiting
                            None => eprint!("."),
                        },
                    }
                }
            }
        }
    }
}

/// Stream job logs over the WebSocket until the stream ends or the user
/// cancels. Stream errors are not fatal; the final status poll decides the
/// outcome.
async fn stream_logs(client: &AuthenticatedClient, job_id: Uuid) -> Result<(), CliError> {
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
                match confirm_and_cancel(client, job_id).await? {
                    // Let the final-status poll confirm the outcome.
                    CancelOutcome::Cancelled => break,
                    CancelOutcome::Resumed => {
                        info!("Job cancellation aborted, continuing to stream logs...");
                    }
                }
            }
            // Wait for the next message or the keep-alive deadline
            res = timeout(PING_IDLE_TIMEOUT, websocket.try_next()) => {
                match res {
                    Ok(Ok(Some(Message::Text(text)))) => println!("{text}"),
                    Ok(Ok(Some(Message::Ping(_)))) => debug!("Received Ping"),
                    Ok(Ok(Some(Message::Pong(_)))) => debug!("Received Pong"),
                    Ok(Ok(Some(Message::Close(frame)))) => {
                        if let Some(frame) = frame {
                            eprintln!("Connection closed: {}", frame.reason);
                        }
                        break;
                    }
                    Ok(Ok(Some(other))) => debug!("Ignoring message: {other:?}"),
                    // WebSocket stream ended
                    Ok(Ok(None)) => break,
                    // A stream error is not necessarily a job failure, so
                    // stop reading logs and let the final status poll decide
                    // the outcome rather than failing outright here.
                    Ok(Err(e)) => {
                        error!(%e, "Log stream error; checking final job status");
                        break;
                    }
                    // No message for a while: check the connection with a ping.
                    Err(_elapsed) => {
                        debug!(
                            "No message received for {}s, sending ping...",
                            PING_IDLE_TIMEOUT.as_secs()
                        );
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

    // Best-effort close handshake; the connection is going away either way.
    let _ = websocket.close(None).await;
    Ok(())
}

/// Fetch the final job status so the user knows the outcome and the process
/// exit code reflects it: success only on `Completed`, error otherwise.
///
/// The DB update may lag behind the `EndOfLogs` sentinel, so poll briefly.
async fn await_final_status(client: &AuthenticatedClient, job_id: Uuid) -> Result<(), CliError> {
    eprint!("Waiting for final job status...");
    let mut poll_delay = INITIAL_POLL_DELAY;
    for _ in 0..FINAL_STATUS_POLL_ATTEMPTS {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                eprintln!();
                return Err(CliError::Interrupted);
            }
            polled = poll_job_status(client, job_id, poll_delay) => {
                poll_delay = POLL_INTERVAL;
                // A failed poll may succeed on the next attempt, so ignore
                // errors here and let the attempt counter decide.
                if let Ok(status) = polled {
                    if status != JobStatusView::Running {
                        eprintln!(" Status: {status}");
                        return match status {
                            JobStatusView::Completed => Ok(()),
                            status => Err(job_failure(status).unwrap_or(CliError::StatusUnknown)),
                        };
                    }
                    eprint!(".");
                }
            }
        }
    }
    eprintln!("Job status: unknown (timed out waiting for final status)");

    Err(CliError::StatusUnknown)
}

/// Handle the `run` command: resolve the board, upload the file, wait for the
/// job to start, stream its logs and report the final status.
pub async fn handle_run(
    cfg: UploadConfig,
    requested_board: String,
    file_path: PathBuf,
    api_key_from_env: bool,
    wait_timeout: u64,
    logging_config: crate::api::generated::types::LoggingConfig,
) -> Result<(), CliError> {
    let client = get_authenticated_client(&cfg.server, api_key_from_env).await?;

    let board_mpn = resolve_board(&client, requested_board).await?;
    info!("Running upload for board: {}", board_mpn);

    let job_id = submit_job(file_path, board_mpn, &cfg, &client, logging_config).await?;
    eprintln!("Submitted file for Job ID {job_id}");

    match wait_for_job_start(&client, job_id, Duration::from_secs(wait_timeout)).await? {
        JobStartOutcome::FinishedEarly => return Ok(()),
        JobStartOutcome::Running => {}
    }

    stream_logs(&client, job_id).await?;
    await_final_status(&client, job_id).await
}
