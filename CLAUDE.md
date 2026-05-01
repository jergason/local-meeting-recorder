# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
pnpm install              # install dependencies
pnpm tauri dev            # run app in dev mode
pnpm test                 # run vitest tests
pnpm test:watch           # run tests in watch mode
pnpm lint:fix             # eslint + prettier
pnpm typecheck            # tsc --noEmit
```

Set `DEV_MODELS=1` to use 1KB dummy whisper model files during development.

LLM summarization runs against Ollama at `localhost:11434`. The bundled sidecar binary must be present at `src-tauri/binaries/ollama-{target-triple}` — populate via `./scripts/fetch-ollama.sh` (copies the binary out of `/Applications/Ollama.app`). At least one chat model must be pulled (`ollama pull qwen3.5:latest`) and named in `config.json`'s `llm_model` field.

Tests:
```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib                       # fast unit tests
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored --nocapture  # slow e2e (real models)
```

## Architecture

Tauri v2 desktop app: React 19 frontend + Rust backend. macOS only (uses ScreenCaptureKit for system audio).

### IPC Pattern

Frontend calls Rust via `invoke()` from `@tauri-apps/api/core`. Backend emits events via `app.emit()`, frontend listens with `listen()`.

```typescript
// frontend
const result = await invoke<TranscriptionResult>("transcribe_recording", { recordingDir });

// backend (lib.rs)
#[tauri::command]
async fn transcribe_recording(recording_dir: String) -> Result<TranscriptionResult, String>
```

All CPU-heavy operations (transcription, summarization) use `tokio::task::spawn_blocking()`.

### Backend Modules (src-tauri/src/)

- **lib.rs**: Tauri command handlers, AppState with Mutex-wrapped recorder/config. Spawns `ollama serve` as a Tauri sidecar at startup (`tauri-plugin-shell`). If a system Ollama already binds 11434, the spawned child exits and the existing server handles requests.
- **audio.rs**: AudioRecorder - captures system audio (SCStream) + mic (CPAL) → WAV files
- **transcribe.rs**: whisper-rs wrapper, processes system.wav ("Meeting" speaker) + mic.wav ("Me" speaker), merges by timestamp. Whisper inference runs on a `std::thread::Builder` with a 64MB stack — tokio's blocking pool default (2MB) is too small for whisper's encoder and will SIGSEGV.
- **summarize.rs**: HTTP client for Ollama at `localhost:11434/api/chat`. Ollama applies the model's chat template based on its Modelfile, so we just send role-tagged messages. `AppConfig.llm_model` is an Ollama tag (e.g. `qwen3.5:latest`), not a filename.
- **config.rs**: AppConfig stored at `~/.local/share/meeting-recorder/config.json`. `whisper_model` is still a filename (whisper-rs loads from disk); `llm_model` is now an Ollama tag.
- **download.rs**: whisper model downloading with progress events. LLM models are pulled by Ollama (`ollama pull <tag>`) — not handled here.

### Frontend Components (src/)

- **App.tsx**: routes by window label ("main" → SetupWizard or RecorderUI, "editor" → EditorWindow)
- **RecorderUI.tsx**: tray window UI for record/transcribe/summarize flow
- **EditorWindow.tsx**: separate window for transcript editing, receives data via "editor-data" event
- **SetupWizard.tsx**: first-run model download wizard

### Data Flow

```
Record: system audio + mic → ~/Documents/MeetingRecordings/YYYY-MM-DD_HH-MM-SS/
        ├── system.wav (other participants)
        ├── mic.wav (user)
        └── mixed.wav (playback)

Transcribe: system.wav → "Meeting" segments
            mic.wav → "Me" segments
            merge by start_time → TranscriptionResult

Summarize: transcript → POST localhost:11434/api/chat → parse ## Summary, ## Key Points, ## Action Items
```

### Windows

- **main**: small tray dropdown, hidden from dock (ActivationPolicy::Accessory), auto-hides on blur
- **editor**: created on-demand via `open_editor()`, 900x700, receives data via event

## Code Style

- Functional React with hooks, prefer immutable state updates (`.map()`, `.filter()`)
- TypeScript strict mode, interfaces defined at component level
- Rust errors are simple `Result<T, String>` (no custom error types)
- Prettier: 2-space, double quotes, trailing commas es5

## Key Types

```typescript
interface TranscriptSegment {
  id: string;           // UUID
  text: string;
  start_time: number;   // seconds
  end_time: number;
  speaker: string;      // "Me" or "Meeting"
}

interface SummaryResult {
  summary: string;
  key_points: string[];
  action_items: string[];
}
```

## Gotchas

- Whisper models are GGUF format (`ggml-*.bin`) loaded directly; LLM is whatever Ollama has pulled (`ollama pull qwen3.5:latest`).
- Audio filenames hardcoded in transcribe.rs (system.wav, mic.wav)
- App checks `needs_setup()` on launch—setup wizard must complete before main UI shows
- The bundled Ollama sidecar lives at `src-tauri/binaries/ollama-{aarch64,x86_64}-apple-darwin`. Not checked in (gitignored); populate locally via `./scripts/fetch-ollama.sh`. Without it, `pnpm tauri dev` and bundle builds will fail.
- Whisper inference must NOT run on `tokio::task::spawn_blocking` — its 2MB stack overflows. Use `std::thread::Builder::new().stack_size(64 * 1024 * 1024)` and bridge with a `tokio::sync::oneshot` channel.
