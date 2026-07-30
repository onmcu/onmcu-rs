# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/onmcu/onmcu-rs/compare/v0.0.1...v0.1.0) - 2026-06-24

### Added

- accept trailing args via `--ignore-trailing-args`
- default missing config keys, reject unknown keys, add `config.example.toml`
- stream serial logs from the device

### Changed

- replace anyhow with thiserror and stable exit codes
- return `ApiError` from `cancel_job` instead of `JobCancelFailed`

### Documentation

- change the config example in the readme to the canonical server
- make the readme more concise

### Fixed

- propagate job failure to the process exit code
- prompt to cancel the job on Ctrl+C while waiting in the queue
- report user-friendly errors for denied and invalid API keys (#8)
- keep the keyring across sessions on Linux by switching to the dbus
  secret-service store (#18)
- build on Linux with vendored libdbus, and handle a missing keyring gracefully

## [0.0.1](https://github.com/onmcu/onmcu-rs/releases/tag/v0.0.1) - 2026-05-20

### Added

- initial release of the `onmcu` CLI, with the `run`, `login`, and
  `list-boards` commands
