use crate::config::AppConfig;
use hound::WavReader;
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// A single transcription segment with timing and speaker
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub text: String,
    pub start_time: f32, // seconds
    pub end_time: f32,   // seconds
    pub speaker: String, // "Me" or "Meeting"
}

/// Full transcription result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TranscriptionResult {
    pub segments: Vec<TranscriptSegment>,
    pub full_text: String,
    pub duration: f32,
}

/// Progress during transcription
#[derive(Clone, serde::Serialize)]
pub struct TranscriptionProgress {
    pub phase: String,         // "system" or "mic"
    pub file_percent: i32,     // 0-100 for current file
    pub overall_percent: f32,  // 0-100 total
}

/// Load audio from WAV file and convert to f32 mono at 16kHz (whisper's expected format)
fn load_audio_for_whisper(audio_path: &Path) -> Result<Vec<f32>, String> {
    let reader = WavReader::open(audio_path)
        .map_err(|e| format!("Failed to open audio file: {}", e))?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    // Read all samples
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1 << (bits - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    // Convert stereo to mono if needed
    let mono_samples: Vec<f32> = if channels > 1 {
        samples
            .chunks(channels)
            .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        samples
    };

    // Resample to 16kHz if needed (whisper expects 16kHz)
    let target_rate = 16000;
    let resampled = if sample_rate != target_rate {
        resample(&mono_samples, sample_rate, target_rate)
    } else {
        mono_samples
    };

    Ok(resampled)
}

/// Simple linear interpolation resampling
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let new_len = ((samples.len() as f64) / ratio).ceil() as usize;
    let mut result = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_idx = i as f64 * ratio;
        let idx_floor = src_idx.floor() as usize;
        let idx_ceil = (idx_floor + 1).min(samples.len() - 1);
        let frac = (src_idx - idx_floor as f64) as f32;

        let sample = samples[idx_floor] * (1.0 - frac) + samples[idx_ceil] * frac;
        result.push(sample);
    }

    result
}

/// Transcribe a recording directory (system.wav + mic.wav) with speaker labels
pub fn transcribe_recording_dir(dir: &Path) -> Result<TranscriptionResult, String> {
    transcribe_recording_dir_with_progress(dir, None)
}

/// Transcribe a recording directory with progress reporting
pub fn transcribe_recording_dir_with_progress(
    dir: &Path,
    progress_tx: Option<std::sync::mpsc::Sender<TranscriptionProgress>>,
) -> Result<TranscriptionResult, String> {
    let system_file = dir.join("system.wav");
    let mic_file = dir.join("mic.wav");

    let config = AppConfig::load();
    let model_path = config
        .whisper_model_path()
        .ok_or("Whisper model not found. Please run setup first.")?;

    println!("Loading whisper model from: {:?}", model_path);

    // Initialize whisper context once (expensive operation)
    let ctx = WhisperContext::new_with_params(
        model_path.to_str().ok_or("Invalid model path")?,
        WhisperContextParameters::default(),
    )
    .map_err(|e| format!("Failed to load whisper model: {}", e))?;

    // Transcribe both sources
    let mut meeting_segments = if system_file.exists() {
        transcribe_file_with_context_and_progress(&ctx, &system_file, "Meeting", "system", &progress_tx)?
    } else {
        vec![]
    };

    let mut me_segments = if mic_file.exists() {
        transcribe_file_with_context_and_progress(&ctx, &mic_file, "Me", "mic", &progress_tx)?
    } else {
        vec![]
    };

    // Merge segments chronologically
    let (segments, full_text, duration) = merge_segments(&mut meeting_segments, &mut me_segments);

    println!("Transcription complete: {} segments total", segments.len());

    Ok(TranscriptionResult {
        segments,
        full_text,
        duration,
    })
}

/// Transcribe a single audio file with a shared whisper context
fn transcribe_file_with_context(
    ctx: &WhisperContext,
    audio_path: &Path,
    speaker: &str,
) -> Result<Vec<TranscriptSegment>, String> {
    transcribe_file_with_context_and_progress(ctx, audio_path, speaker, speaker, &None)
}

