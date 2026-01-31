import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./EditorWindow.css";

interface TranscriptSegment {
  id: string;
  text: string;
  start_time: number;
  end_time: number;
  speaker: string;
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

interface EditorPayload {
  recording_dir: string;
  transcript: TranscriptionResult;
  summary: SummaryResult | null;
}

function formatTime(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

interface SegmentRowProps {
  segment: TranscriptSegment;
  speakers: string[];
  onTextChange: (id: string, text: string) => void;
  onSpeakerChange: (id: string, speaker: string) => void;
  onDelete: (id: string) => void;
}

function SegmentRow({ segment, speakers, onTextChange, onSpeakerChange, onDelete }: SegmentRowProps) {
  return (
    <div className="segment-row">
      <div className="segment-meta">
        <select
          className="speaker-select"
          value={segment.speaker}
          onChange={(e) => onSpeakerChange(segment.id, e.target.value)}
        >
          {speakers.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
        <span className="segment-time">{formatTime(segment.start_time)}</span>
        <button className="delete-btn" onClick={() => onDelete(segment.id)} title="Delete segment">
          ×
        </button>
      </div>
      <textarea
        className="segment-text"
        value={segment.text}
        onChange={(e) => onTextChange(segment.id, e.target.value)}
        rows={2}
      />
    </div>
  );
}

export default function EditorWindow() {
  const [recordingDir, setRecordingDir] = useState<string>("");
  const [segments, setSegments] = useState<TranscriptSegment[]>([]);
  const [duration, setDuration] = useState<number>(0);
  const [summary, setSummary] = useState<SummaryResult | null>(null);
  const [summarizing, setSummarizing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [hasChanges, setHasChanges] = useState(false);
  const [status, setStatus] = useState("Ready");

  // unique speakers from segments
  const speakers = [...new Set(segments.map((s) => s.speaker))];

  useEffect(() => {
    const unlisten = listen<EditorPayload>("editor-data", (event) => {
      const { recording_dir, transcript, summary } = event.payload;
      setRecordingDir(recording_dir);
      setSegments(transcript.segments);
      setDuration(transcript.duration);
      setSummary(summary);
      setHasChanges(false);
      setStatus("Loaded");
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleTextChange = useCallback((id: string, text: string) => {
    setSegments((prev) =>
      prev.map((seg) => (seg.id === id ? { ...seg, text } : seg))
    );
    setHasChanges(true);
  }, []);

  const handleSpeakerChange = useCallback((id: string, speaker: string) => {
    setSegments((prev) =>
      prev.map((seg) => (seg.id === id ? { ...seg, speaker } : seg))
    );
    setHasChanges(true);
  }, []);

  const handleDelete = useCallback((id: string) => {
    setSegments((prev) => prev.filter((seg) => seg.id !== id));
    setHasChanges(true);
  }, []);

  const buildTranscript = useCallback((): TranscriptionResult => {
    const fullText = segments.map((s) => `[${s.speaker}] ${s.text}`).join("\n");
    return {
      segments,
      full_text: fullText,
      duration,
    };
  }, [segments, duration]);

  const handleSave = async () => {
    if (!recordingDir) return;
    try {
      setSaving(true);
      setStatus("Saving...");
      await invoke("save_edited_transcript", {
        recordingDir,
        transcript: buildTranscript(),
      });
      setHasChanges(false);
      setStatus("Saved");
    } catch (e) {
      setStatus(`Save error: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  const handleRegenerate = async () => {
    try {
      setSummarizing(true);
      setStatus("Regenerating summary...");
      const result = await invoke<SummaryResult>("summarize_transcript", {
        transcript: buildTranscript(),
      });
      setSummary(result);
      setStatus("Summary updated");
    } catch (e) {
      setStatus(`Summarization error: ${e}`);
    } finally {
      setSummarizing(false);
    }
  };

  if (segments.length === 0) {
    return (
      <div className="editor-window">
        <div className="editor-empty">
          <p>No transcript loaded.</p>
          <p className="hint">Open a recording from the main window to edit it here.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="editor-window">
      <header className="editor-header">
        <div className="header-left">
          <h1>Transcript Editor</h1>
          <span className="duration">({formatTime(duration)})</span>
        </div>
        <div className="header-right">
          <span className="status">{status}</span>
          <button
            className="save-btn"
            onClick={handleSave}
            disabled={saving || !hasChanges}
          >
            {saving ? "Saving..." : hasChanges ? "Save" : "Saved"}
          </button>
        </div>
      </header>

      <div className="editor-content">
        <div className="transcript-panel">
          <h2>Transcript</h2>
          <div className="segments-list">
            {segments.map((seg) => (
              <SegmentRow
                key={seg.id}
                segment={seg}
                speakers={speakers}
                onTextChange={handleTextChange}
                onSpeakerChange={handleSpeakerChange}
                onDelete={handleDelete}
              />
            ))}
          </div>
        </div>

        <div className="summary-panel">
          <div className="summary-header">
            <h2>Summary</h2>
            <button
              className="regenerate-btn"
              onClick={handleRegenerate}
              disabled={summarizing}
            >
              {summarizing ? "Generating..." : "Regenerate"}
            </button>
          </div>

          {summary ? (
            <div className="summary-content">
              <section>
                <h3>Overview</h3>
                <p>{summary.summary}</p>
              </section>

              {summary.key_points.length > 0 && (
                <section>
                  <h3>Key Points</h3>
                  <ul>
                    {summary.key_points.map((point, i) => (
                      <li key={i}>{point}</li>
                    ))}
                  </ul>
                </section>
              )}

              {summary.action_items.length > 0 && (
                <section>
                  <h3>Action Items</h3>
                  <ul className="action-items">
                    {summary.action_items.map((item, i) => (
                      <li key={i}>{item}</li>
                    ))}
                  </ul>
                </section>
              )}
            </div>
          ) : (
            <div className="no-summary">
              <p>No summary yet.</p>
              <button
                className="primary-btn"
                onClick={handleRegenerate}
                disabled={summarizing}
              >
                {summarizing ? "Generating..." : "Generate Summary"}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
