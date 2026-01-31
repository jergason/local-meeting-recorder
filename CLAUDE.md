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

Set `DEV_MODELS=1` env var to use 1KB dummy model files instead of real 2GB+ models during development.

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

- **lib.rs**: Tauri command handlers, AppState with Mutex-wrapped recorder/config
- **audio.rs**: AudioRecorder - captures system audio (SCStream) + mic (CPAL) → WAV files
- **transcribe.rs**: whisper-rs wrapper, processes system.wav ("Meeting" speaker) + mic.wav ("Me" speaker), merges by timestamp
- **summarize.rs**: llama-cpp-2 wrapper, builds prompt, parses markdown sections into SummaryResult
- **config.rs**: AppConfig stored at `~/.local/share/meeting-recorder/config.json`, tracks model paths
- **download.rs**: model downloading with progress events

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

Summarize: transcript → LLM prompt → parse ## Summary, ## Key Points, ## Action Items
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

- Models must be GGUF format (ggml-*.bin for Whisper, *.gguf for Llama)
- Audio filenames hardcoded in transcribe.rs (system.wav, mic.wav)
- App checks `needs_setup()` on launch—setup wizard must complete before main UI shows
