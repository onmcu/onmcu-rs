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

use crate::api::generated::types::JobStatusView;
use crate::api::interface::{fetch_all_boards, is_board_supported};
use crate::api::{ApiError, get_authenticated_client};
use crate::error::CliError;
use crate::upload::UploadConfig;
use crate::upload::submit_job;

/// How many times to poll for the final job status after the log stream ends,
/// at one-second intervals. The job status can lag the end-of-logs marker, so
/// allow a brief grace period before reporting the status as unknown.
const FINAL_STATUS_POLL_ATTEMPTS: u32 = 10;

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
        let wait_start = tokio::time::Instant::now();
        loop {
            if wait_start.elapsed() > max_wait {
                eprintln!();
                eprintln!(
                    "No device available after {}s. Wait another {}s or cancel? [w/c]: ",
                    max_wait.as_secs(),
                    max_wait.as_secs()
                );

                let mut reader = BufReader::new(tokio::io::stdin());
                let mut response = String::new();
                if reader.read_line(&mut response).await.is_ok() {
                    let response = response.trim().to_lowercase();
                    if response == "w" || response == "wait" {
                        eprint!("⏳ Waiting for job to start...");
                        continue 'wait;
                    }
                }

                // Cancel the pending job so it doesn't sit in the queue
                info!("Cancelling pending job...");
                if let Err(e) = client
                    .api()
                    .cancel_job()
                    .id(job_id)
                    .x_api_key(client.api_key.expose_secret())
                    .send()
                    .await
                {
                    error!("Failed to cancel job: {e}");
                }
                return Err(CliError::NoDeviceAvailable);
            }

            let job = client
                .api()
                .get_job()
                .id(job_id)
                .x_api_key(client.api_key.expose_secret())
                .send()
                .await
                .map_err(ApiError::from)
                .map_err(CliError::JobStatus)?;

            match job.into_inner().status {
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
                _ => {
                    // Still pending/dispatched — wait and retry
                    eprint!(".");
                    tokio::time::sleep(Duration::from_secs(1)).await;
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
                eprintln!("Received Ctrl+C. Do you want to cancel the job? [y/n]");

                let mut reader = BufReader::new(tokio::io::stdin());
                let mut response = String::new();

                if reader.read_line(&mut response).await.is_ok() {
                    let response = response.trim().to_lowercase();
                    if response == "y" || response == "yes" {
                        info!("Cancelling job...");
                        match client.api().cancel_job().id(job_id).x_api_key(client.api_key.expose_secret()).send().await {
                            Ok(_) => info!("Job cancelled successfully"),
                            Err(e) => error!("Failed to cancel job: {}", e),
                        }
                    } else {
                        info!("Job cancellation aborted, continuing to stream logs...");
                        continue;
                    }
                }

                break;
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
