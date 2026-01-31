# Local Meeting Note Taker - Implementation Plan

## Architecture Overview

```
┌────────────────────────────────┐     ┌─────────────────────────────────┐
│     Menu Bar (System Tray)     │     │        Editor Window            │
├────────────────────────────────┤     ├─────────────────────────────────┤
│ Start/Stop │ Recent │ Prefs    │────▶│ Transcript Editor (per-segment) │
└──────┬─────────────────────────┘     │ Speaker Label Editor            │
       │                               │ [Re-run Summary] button         │
       │                               │ Summary Preview                 │
       ▼                               └─────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────┐
│                           Processing Pipeline                            │
├──────────────────────────────────────────────────────────────────────────┤
│  Audio Capture → Transcribe → [USER EDIT] → Summarize                    │
│  (ScreenCaptureKit) (whisper-rs)            (llama.cpp)                  │
│                                    ↑              │                      │
│                                    └──────────────┘                      │
│                                     re-run loop                          │
└──────────────────────────────────────────────────────────────────────────┘
```

## Workflow

1. **Record**: Click "Start" in menu bar, captures system audio + mic
2. **Process**: Stop recording → auto-runs transcription
3. **Edit**: Editor window opens with editable transcript segments
   - Each segment: `"text..." [timestamp]`
   - Can edit/merge/split transcript segments
   - Can correct transcription errors
4. **Summarize**: Click "Generate Summary" → LLM processes edited transcript
5. **Iterate**: Edit more, click "Re-generate Summary" as needed
6. **Save**: Export final transcript + summary as markdown

## Tech Stack

| Component     | Tool                    | Why                                      |
| ------------- | ----------------------- | ---------------------------------------- |
| App framework | Tauri v2                | Light (~10MB), rust backend, native feel |
| Frontend      | React + TypeScript      | Fast dev, familiar stack                 |
| Menu bar      | tauri-plugin-positioner | Position window near tray icon           |
| System audio  | `screencapturekit-rs`   | Native macOS, no drivers needed          |
| Microphone    | `cpal` crate            | Standard audio input                     |
| Transcription | `whisper-rs`            | Rust bindings to whisper.cpp, bundled    |
| Summarization | `llama-cpp-rs`          | Rust bindings to llama.cpp, bundled      |
| Storage       | JSON flat files         | Simple, no DB needed initially           |

## Design Principles

- **Self-contained**: No system-level dependencies (no global ollama, python, etc.)
- **Bundled models**: Whisper + LLM models downloaded to app data directory
- **Simple setup**: First-run wizard handles model downloads automatically

## Implementation Steps

### Phase 1: Tauri Scaffold ✅ COMPLETE

- [x] use pnpm for node package management
- [x] Init tauri project
- [x] Configure as menubar-only app (hide dock icon)
- [x] Set up system tray with basic menu (Start/Stop/Quit)
- [x] Add tauri-plugin-positioner for window positioning
- [x] add eslint + prettier + typescript configs

### Phase 2: Audio Capture ✅ COMPLETE

- [x] Rust side: use `screencapturekit-rs` for system audio
- [x] Use `cpal` for microphone input
- [x] Mix both streams into single WAV with resampling
- [x] IPC: expose start_recording / stop_recording commands
- [x] Store recordings in ~/Documents/MeetingRecordings/

### Phase 1.5: First-Run Setup Wizard ✅ COMPLETE

- [x] Detect first run (no config file / models missing)
- [x] Show setup wizard window with steps:
  1. Welcome / explain what will be downloaded
  2. Download Whisper model (~1.5GB) with progress bar
  3. Download LLM model (~2-4GB) with progress bar
- [x] Store config in app data directory
- [x] Mark setup complete, don't show again
- [x] Dev mode (DEV_MODELS=1) for testing with tiny files

### Phase 3: Transcription ✅ COMPLETE

- [x] Add `whisper-rs` crate for transcription
- [x] Load whisper model from app data directory
- [x] Implement batch transcription (post-recording)
- [x] Return timestamped segments
- [x] IPC command to transcribe audio file
- [x] UI: Transcribe button + segment display with timestamps

### Phase 3.5: Source-Based Speaker Diarization ✅ COMPLETE

Save system audio and mic audio separately, transcribe each with speaker labels.

**Recording Output Structure:**
```
~/Documents/MeetingRecordings/2026-01-15_10-30-00/
  system.wav      # stereo 48kHz, meeting participants
  mic.wav         # mono 48kHz, user's voice
  mixed.wav       # stereo 48kHz, for playback
```

