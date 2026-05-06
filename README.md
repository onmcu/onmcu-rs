# onmcu

The official `onmcu` CLI: a tool for remote MCU development, flashing, and
testing on the [OnMCU](https://onmcu.com) platform.

## Install

### Linux / macOS

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/onmcu/onmcu-rs/releases/latest/download/onmcu-installer.sh | sh
```

### Windows (PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/onmcu/onmcu-rs/releases/latest/download/onmcu-installer.ps1 | iex"
```

### From crates.io

```sh
cargo install onmcu --locked
```

### Pre-built binaries

Download the archive matching your platform from the
[latest release](https://github.com/onmcu/onmcu-rs/releases/latest):

- Linux x86_64: `onmcu-x86_64-unknown-linux-gnu.tar.xz`
- Linux aarch64: `onmcu-aarch64-unknown-linux-gnu.tar.xz`
- macOS Apple Silicon: `onmcu-aarch64-apple-darwin.tar.xz`
- macOS Intel: `onmcu-x86_64-apple-darwin.tar.xz`
- Windows x86_64: `onmcu-x86_64-pc-windows-msvc.zip`

## Usage

```sh
# Store your API key in the OS keyring (one-time setup)
onmcu login

# List available boards
onmcu list-boards

# Flash and run firmware on a remote board
onmcu run --board NUCLEO-H755ZI-Q --file ./target/thumbv7em-none-eabihf/release/blinky
```

Get your API key at <https://app.onmcu.com/settings>.

### Configuration

By default the CLI talks to `https://ctrl1.onmcu.com`. To point it at a
different controller, supply a TOML config file via `--config` or
`ONMCU_CLI_CONFIG_PATH`:

```toml
server = "https://controller.onmcu.com"
chunk_size = 5
retries = 3
timeout_seconds = 600
```

## Development

This repository is the public, standalone home of the `onmcu` CLI. The
generated API client (`src/api/generated.rs`) is built at compile time
from `openapi/openapi.json`, which is auto-synced from the upstream
controller via the `openapi-sync` workflow.

```sh
cargo build
cargo test
cargo run -- --help
```

## License

[MIT](LICENSE)
