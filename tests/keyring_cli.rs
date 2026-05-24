//! CLI behavior when the OS keyring backend is unavailable.
//!
//! Linux-only: we simulate "no Secret Service" by pointing D-Bus at a
//! nonexistent socket, which is meaningless on macOS/Windows where the native
//! keyring is always present.
#![cfg(target_os = "linux")]

use std::process::{Command, Stdio};

/// Run the onmcu binary with the Secret Service made unreachable and no
/// `ONMCU_API_KEY` in the environment. Returns combined stdout+stderr, since
/// the CLI emits some errors via anyhow (stderr) and some via tracing (stdout).
fn onmcu_without_keyring(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_onmcu"))
        .args(args)
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .env_remove("ONMCU_API_KEY")
        .stdin(Stdio::null())
        .output()
        .expect("failed to run onmcu binary");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), combined)
}

/// `login` needs the keyring, so it must fail with the actionable hint.
#[test]
fn login_reports_missing_keyring() {
    let (ok, output) = onmcu_without_keyring(&["login"]);
    assert!(!ok, "expected failure, output: {output}");
    assert!(
        output.contains("OS keyring is not available"),
        "output: {output}"
    );
    assert!(output.contains("ONMCU_API_KEY"), "output: {output}");
}

/// Commands that read the key from the keyring report the same hint.
#[test]
fn keyring_command_reports_missing_keyring() {
    let (ok, output) = onmcu_without_keyring(&["list-boards"]);
    assert!(!ok, "expected failure, output: {output}");
    assert!(
        output.contains("OS keyring is not available"),
        "output: {output}"
    );
}

/// `--api-key-from-env` must not touch the keyring: with the env var unset the
/// error is about the missing env var, never the keyring (headless regression).
#[test]
fn env_key_path_ignores_keyring() {
    let (ok, output) = onmcu_without_keyring(&["list-boards", "--api-key-from-env"]);
    assert!(!ok, "expected failure, output: {output}");
    assert!(
        output.contains("ONMCU_API_KEY is missing"),
        "output: {output}"
    );
    assert!(
        !output.contains("OS keyring is not available"),
        "env path must not mention the keyring, output: {output}"
    );
}
