# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/onmcu/onmcu-rs/compare/v0.2.0...v0.3.0) - 2026-08-10

### Added

- add -s/--server config override ([#61](https://github.com/onmcu/onmcu-rs/pull/61))
- store api key per hostname ([#58](https://github.com/onmcu/onmcu-rs/pull/58))

## [0.2.0](https://github.com/onmcu/onmcu-rs/compare/v0.1.0...v0.2.0) - 2026-08-07

### Added

- clippy pedantic and nursery lints ([#55](https://github.com/onmcu/onmcu-rs/pull/55))
- add update notification ([#48](https://github.com/onmcu/onmcu-rs/pull/48))
- check if controller version is supported ([#44](https://github.com/onmcu/onmcu-rs/pull/44))
- partially mask API key input
- accept board names case-insensitively

### Changed

- name canonical board MPN explicitly
- rename config timeout_seconds to job_timeout_seconds

### Dependencies

- *(deps)* bump clap from 4.6.4 to 4.6.5 in the cargo-minor-and-patch group ([#53](https://github.com/onmcu/onmcu-rs/pull/53))
- *(deps)* bump toml from 1.1.3+spec-1.1.0 to 1.1.4+spec-1.1.0 in the cargo-minor-and-patch group across 1 directory ([#50](https://github.com/onmcu/onmcu-rs/pull/50))

### Fixed

- appease clippy's print_literal in the list-boards header
- size list-boards columns to the longest entry
- stop requiring a readable config file for login
- report a mid-session 401 as invalid key, not access denied
- close the log stream websocket instead of just dropping it
- stat the upload file once and reject empty files early
- print device log lines as-is so output is pipeable
- validate ONMCU_API_KEY up front instead of sending garbage to the server
- validate chunk_size when loading the config and report limits in bytes
- refuse plain-http server URLs so the API key can't leak in cleartext
- hide the API key while typing and keep it in a SecretString
- step board pagination by items received, not page size
- show the login success message without needing -v
- make Ctrl+C actually stop the CLI when stdin is not a terminal
- don't retry chunk uploads on errors that can't succeed anyway
- cap chunk retry backoff so high retry counts can't overflow or stall for hours
- propagate read errors while hashing instead of treating them as EOF

### Performance

- share chunk buffers between retries instead of cloning them
- hash the upload file off the async runtime

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
