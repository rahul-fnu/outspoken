# Outspoken

AI-powered dictation from the terminal and desktop.

[![CI](https://github.com/rahul-fnu/outspoken/actions/workflows/ci.yml/badge.svg)](https://github.com/rahul-fnu/outspoken/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## What is Outspoken

Outspoken is a free, self-hosted AI dictation tool that runs locally on your machine. It uses OpenAI's Whisper model for speech-to-text, so your audio never leaves your computer. It works on macOS, Windows, and Linux as both a CLI tool and a desktop system tray app.

## Features

- **CLI dictation** — record and transcribe from the terminal
- **Continuous listening mode** — transcribe each utterance as a new line
- **Claude Code plugin** — use voice input directly in Claude Code via MCP
- **Voice activity detection (VAD)** — automatically detects speech boundaries
- **Filler removal** — cleans up "um", "uh", and other filler words
- **Self-correction detection** — handles mid-sentence corrections
- **System tray desktop app** — global hotkey dictation with Tauri v2
- **Multi-language support** — powered by Whisper's multilingual models
- **Automatic model download** — models are fetched on first use
- **Clipboard integration** — optionally copy transcriptions to clipboard
- **JSON output** — structured output with segments, timestamps, and duration

## Quick Start (CLI)

Install via Cargo:

```sh
cargo install --git https://github.com/rahul-fnu/outspoken.git --no-default-features
```

Then dictate:

```sh
outspoken dictate
# Recording... press Ctrl+C to stop and transcribe.
```

## Quick Start (Claude Code Plugin)

Add Outspoken as an MCP server in Claude Code:

```sh
claude mcp add outspoken -- outspoken mcp serve
```

Once added, Claude Code can use voice input through the Outspoken MCP tool.

## Building from Source

### Prerequisites

- **Rust** (stable toolchain) — [rustup.rs](https://rustup.rs)
- **cmake** and **clang** — required by whisper-rs
- **Node.js 20+** and **npm** — required for the desktop app frontend

#### macOS

```sh
xcode-select --install
brew install cmake
```

#### Linux (Ubuntu/Debian)

```sh
sudo apt-get install -y cmake clang libclang-dev libasound2-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libssl-dev libxdo-dev pkg-config
```

#### Windows

- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the C++ workload
- Install [CMake](https://cmake.org/download/)

### Build the CLI

```sh
cd src-tauri
cargo build --release --no-default-features --bin outspoken
```

The binary will be at `src-tauri/target/release/outspoken`.

### Build the Desktop App

```sh
npm ci
npm run build
cd src-tauri
cargo build --release --features desktop --bin outspoken-app
```

Or use the Tauri CLI:

```sh
npm ci
npm run tauri build
```

### Feature Flags

| Flag | Description |
|------|-------------|
| `desktop` | Enable the Tauri desktop app (default) |
| `metal` | Use Apple Metal GPU acceleration (macOS) |
| `cuda` | Use NVIDIA CUDA GPU acceleration |

Build with GPU acceleration:

```sh
# macOS with Metal
cargo build --release --features metal --bin outspoken

# Linux/Windows with CUDA
cargo build --release --features cuda --bin outspoken
```

## Usage

### `outspoken dictate`

Record from the microphone, transcribe on stop (Ctrl+C), and print to stdout.

```sh
outspoken dictate                        # basic dictation
outspoken dictate --copy                 # also copy result to clipboard
outspoken dictate --json                 # output as JSON with segments
outspoken dictate --no-vad               # disable voice activity detection
outspoken dictate --no-corrections       # disable self-correction detection
outspoken dictate --model base           # use a different model
outspoken dictate --device "MacBook Pro Microphone"  # select input device
```

### `outspoken listen`

Continuous mode — transcribes each utterance as a new line, runs until Ctrl+C.

```sh
outspoken listen                         # continuous transcription
outspoken listen --silence-timeout 3     # wait 3s of silence before finalizing (default: 2)
outspoken listen --json                  # JSON output per utterance
outspoken listen --copy                  # copy each utterance to clipboard
```

### `outspoken config`

Manage models and devices.

```sh
outspoken config models                  # list available and downloaded models
outspoken config download large-v3       # download a specific model
outspoken config devices                 # list audio input devices
```

### `outspoken mcp serve`

Start the MCP JSON-RPC server on stdio for Claude Code integration.

```sh
outspoken mcp serve
```

### `outspoken version`

Print version information.

### Common Flags

| Flag | Commands | Description |
|------|----------|-------------|
| `--model <name>` | dictate, listen | Whisper model to use (default: `large-v3-turbo-q5_0`) |
| `--copy` | dictate, listen | Copy transcription to clipboard |
| `--json` | dictate, listen | Output as JSON with segments and timestamps |
| `--no-vad` | dictate, listen | Disable voice activity detection |
| `--no-corrections` | dictate, listen | Disable self-correction detection |
| `--device <name>` | dictate, listen | Audio input device name |
| `--silence-timeout <secs>` | listen | Seconds of silence before finalizing (default: 2) |

## Configuration

### Model Selection

Models are automatically downloaded on first use. The default model is `large-v3-turbo-q5_0`. List available models with:

```sh
outspoken config models
```

Download a model ahead of time:

```sh
outspoken config download large-v3-turbo-q5_0
```

Models are stored in the platform-specific data directory (`~/.local/share/outspoken` on Linux, `~/Library/Application Support/outspoken` on macOS, `AppData` on Windows).

### Audio Device

List available input devices:

```sh
outspoken config devices
```

Select a device by name with the `--device` flag on any recording command.

## Architecture

Outspoken is built with:

- **[Tauri v2](https://v2.tauri.app/)** — desktop app framework (Rust backend + React frontend)
- **[whisper-rs](https://github.com/tazz4843/whisper-rs)** — Rust bindings for OpenAI's Whisper speech recognition
- **[cpal](https://github.com/RustAudio/cpal)** — cross-platform audio capture
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — SQLite for local data storage
- **[clap](https://github.com/clap-rs/clap)** — CLI argument parsing
- **Energy-based VAD** — lightweight voice activity detection with no external dependencies

The CLI binary (`outspoken`) and desktop app binary (`outspoken-app`) share the same core library (`outspoken_lib`).

## Contributing

Contributions are welcome! To get started:

1. Fork the repository
2. Create a feature branch (`git checkout -b my-feature`)
3. Make your changes and add tests where applicable
4. Ensure `cargo check` and `cargo test` pass in `src-tauri/`
5. Ensure `npm run build` passes for frontend changes
6. Submit a pull request

Please open an issue first for large changes to discuss the approach.

## License

MIT
