//! CLI behavior for the `run` subcommand's trailing-argument handling.
//!
//! When `--ignore-trailing-args` is not set, stray trailing arguments after
//! `run` should be rejected with exit code 2 and a clear error message.
//! When it *is* set, those arguments are silently discarded.
//!
//! Tests pass `--api-key-from-env` with `ONMCU_API_KEY` removed so any command
//! that gets past the args check fails deterministically on the missing env
//! var, without consulting the OS keyring or contacting the API.

use std::process::{Command, ExitStatus};

/// Run the onmcu binary with the given arguments, capturing combined output and exit status.
fn onmcu(args: &[&str]) -> (ExitStatus, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_onmcu"))
        .args(args)
        .env_remove("ONMCU_API_KEY")
        .output()
        .expect("failed to run onmcu binary");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status, combined)
}

/// Stray trailing args are rejected when `--ignore-trailing-args` is absent.
#[test]
fn trailing_args_rejected_without_flag() {
    let (status, output) = onmcu(&[
        "run",
        "--board",
        "nrf52840dk",
        "--file",
        "firmware.bin",
        "--exact",
        "--nocapture",
    ]);
    assert!(!status.success(), "expected failure, output: {output}");

    // Exit code 2 signals a CLI usage error
    assert_eq!(
        status.code(),
        Some(2),
        "expected exit code 2, got {:?}, output: {output}",
        status.code()
    );

    assert!(output.contains("Unexpected arguments"), "output: {output}");
    assert!(
        output.contains("--exact"),
        "must mention the offending arg, output: {output}"
    );
    assert!(
        output.contains("--nocapture"),
        "must mention all offending args, output: {output}"
    );
    assert!(
        output.contains("--ignore-trailing-args"),
        "must suggest the flag, output: {output}"
    );
}

/// When `--ignore-trailing-args` is set, the trailing args are discarded and
/// validation proceeds further (fails on the missing API key, not on args).
#[test]
fn trailing_args_accepted_with_flag() {
    let (status, output) = onmcu(&[
        "run",
        "--board",
        "nrf52840dk",
        "--file",
        "firmware.bin",
        "--api-key-from-env",
        "--ignore-trailing-args",
        "--exact",
        "--nocapture",
    ]);
    // The args check passes, so it should NOT be the "Unexpected arguments" error.
    assert!(!status.success(), "expected failure, output: {output}");
    assert!(
        !output.contains("Unexpected arguments"),
        "must not reject trailing args when flag is set, output: {output}"
    );
    // Downstream error confirms the args check was bypassed
    assert!(
        output.contains("ONMCU_API_KEY"),
        "should fail downstream of the args check, output: {output}"
    );
}

/// No trailing args should still work as before (fails on the missing API key, not on args).
#[test]
fn no_trailing_args_still_works() {
    let (status, output) = onmcu(&[
        "run",
        "--board",
        "nrf52840dk",
        "--file",
        "firmware.bin",
        "--api-key-from-env",
    ]);
    assert!(!status.success(), "expected failure, output: {output}");
    assert!(
        !output.contains("Unexpected arguments"),
        "no trailing args should not trigger this error, output: {output}"
    );
    assert!(
        output.contains("ONMCU_API_KEY"),
        "should fail downstream of the args check, output: {output}"
    );
}

/// The `--ignore-trailing-args` flag alone (without trailing args) must also work.
#[test]
fn ignore_flag_without_trailing_args() {
    let (status, output) = onmcu(&[
        "run",
        "--board",
        "nrf52840dk",
        "--file",
        "firmware.bin",
        "--api-key-from-env",
        "--ignore-trailing-args",
    ]);
    assert!(!status.success(), "expected failure, output: {output}");
    assert!(
        !output.contains("Unexpected arguments"),
        "must not invent an error when no trailing args, output: {output}"
    );
    assert!(
        output.contains("ONMCU_API_KEY"),
        "should fail downstream of the args check, output: {output}"
    );
}