**Completed:**
- [x] `audio.rs`: Add `RecordingOutput` struct, separate save functions
- [x] `audio.rs`: Modify `stop_recording()` to create timestamped subdirectory
- [x] `transcribe.rs`: Add `speaker` field to TranscriptSegment
- [x] `transcribe.rs`: Add `transcribe_recording_dir()` with merge logic
- [x] `lib.rs`: Update commands for new return types
- [x] `App.tsx`: Update types, state, and UI for speaker labels
- [x] Add rust unit tests for resample(), segment merging (11 tests)
- [x] Add vitest setup + frontend tests (9 tests)

### Phase 5: Summarization

- [ ] Add `llama-cpp-rs` or similar for LLM inference
- [ ] Load LLM model from app data directory
- [ ] Prompt design: extract summary, key points, action items
- [ ] Generate markdown output
- [ ] IPC command to summarize transcript

### Phase 6: Editor Window

- [ ] Create separate Tauri window for editor (larger, resizable)
- [ ] TranscriptEditor component: list of editable segments
- [ ] SegmentRow: inline edit text, timestamp display
- [ ] Segment operations: merge adjacent, split at cursor, delete

### Phase 7: Summary Pipeline UI

- [ ] SummaryPanel: shows generated summary in markdown preview
- [ ] "Generate Summary" / "Re-generate" button
- [ ] Loading states during LLM processing
- [ ] Side-by-side or tabbed layout (transcript | summary)

### Phase 8: Storage & Persistence

- [ ] Data model: Meeting → Segments[] → Summary
- [ ] Save to `~/Documents/MeetingNotes/{date}-{title}/`
- [ ] Export as markdown: transcript.md, summary.md

### Phase 9: Polish

- [ ] System notifications (recording start/stop/complete)
- [ ] Preferences panel (model selection, output dir)
- [ ] Keyboard shortcuts
- [ ] Error handling and recovery

### Future: ML-Based Speaker Diarization (DEFERRED)

Full speaker diarization within system audio (distinguishing Alice from Bob):
- [ ] Research self-contained diarization options
- [ ] Options: pyannote via embedded python, ONNX export, or simpler clustering
- [ ] Add speaker labels beyond "Me" / "Meeting"
- [ ] SpeakerManager: rename "Speaker 1" → "Alice", merge speakers

## Model Sizes & Storage

| Model              | Size    | Location                               |
| ------------------ | ------- | -------------------------------------- |
| Whisper base.en    | ~150MB  | `~/.local/share/meeting-recorder/`     |
| Whisper large-v3   | ~1.5GB  | `~/.local/share/meeting-recorder/`     |
| LLM (Llama 3.2 3B) | ~2GB    | `~/.local/share/meeting-recorder/`     |

Total first-run download: ~2-4GB depending on model choices

## Data Model

```typescript
interface Meeting {
  id: string;
  title: string;
  createdAt: Date;
  duration: number; // seconds
  audioPath: string; // path to WAV
  segments: Segment[];
  summary?: string; // markdown
}

interface Segment {
  id: string;
  text: string;
  startTime: number; // seconds
  endTime: number;
  isEdited: boolean; // track user modifications
}
```

## File Structure

```
local-meeting-recorder/
├── src/                      # React frontend
│   ├── App.tsx
│   ├── windows/
│   │   ├── TrayWindow.tsx    # Small dropdown from menu bar
│   │   ├── EditorWindow.tsx  # Main transcript editor
│   │   └── SetupWizard.tsx   # First-run model download wizard
│   ├── components/
│   │   ├── TranscriptEditor.tsx
│   │   ├── SegmentRow.tsx
│   │   ├── SummaryPanel.tsx
│   │   └── ProcessingStatus.tsx
│   └── services/
│       ├── transcribe.ts
│       ├── summarize.ts
│       └── storage.ts
├── src-tauri/               # Rust backend
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── audio.rs         # Audio recording
│   │   ├── transcribe.rs    # Whisper integration
│   │   └── summarize.rs     # LLM integration
│   ├── Cargo.toml
│   └── tauri.conf.json
├── package.json
└── PLAN.md
```

## Rust Dependencies

```toml
[dependencies]
# Tauri
tauri = { version = "2", features = ["macos-private-api", "tray-icon"] }
tauri-plugin-positioner = { version = "2", features = ["tray-icon"] }

# Audio
screencapturekit = "1.5"
cpal = "0.17"
hound = "3.5"

# ML (to be added)
whisper-rs = "0.12"           # Whisper bindings
# llama-cpp-rs or similar     # LLM bindings

# Utils
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["sync", "rt-multi-thread"] }
parking_lot = "0.12"
chrono = "0.4"
dirs = "5"
reqwest = { version = "0.12", features = ["stream"] }  # For model downloads
```