/// Transcribe a single audio file with progress reporting
fn transcribe_file_with_context_and_progress(
    ctx: &WhisperContext,
    audio_path: &Path,
    speaker: &str,
    phase: &str,
    progress_tx: &Option<std::sync::mpsc::Sender<TranscriptionProgress>>,
) -> Result<Vec<TranscriptSegment>, String> {
    // Load audio
    let audio_data = load_audio_for_whisper(audio_path)?;

    if audio_data.is_empty() {
        return Ok(vec![]);
    }

    println!(
        "Transcribing {} audio: {} samples ({:.1}s)",
        speaker,
        audio_data.len(),
        audio_data.len() as f32 / 16000.0
    );

    // Configure transcription parameters
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_token_timestamps(true);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // Set up progress callback if we have a channel
    if let Some(tx) = progress_tx {
        let tx = tx.clone();
        let phase_str = phase.to_string();
        params.set_progress_callback_safe(move |progress| {
            // system = 0-50%, mic = 50-100%
            let overall = if phase_str == "system" {
                progress as f32 / 2.0
            } else {
                50.0 + progress as f32 / 2.0
            };
            let _ = tx.send(TranscriptionProgress {
                phase: phase_str.clone(),
                file_percent: progress,
                overall_percent: overall,
            });
        });
    }

    // Create state and run inference
    let mut state = ctx
        .create_state()
        .map_err(|e| format!("Failed to create whisper state: {}", e))?;

    state
        .full(params, &audio_data)
        .map_err(|e| format!("Transcription failed for {}: {}", speaker, e))?;

    // Extract segments
    let num_segments = state.full_n_segments().map_err(|e| format!("{}", e))?;
    let mut segments = Vec::with_capacity(num_segments as usize);

    for i in 0..num_segments {
        let text = state
            .full_get_segment_text(i)
            .map_err(|e| format!("Failed to get segment {}: {}", i, e))?;

        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }

        let start = state
            .full_get_segment_t0(i)
            .map_err(|e| format!("Failed to get segment start: {}", e))?;

        let end = state
            .full_get_segment_t1(i)
            .map_err(|e| format!("Failed to get segment end: {}", e))?;

        // Convert from centiseconds to seconds
        let start_sec = start as f32 / 100.0;
        let end_sec = end as f32 / 100.0;

        segments.push(TranscriptSegment {
            id: String::new(), // Will be assigned during merge
            text: trimmed.to_string(),
            start_time: start_sec,
            end_time: end_sec,
            speaker: speaker.to_string(),
        });
    }

    println!("{} transcription: {} segments", speaker, segments.len());
    Ok(segments)
}

/// Merge segments from two sources chronologically by start_time
fn merge_segments(
    meeting: &mut Vec<TranscriptSegment>,
    me: &mut Vec<TranscriptSegment>,
) -> (Vec<TranscriptSegment>, String, f32) {
    // Combine all segments
    let mut all_segments: Vec<TranscriptSegment> = Vec::new();
    all_segments.append(meeting);
    all_segments.append(me);

    // Sort by start time
    all_segments.sort_by(|a, b| a.start_time.partial_cmp(&b.start_time).unwrap());

    // Assign sequential IDs and build full text
    let mut full_text = String::new();
    let mut max_end_time: f32 = 0.0;

    for (i, seg) in all_segments.iter_mut().enumerate() {
        seg.id = format!("seg_{}", i);

        if !full_text.is_empty() {
            full_text.push(' ');
        }
        full_text.push_str(&seg.text);

        if seg.end_time > max_end_time {
            max_end_time = seg.end_time;
        }
    }

    (all_segments, full_text, max_end_time)
}

