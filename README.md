# Local Meeting Recorder

Tauri desktop app that records meetings (system audio + mic), transcribes them with Whisper, and summarizes them with a local LLM. Everything runs on-device. macOS only (uses ScreenCaptureKit for system audio).

## Stack

- Tauri v2 + Rust backend
- React 19 + TypeScript + Vite frontend
- `whisper-rs` for transcription
- [Ollama](https://ollama.com/) sidecar for LLM summarization (HTTP API at `localhost:11434`)
- ScreenCaptureKit (system audio) + CPAL (mic)

## Requirements

- macOS
- Node 20+ and [pnpm](https://pnpm.io/)
- Rust toolchain (`rustup`)
- Xcode command line tools
- [Ollama](https://ollama.com/download) installed (the binary is bundled into the app, but `./scripts/fetch-ollama.sh` copies it from `/Applications/Ollama.app`)

## Setup

```bash
pnpm install
./scripts/fetch-ollama.sh        # populate src-tauri/binaries/ from /Applications/Ollama.app
ollama pull qwen3.5:latest       # or any chat-capable model
pnpm tauri dev
```

On first launch the setup wizard downloads the Whisper model. The LLM is whatever you've pulled in Ollama — set the tag in `~/.local/share/meeting-recorder/config.json`'s `llm_model` field (default `qwen3.5:latest`).

To skip the Whisper download during development, use a 1KB dummy file:

```bash
DEV_MODELS=1 pnpm tauri dev
```

Recordings are saved to `~/Documents/MeetingRecordings/<timestamp>/` as `system.wav`, `mic.wav`, and `mixed.wav`.

## Commands

```bash
pnpm tauri dev      # run app in dev mode
pnpm test           # vitest (frontend)
pnpm test:watch     # vitest in watch mode
pnpm lint:fix       # eslint + prettier
pnpm typecheck      # tsc --noEmit

# rust tests
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored --nocapture   # slow e2e against real models
```

## Permissions

macOS will prompt for Screen Recording (system audio capture) and Microphone access on first run. Grant both in System Settings → Privacy & Security.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
