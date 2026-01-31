import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./App.css";
import EditorWindow from "./EditorWindow";

interface ModelInfo {
  id: string;
  name: string;
  size_bytes: number;
  url: string;
  filename: string;
}

interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
  percent: number;
}

interface MixingProgress {
  current_frame: number;
  total_frames: number;
  percent: number;
}

interface TranscriptionProgress {
  phase: string;
  file_percent: number;
  overall_percent: number;
}

interface TranscriptSegment {
  id: string;
  text: string;
  start_time: number;
  end_time: number;
  speaker: string;
}

interface RecordingOutput {
  directory: string;
  system_file: string;
  mic_file: string;
  mixed_file: string;
}

interface TranscriptionResult {
  segments: TranscriptSegment[];
  full_text: string;
  duration: number;
}

interface SummaryResult {
  summary: string;
  key_points: string[];
  action_items: string[];
}

type SetupStep = "welcome" | "whisper" | "llm" | "complete";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  if (bytes < 1024 * 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(1) + " MB";
  return (bytes / (1024 * 1024 * 1024)).toFixed(2) + " GB";
}

function SetupWizard({ onComplete }: { onComplete: () => void }) {
  const [step, setStep] = useState<SetupStep>("welcome");
  const [whisperModels, setWhisperModels] = useState<ModelInfo[]>([]);
  const [llmModels, setLlmModels] = useState<ModelInfo[]>([]);
  const [selectedWhisper, setSelectedWhisper] = useState<string>("");
  const [selectedLlm, setSelectedLlm] = useState<string>("");
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Load available models
    invoke<ModelInfo[]>("get_whisper_models").then(setWhisperModels);
    invoke<ModelInfo[]>("get_llm_models").then(setLlmModels);

    // Listen for download progress
    const unlisten = listen<DownloadProgress>("download-progress", (event) => {
      setProgress(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (whisperModels.length > 0 && !selectedWhisper) {
      setSelectedWhisper(whisperModels[0].id);
    }
  }, [whisperModels, selectedWhisper]);

  useEffect(() => {
    if (llmModels.length > 0 && !selectedLlm) {
      setSelectedLlm(llmModels[0].id);
    }
  }, [llmModels, selectedLlm]);

  async function downloadWhisper() {
    setDownloading(true);
    setError(null);
    setProgress(null);
    try {
      await invoke("download_whisper_model", { modelId: selectedWhisper });
      setStep("llm");
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(false);
      setProgress(null);
    }
  }

  async function downloadLlm() {
    setDownloading(true);
    setError(null);
    setProgress(null);
    try {
      await invoke("download_llm_model", { modelId: selectedLlm });
      await invoke("complete_setup");
      setStep("complete");
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(false);
      setProgress(null);
    }
  }

  const selectedWhisperModel = whisperModels.find((m) => m.id === selectedWhisper);
  const selectedLlmModel = llmModels.find((m) => m.id === selectedLlm);

  return (
    <div className="setup-wizard">
      {step === "welcome" && (
        <div className="setup-step">
          <h1>Welcome to Meeting Recorder</h1>
          <p>Let's set up the AI models for transcription and summarization.</p>
          <p className="info">
            This will download approximately{" "}
            <strong>
              {formatBytes(
                (selectedWhisperModel?.size_bytes || 0) + (selectedLlmModel?.size_bytes || 0)
              )}
            </strong>{" "}
            of model files.
          </p>
          <button onClick={() => setStep("whisper")} className="primary-btn">
            Get Started
          </button>
        </div>
      )}

      {step === "whisper" && (
        <div className="setup-step">
          <h2>Step 1: Transcription Model</h2>
          <p>Choose a Whisper model for speech-to-text:</p>

          <div className="model-options">
            {whisperModels.map((model) => (
              <label key={model.id} className="model-option">
                <input
                  type="radio"
                  name="whisper"
                  value={model.id}
                  checked={selectedWhisper === model.id}
                  onChange={(e) => setSelectedWhisper(e.target.value)}
                  disabled={downloading}
                />
                <div className="model-info">
                  <span className="model-name">{model.name}</span>
                  <span className="model-size">{formatBytes(model.size_bytes)}</span>
                </div>
              </label>
            ))}
          </div>

          {downloading && progress && (
            <div className="progress-container">
              <div className="progress-bar">
                <div className="progress-fill" style={{ width: `${progress.percent}%` }} />
              </div>
              <span className="progress-text">
                {formatBytes(progress.downloaded)} / {formatBytes(progress.total)} (
                {progress.percent.toFixed(1)}%)
              </span>
            </div>
          )}

          {error && <p className="error">{error}</p>}

          <button onClick={downloadWhisper} disabled={downloading} className="primary-btn">
            {downloading ? "Downloading..." : "Download & Continue"}
          </button>
        </div>
      )}

      {step === "llm" && (
        <div className="setup-step">
          <h2>Step 2: Summarization Model</h2>
          <p>Choose an LLM for generating meeting summaries:</p>

          <div className="model-options">
            {llmModels.map((model) => (
              <label key={model.id} className="model-option">
                <input
                  type="radio"
                  name="llm"
                  value={model.id}
                  checked={selectedLlm === model.id}
                  onChange={(e) => setSelectedLlm(e.target.value)}
                  disabled={downloading}
                />
                <div className="model-info">
                  <span className="model-name">{model.name}</span>
                  <span className="model-size">{formatBytes(model.size_bytes)}</span>
                </div>
              </label>
            ))}
          </div>

          {downloading && progress && (
            <div className="progress-container">
              <div className="progress-bar">
                <div className="progress-fill" style={{ width: `${progress.percent}%` }} />
              </div>
              <span className="progress-text">
                {formatBytes(progress.downloaded)} / {formatBytes(progress.total)} (
                {progress.percent.toFixed(1)}%)
              </span>
            </div>
          )}

          {error && <p className="error">{error}</p>}

          <button onClick={downloadLlm} disabled={downloading} className="primary-btn">
            {downloading ? "Downloading..." : "Download & Finish"}
          </button>
        </div>
      )}

      {step === "complete" && (
        <div className="setup-step">
          <h2>Setup Complete!</h2>
          <p>All models have been downloaded. You're ready to start recording meetings.</p>
          <button onClick={onComplete} className="primary-btn">
            Start Using Meeting Recorder
          </button>
        </div>
      )}
    </div>
  );
}

function formatTime(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);
  if (hours > 0) {
    return `${hours}:${mins.toString().padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
  }
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

interface RecordingStats {
  duration_secs: number;
  system_samples_written: number;
  mic_samples_written: number;
}

// milestone thresholds in seconds
const MILESTONES = [
  { secs: 30 * 60, label: "30 minutes" },
  { secs: 60 * 60, label: "1 hour" },
  { secs: 2 * 60 * 60, label: "2 hours" },
];

function RecorderUI() {
  const [isRecording, setIsRecording] = useState(false);
  const [status, setStatus] = useState("Ready");
  const [lastRecording, setLastRecording] = useState<RecordingOutput | null>(null);
  const [transcribing, setTranscribing] = useState(false);
  const [transcription, setTranscription] = useState<TranscriptionResult | null>(null);
  const [summarizing, setSummarizing] = useState(false);
  const [summary, setSummary] = useState<SummaryResult | null>(null);
  const [elapsedTime, setElapsedTime] = useState(0);
  const [warning, setWarning] = useState<string | null>(null);
  const [milestonesReached, setMilestonesReached] = useState<Set<number>>(new Set());
  const [processingProgress, setProcessingProgress] = useState<{
    phase: "idle" | "mixing" | "transcribing";
    percent: number;
    label: string;
  }>({ phase: "idle", percent: 0, label: "" });

  useEffect(() => {
    invoke<boolean>("is_recording").then(setIsRecording);
  }, []);

  // listen for mixing and transcription progress events
  useEffect(() => {
    const unlistenMixing = listen<MixingProgress>("mixing-progress", (event) => {
      setProcessingProgress({
        phase: "mixing",
        percent: event.payload.percent,
        label: "Mixing audio...",
      });
    });

    const unlistenTranscription = listen<TranscriptionProgress>("transcription-progress", (event) => {
      const label =
        event.payload.phase === "system"
          ? "Transcribing meeting audio..."
          : "Transcribing your audio...";
      setProcessingProgress({
        phase: "transcribing",
        percent: event.payload.overall_percent,
        label,
      });
    });

    return () => {
      unlistenMixing.then((fn) => fn());
      unlistenTranscription.then((fn) => fn());
    };
  }, []);

  // poll recording stats while recording
  useEffect(() => {
    if (!isRecording) {
      setElapsedTime(0);
      setMilestonesReached(new Set());
      return;
    }

    const interval = setInterval(async () => {
      try {
        const stats = await invoke<RecordingStats | null>("get_recording_stats");
        if (stats) {
          setElapsedTime(stats.duration_secs);

          // check milestones
          for (const milestone of MILESTONES) {
            if (stats.duration_secs >= milestone.secs && !milestonesReached.has(milestone.secs)) {
              setMilestonesReached((prev) => new Set([...prev, milestone.secs]));
              setWarning(`Recording has been running for ${milestone.label}`);

              // send system notification
              try {
                const { sendNotification, isPermissionGranted, requestPermission } = await import(
                  "@tauri-apps/plugin-notification"
                );
                let permitted = await isPermissionGranted();
                if (!permitted) {
                  const permission = await requestPermission();
                  permitted = permission === "granted";
                }
                if (permitted) {
                  sendNotification({
                    title: "Meeting Recorder",
                    body: `Recording has been running for ${milestone.label}`,
                  });
                }
              } catch (e) {
                console.error("Failed to send notification:", e);
              }

              // auto-dismiss warning after 10 seconds
              setTimeout(() => setWarning(null), 10000);
            }
          }
        }
      } catch (e) {
        console.error("Failed to get recording stats:", e);
      }
    }, 5000);

    return () => clearInterval(interval);
  }, [isRecording, milestonesReached]);

  async function startRecording() {
    try {
      setStatus("Starting...");
      await invoke("start_recording");
      setIsRecording(true);
      setStatus("Recording");
      setLastRecording(null);
      setTranscription(null);
      setSummary(null);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  }

  async function stopRecording() {
    try {
      setStatus("Stopping...");
      const output = await invoke<RecordingOutput>("stop_recording");
      setIsRecording(false);
      setStatus("Saved");
      setLastRecording(output);
      setProcessingProgress({ phase: "idle", percent: 0, label: "" });
    } catch (e) {
      setStatus(`Error: ${e}`);
      setProcessingProgress({ phase: "idle", percent: 0, label: "" });
    }
  }

  async function transcribeRecording() {
    if (!lastRecording) return;
    try {
      setTranscribing(true);
      setStatus("Transcribing...");
      const result = await invoke<TranscriptionResult>("transcribe_recording", {
        recordingDir: lastRecording.directory,
      });
      setTranscription(result);
      setSummary(null); // Clear any previous summary
      setStatus("Transcribed");
    } catch (e) {
      setStatus(`Transcription error: ${e}`);
    } finally {
      setTranscribing(false);
      setProcessingProgress({ phase: "idle", percent: 0, label: "" });
    }
  }

  async function summarizeTranscript() {
    if (!transcription) return;
    try {
      setSummarizing(true);
      setStatus("Summarizing...");
      const result = await invoke<SummaryResult>("summarize_transcript", {
        transcript: transcription,
      });
      setSummary(result);
      setStatus("Summarized");
    } catch (e) {
      setStatus(`Summarization error: ${e}`);
    } finally {
      setSummarizing(false);
    }
  }

  async function openEditor() {
    if (!lastRecording || !transcription) return;
    try {
      await invoke("open_editor", {
        recordingDir: lastRecording.directory,
        transcript: transcription,
        summary: summary,
      });
    } catch (e) {
      setStatus(`Failed to open editor: ${e}`);
    }
  }

  return (
    <main className="container">
      <h1>Meeting Recorder</h1>

      {warning && (
        <div className="warning-toast" onClick={() => setWarning(null)}>
          ⚠️ {warning}
        </div>
      )}

      <div className="status">
        <span className={`indicator ${isRecording ? "recording" : ""}`} />
        <span>{status}</span>
        {isRecording && elapsedTime > 0 && (
          <span className="elapsed-time">{formatDuration(elapsedTime)}</span>
        )}
      </div>

      {processingProgress.phase !== "idle" && (
        <div className="progress-container">
          <div className="progress-bar">
            <div className="progress-fill" style={{ width: `${processingProgress.percent}%` }} />
          </div>
          <span className="progress-text">
            {processingProgress.label} {processingProgress.percent.toFixed(0)}%
          </span>
        </div>
      )}

      <div className="controls">
        {!isRecording ? (
          <button onClick={startRecording} className="start-btn">
            Start Recording
          </button>
        ) : (
          <button onClick={stopRecording} className="stop-btn">
            Stop Recording
          </button>
        )}
      </div>

      {lastRecording && !transcription && (
        <div className="last-recording">
          <p>Saved to:</p>
          <code>{lastRecording.directory}</code>
          <button
            onClick={transcribeRecording}
            disabled={transcribing}
            className="primary-btn"
            style={{ marginTop: "12px" }}
          >
            {transcribing ? "Transcribing..." : "Transcribe"}
          </button>
        </div>
      )}

      {transcription && (
        <div className="transcription">
          <h3>Transcript ({formatTime(transcription.duration)})</h3>
          <div className="segments">
            {transcription.segments.map((seg) => (
              <div key={seg.id} className="segment">
                <span className={`speaker ${seg.speaker.toLowerCase()}`}>
                  {seg.speaker}:
                </span>
                <span className="timestamp">[{formatTime(seg.start_time)}]</span>
                <span className="text">{seg.text}</span>
              </div>
            ))}
          </div>

          {!summary && (
            <div style={{ display: "flex", gap: "8px", marginTop: "12px" }}>
              <button
                onClick={summarizeTranscript}
                disabled={summarizing}
                className="primary-btn"
              >
                {summarizing ? "Summarizing..." : "Generate Summary"}
              </button>
              <button onClick={openEditor} className="secondary-btn">
                Open Editor
              </button>
            </div>
          )}
        </div>
      )}

      {summary && (
        <div className="summary">
          <h3>Summary</h3>
          <p className="summary-text">{summary.summary}</p>

          {summary.key_points.length > 0 && (
            <>
              <h4>Key Points</h4>
              <ul className="key-points">
                {summary.key_points.map((point, i) => (
                  <li key={i}>{point}</li>
                ))}
              </ul>
            </>
          )}

          {summary.action_items.length > 0 && (
            <>
              <h4>Action Items</h4>
              <ul className="action-items">
                {summary.action_items.map((item, i) => (
                  <li key={i}>{item}</li>
                ))}
              </ul>
            </>
          )}

          <div style={{ display: "flex", gap: "8px", marginTop: "12px" }}>
            <button
              onClick={summarizeTranscript}
              disabled={summarizing}
              className="secondary-btn"
            >
              {summarizing ? "Regenerating..." : "Regenerate Summary"}
            </button>
            <button onClick={openEditor} className="secondary-btn">
              Open Editor
            </button>
          </div>
        </div>
      )}
    </main>
  );
}

function App() {
  const [windowLabel, setWindowLabel] = useState<string | null>(null);
  const [needsSetup, setNeedsSetup] = useState<boolean | null>(null);

  useEffect(() => {
    const win = getCurrentWebviewWindow();
    setWindowLabel(win.label);
    invoke<boolean>("check_setup_needed").then(setNeedsSetup);
  }, []);

  // Loading state
  if (windowLabel === null || needsSetup === null) {
    return (
      <main className="container">
        <p>Loading...</p>
      </main>
    );
  }

  // Editor window
  if (windowLabel === "editor") {
    return <EditorWindow />;
  }

  // Main window - setup wizard
  if (needsSetup) {
    return <SetupWizard onComplete={() => setNeedsSetup(false)} />;
  }

  // Main window - recorder
  return <RecorderUI />;
}

export default App;