/// Legacy function for single-file transcription (kept for compatibility)
pub fn transcribe_audio(audio_path: &Path) -> Result<TranscriptionResult, String> {
    let config = AppConfig::load();

    let model_path = config
        .whisper_model_path()
        .ok_or("Whisper model not found. Please run setup first.")?;

    println!("Loading whisper model from: {:?}", model_path);

    // Load audio
    let audio_data = load_audio_for_whisper(audio_path)?;
    let duration = audio_data.len() as f32 / 16000.0;

    println!(
        "Loaded audio: {} samples ({:.1}s)",
        audio_data.len(),
        duration
    );

    // Initialize whisper context
    let ctx = WhisperContext::new_with_params(
        model_path.to_str().ok_or("Invalid model path")?,
        WhisperContextParameters::default(),
    )
    .map_err(|e| format!("Failed to load whisper model: {}", e))?;

    // Configure transcription parameters
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    // Set language to English (auto-detect if multilingual model)
    params.set_language(Some("en"));

    // Enable token timestamps for segment timing
    params.set_token_timestamps(true);

    // Don't print progress to stdout
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // Create state and run inference
    let mut state = ctx
        .create_state()
        .map_err(|e| format!("Failed to create whisper state: {}", e))?;

    state
        .full(params, &audio_data)
        .map_err(|e| format!("Transcription failed: {}", e))?;

    // Extract segments
    let num_segments = state.full_n_segments().map_err(|e| format!("{}", e))?;
    let mut segments = Vec::with_capacity(num_segments as usize);
    let mut full_text = String::new();

    for i in 0..num_segments {
        let text = state
            .full_get_segment_text(i)
            .map_err(|e| format!("Failed to get segment {}: {}", i, e))?;

        let start = state
            .full_get_segment_t0(i)
            .map_err(|e| format!("Failed to get segment start: {}", e))?;

        let end = state
            .full_get_segment_t1(i)
            .map_err(|e| format!("Failed to get segment end: {}", e))?;

        // Convert from centiseconds to seconds
        let start_sec = start as f32 / 100.0;
        let end_sec = end as f32 / 100.0;

        if !full_text.is_empty() {
            full_text.push(' ');
        }
        full_text.push_str(text.trim());

        segments.push(TranscriptSegment {
            id: format!("seg_{}", i),
            text: text.trim().to_string(),
            start_time: start_sec,
            end_time: end_sec,
            speaker: "Unknown".to_string(),
        });
    }

    println!("Transcription complete: {} segments", segments.len());

    Ok(TranscriptionResult {
        segments,
        full_text,
        duration,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_segment(id: &str, text: &str, start: f32, end: f32, speaker: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: id.to_string(),
            text: text.to_string(),
            start_time: start,
            end_time: end,
            speaker: speaker.to_string(),
        }
    }

    #[test]
    fn test_merge_segments_empty() {
        let mut meeting: Vec<TranscriptSegment> = vec![];
        let mut me: Vec<TranscriptSegment> = vec![];
        let (segments, full_text, duration) = merge_segments(&mut meeting, &mut me);

        assert!(segments.is_empty());
        assert!(full_text.is_empty());
        assert_eq!(duration, 0.0);
    }

    #[test]
    fn test_merge_segments_single_source() {
        let mut meeting = vec![
            make_segment("", "Hello", 0.0, 1.0, "Meeting"),
            make_segment("", "World", 2.0, 3.0, "Meeting"),
        ];
        let mut me: Vec<TranscriptSegment> = vec![];
        let (segments, full_text, duration) = merge_segments(&mut meeting, &mut me);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].id, "seg_0");
        assert_eq!(segments[1].id, "seg_1");
        assert_eq!(full_text, "Hello World");
        assert_eq!(duration, 3.0);
    }

    #[test]
    fn test_merge_segments_chronological() {
        let mut meeting = vec![
            make_segment("", "First", 0.0, 1.0, "Meeting"),
            make_segment("", "Third", 4.0, 5.0, "Meeting"),
        ];
        let mut me = vec![
            make_segment("", "Second", 2.0, 3.0, "Me"),
            make_segment("", "Fourth", 6.0, 7.0, "Me"),
        ];
        let (segments, full_text, _) = merge_segments(&mut meeting, &mut me);

        assert_eq!(segments.len(), 4);
        // Verify chronological order
        assert_eq!(segments[0].text, "First");
        assert_eq!(segments[0].speaker, "Meeting");
        assert_eq!(segments[1].text, "Second");
        assert_eq!(segments[1].speaker, "Me");
        assert_eq!(segments[2].text, "Third");
        assert_eq!(segments[3].text, "Fourth");

        // IDs should be reassigned sequentially
        assert_eq!(segments[0].id, "seg_0");
        assert_eq!(segments[1].id, "seg_1");
        assert_eq!(segments[2].id, "seg_2");
        assert_eq!(segments[3].id, "seg_3");

        assert_eq!(full_text, "First Second Third Fourth");
    }

    #[test]
    fn test_merge_segments_overlapping() {
        // When segments overlap, they should still sort by start_time
        let mut meeting = vec![make_segment("", "Meeting overlap", 1.0, 3.0, "Meeting")];
        let mut me = vec![make_segment("", "Me overlap", 1.5, 2.5, "Me")];
        let (segments, _, _) = merge_segments(&mut meeting, &mut me);

        assert_eq!(segments.len(), 2);
        // Meeting starts first (1.0) so it comes first
        assert_eq!(segments[0].speaker, "Meeting");
        assert_eq!(segments[1].speaker, "Me");
    }

    #[test]
    fn test_resample_identity() {
        let input = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let output = resample(&input, 48000, 48000);
        assert_eq!(input, output);
    }

    #[test]
    fn test_resample_empty() {
        let input: Vec<f32> = vec![];
        let output = resample(&input, 48000, 16000);
        assert!(output.is_empty());
    }
}
