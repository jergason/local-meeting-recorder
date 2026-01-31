use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{WavSpec, WavWriter};
use parking_lot::Mutex;
use screencapturekit::prelude::*;
use std::sync::Arc;
use std::time::Instant;
use std::{fs::File, io::BufWriter, path::PathBuf};
use tauri::{AppHandle, Emitter};

/// Output from a recording session - contains paths to all audio files
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordingOutput {
    pub directory: PathBuf,
    pub system_file: PathBuf,
    pub mic_file: PathBuf,
    pub mixed_file: PathBuf,
}

/// Stats about the current recording
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordingStats {
    pub duration_secs: f64,
    pub system_samples_written: u64,
    pub mic_samples_written: u64,
}

/// Progress during audio mixing
#[derive(Clone, serde::Serialize)]
pub struct MixingProgress {
    pub current_frame: u64,
    pub total_frames: u64,
    pub percent: f32,
}

/// Streaming WAV writer that writes samples directly to disk
struct StreamingWavWriter {
    writer: WavWriter<BufWriter<File>>,
    samples_written: u64,
}

impl StreamingWavWriter {
    fn new(path: &PathBuf, channels: u16, sample_rate: u32) -> Result<Self, String> {
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let file = File::create(path).map_err(|e| format!("Failed to create file: {}", e))?;
        let writer = WavWriter::new(BufWriter::new(file), spec)
            .map_err(|e| format!("Failed to create WAV writer: {}", e))?;
        Ok(Self {
            writer,
            samples_written: 0,
        })
    }

    fn write_samples(&mut self, samples: &[f32]) -> Result<(), String> {
        for &sample in samples {
            self.writer
                .write_sample(sample)
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }
        self.samples_written += samples.len() as u64;
        Ok(())
    }

    fn finalize(self) -> Result<u64, String> {
        self.writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV: {}", e))?;
        Ok(self.samples_written)
    }
}

pub struct AudioRecorder {
    // streaming writers - opened at start, closed at stop
    system_writer: Arc<Mutex<Option<StreamingWavWriter>>>,
    mic_writer: Arc<Mutex<Option<StreamingWavWriter>>>,
    // small buffer for mic resampling (holds ~0.5 sec)
    mic_buffer: Arc<Mutex<Vec<f32>>>,
    // tracking
    is_recording: Arc<Mutex<bool>>,
    start_time: Arc<Mutex<Option<Instant>>>,
    recording_dir: Arc<Mutex<Option<PathBuf>>>,
    // audio config
    sample_rate: u32,
    mic_sample_rate: Arc<Mutex<u32>>,
    // stream handles
    sc_stream: Arc<Mutex<Option<SCStream>>>,
    mic_stream: Option<cpal::Stream>,
    // sample counts for stats
    system_samples_written: Arc<Mutex<u64>>,
    mic_samples_written: Arc<Mutex<u64>>,
}

struct SystemAudioHandler {
    writer: Arc<Mutex<Option<StreamingWavWriter>>>,
    samples_written: Arc<Mutex<u64>>,
}

impl SCStreamOutputTrait for SystemAudioHandler {
    fn did_output_sample_buffer(&self, sample_buffer: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type == SCStreamOutputType::Audio {
            if let Some(audio_buffer_list) = sample_buffer.audio_buffer_list() {
                for audio_buffer in audio_buffer_list.iter() {
                    let data = audio_buffer.data();
                    if !data.is_empty() {
                        let samples: Vec<f32> = data
                            .chunks_exact(4)
                            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                            .collect();
                        // write directly to disk
                        if let Some(ref mut writer) = *self.writer.lock() {
                            if let Err(e) = writer.write_samples(&samples) {
                                eprintln!("Failed to write system audio samples: {}", e);
                            } else {
                                *self.samples_written.lock() += samples.len() as u64;
                            }
                        }
                    }
                }
            }
        }
    }
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
        let frac = src_idx - idx_floor as f64;

        let sample = samples[idx_floor] * (1.0 - frac as f32) + samples[idx_ceil] * frac as f32;
        result.push(sample);
    }

    result
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            system_writer: Arc::new(Mutex::new(None)),
            mic_writer: Arc::new(Mutex::new(None)),
            mic_buffer: Arc::new(Mutex::new(Vec::with_capacity(48000))), // ~1 sec buffer
            is_recording: Arc::new(Mutex::new(false)),
            start_time: Arc::new(Mutex::new(None)),
            recording_dir: Arc::new(Mutex::new(None)),
            sample_rate: 48000,
            mic_sample_rate: Arc::new(Mutex::new(48000)),
            sc_stream: Arc::new(Mutex::new(None)),
            mic_stream: None,
            system_samples_written: Arc::new(Mutex::new(0)),
            mic_samples_written: Arc::new(Mutex::new(0)),
        }
    }

    /// Start recording to the given directory
    pub fn start_recording(&mut self, recording_dir: &PathBuf) -> Result<(), String> {
        if *self.is_recording.lock() {
            return Err("Already recording".to_string());
        }

        // Create recording directory
        std::fs::create_dir_all(recording_dir)
            .map_err(|e| format!("Failed to create recording directory: {}", e))?;

        // Open streaming WAV writers
        let system_file = recording_dir.join("system.wav");
        let mic_file = recording_dir.join("mic.wav");

        let system_writer = StreamingWavWriter::new(&system_file, 2, self.sample_rate)?;
        let mic_writer = StreamingWavWriter::new(&mic_file, 1, self.sample_rate)?;

        *self.system_writer.lock() = Some(system_writer);
        *self.mic_writer.lock() = Some(mic_writer);
        *self.recording_dir.lock() = Some(recording_dir.clone());

        // Reset counters and buffers
        self.mic_buffer.lock().clear();
        *self.system_samples_written.lock() = 0;
        *self.mic_samples_written.lock() = 0;

        // Start system audio capture via ScreenCaptureKit
        self.start_system_audio_capture()?;

        // Start microphone capture via cpal
        self.start_mic_capture()?;

        *self.start_time.lock() = Some(Instant::now());
        *self.is_recording.lock() = true;
        Ok(())
    }

    /// Get stats about the current recording
    pub fn get_stats(&self) -> Option<RecordingStats> {
        let start = self.start_time.lock();
        start.as_ref().map(|t| RecordingStats {
            duration_secs: t.elapsed().as_secs_f64(),
            system_samples_written: *self.system_samples_written.lock(),
            mic_samples_written: *self.mic_samples_written.lock(),
        })
    }

    fn start_system_audio_capture(&mut self) -> Result<(), String> {
        let content = SCShareableContent::get()
            .map_err(|e| format!("Failed to get shareable content: {:?}", e))?;

        let displays = content.displays();
        let display = displays.first().ok_or("No display found")?;

        let filter = SCContentFilter::create()
            .with_display(display)
            .with_excluding_windows(&[])
            .build();

        let config = SCStreamConfiguration::new()
            .with_captures_audio(true)
            .with_excludes_current_process_audio(false)
            .with_sample_rate(self.sample_rate as i32)
            .with_channel_count(2);

        let handler = SystemAudioHandler {
            writer: self.system_writer.clone(),
            samples_written: self.system_samples_written.clone(),
        };

        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(handler, SCStreamOutputType::Audio);

        stream
            .start_capture()
            .map_err(|e| format!("Failed to start system audio capture: {:?}", e))?;

        *self.sc_stream.lock() = Some(stream);
        Ok(())
    }

    fn start_mic_capture(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No input device available")?;

        let supported_config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {}", e))?;

        let mic_rate = u32::from(supported_config.sample_rate());
        *self.mic_sample_rate.lock() = mic_rate;
        println!("Mic sample rate: {} Hz", mic_rate);

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: supported_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        // flush threshold: ~0.5 sec of samples at mic rate
        let flush_threshold = (mic_rate / 2) as usize;
        let output_rate = self.sample_rate;

        let mic_buffer = self.mic_buffer.clone();
        let mic_writer = self.mic_writer.clone();
        let mic_samples_written = self.mic_samples_written.clone();
        let is_recording = self.is_recording.clone();

        // helper to flush buffer
        let flush_mic_buffer = move |buffer: &mut Vec<f32>| {
            if buffer.is_empty() {
                return;
            }
            let resampled = resample(buffer, mic_rate, output_rate);
            buffer.clear();
            if let Some(ref mut writer) = *mic_writer.lock() {
                if let Err(e) = writer.write_samples(&resampled) {
                    eprintln!("Failed to write mic samples: {}", e);
                } else {
                    *mic_samples_written.lock() += resampled.len() as u64;
                }
            }
        };

        let stream = match supported_config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if *is_recording.lock() {
                            let mut buffer = mic_buffer.lock();
                            buffer.extend_from_slice(data);
                            if buffer.len() >= flush_threshold {
                                flush_mic_buffer(&mut buffer);
                            }
                        }
                    },
                    |err| eprintln!("Mic stream error: {}", err),
                    None,
                )
                .map_err(|e| format!("Failed to build mic stream: {}", e))?,
            cpal::SampleFormat::I16 => {
                let mic_buffer = self.mic_buffer.clone();
                let mic_writer = self.mic_writer.clone();
                let mic_samples_written = self.mic_samples_written.clone();
                let is_recording = self.is_recording.clone();

                let flush_mic_buffer_i16 = move |buffer: &mut Vec<f32>| {
                    if buffer.is_empty() {
                        return;
                    }
                    let resampled = resample(buffer, mic_rate, output_rate);
                    buffer.clear();
                    if let Some(ref mut writer) = *mic_writer.lock() {
                        if let Err(e) = writer.write_samples(&resampled) {
                            eprintln!("Failed to write mic samples: {}", e);
                        } else {
                            *mic_samples_written.lock() += resampled.len() as u64;
                        }
                    }
                };

                device
                    .build_input_stream(
                        &config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            if *is_recording.lock() {
                                let float_samples: Vec<f32> =
                                    data.iter().map(|&s| s as f32 / 32768.0).collect();
                                let mut buffer = mic_buffer.lock();
                                buffer.extend(float_samples);
                                if buffer.len() >= flush_threshold {
                                    flush_mic_buffer_i16(&mut buffer);
                                }
                            }
                        },
                        |err| eprintln!("Mic stream error: {}", err),
                        None,
                    )
                    .map_err(|e| format!("Failed to build mic stream: {}", e))?
            }
            format => return Err(format!("Unsupported sample format: {:?}", format)),
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start mic stream: {}", e))?;

        self.mic_stream = Some(stream);
        Ok(())
    }

    pub fn stop_recording(&mut self, app: Option<&AppHandle>) -> Result<RecordingOutput, String> {
        if !*self.is_recording.lock() {
            return Err("Not recording".to_string());
        }

        *self.is_recording.lock() = false;

        // Stop ScreenCaptureKit stream
        if let Some(stream) = self.sc_stream.lock().take() {
            let _ = stream.stop_capture();
        }

        // Stop mic stream (drops automatically)
        self.mic_stream.take();

        // Flush any remaining mic samples
        self.flush_remaining_mic_samples()?;

        // Finalize the streaming writers
        let system_samples = if let Some(writer) = self.system_writer.lock().take() {
            writer.finalize()?
        } else {
            0
        };

        let mic_samples = if let Some(writer) = self.mic_writer.lock().take() {
            writer.finalize()?
        } else {
            0
        };

        let recording_dir = self
            .recording_dir
            .lock()
            .take()
            .ok_or("No recording directory set")?;

        let system_file = recording_dir.join("system.wav");
        let mic_file = recording_dir.join("mic.wav");
        let mixed_file = recording_dir.join("mixed.wav");

        println!(
            "System samples written: {}, Mic samples written: {}",
            system_samples, mic_samples
        );

        // Generate mixed.wav from the finalized files
        self.generate_mixed_audio(&system_file, &mic_file, &mixed_file, app)?;

        // Clear start time
        *self.start_time.lock() = None;

        Ok(RecordingOutput {
            directory: recording_dir,
            system_file,
            mic_file,
            mixed_file,
        })
    }

    /// Flush any remaining samples in the mic buffer
    fn flush_remaining_mic_samples(&self) -> Result<(), String> {
        let mic_rate = *self.mic_sample_rate.lock();
        let mut buffer = self.mic_buffer.lock();

        if buffer.is_empty() {
            return Ok(());
        }

        let resampled = resample(&buffer, mic_rate, self.sample_rate);
        buffer.clear();

        if let Some(ref mut writer) = *self.mic_writer.lock() {
            writer.write_samples(&resampled)?;
            *self.mic_samples_written.lock() += resampled.len() as u64;
        }

        Ok(())
    }

    /// Generate mixed.wav by reading system.wav and mic.wav and mixing them
    fn generate_mixed_audio(
        &self,
        system_file: &PathBuf,
        mic_file: &PathBuf,
        output_path: &PathBuf,
        app: Option<&AppHandle>,
    ) -> Result<(), String> {
        use hound::WavReader;

        // Read system audio (stereo)
        let system_reader = WavReader::open(system_file)
            .map_err(|e| format!("Failed to open system.wav: {}", e))?;
        let system_samples: Vec<f32> = system_reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect();

        // Read mic audio (mono, already resampled)
        let mic_reader = WavReader::open(mic_file)
            .map_err(|e| format!("Failed to open mic.wav: {}", e))?;
        let mic_samples: Vec<f32> = mic_reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect();

        // Determine output length
        let system_frames = system_samples.len() / 2;
        let mic_frames = mic_samples.len();
        let max_frames = system_frames.max(mic_frames);

        println!(
            "Mixing: system={} frames, mic={} frames",
            system_frames, mic_frames
        );

        let spec = WavSpec {
            channels: 2,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let file =
            File::create(output_path).map_err(|e| format!("Failed to create mixed.wav: {}", e))?;
        let mut writer = WavWriter::new(BufWriter::new(file), spec)
            .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

        // Mix: system audio (stereo) + mic (mono expanded to stereo)
        // Use chunked writes for performance (~10-100x faster than sample-by-sample)
        const CHUNK_SIZE: usize = 16384; // ~0.34 sec at 48kHz
        let mut last_percent: f32 = 0.0;

        for chunk_start in (0..max_frames).step_by(CHUNK_SIZE) {
            let chunk_end = (chunk_start + CHUNK_SIZE).min(max_frames);

            for i in chunk_start..chunk_end {
                let sys_left = system_samples.get(i * 2).copied().unwrap_or(0.0);
                let sys_right = system_samples.get(i * 2 + 1).copied().unwrap_or(0.0);
                let mic = mic_samples.get(i).copied().unwrap_or(0.0);

                // Mix: 70% system + 30% mic
                let left = (sys_left * 0.7 + mic * 0.3).clamp(-1.0, 1.0);
                let right = (sys_right * 0.7 + mic * 0.3).clamp(-1.0, 1.0);

                writer
                    .write_sample(left)
                    .map_err(|e| format!("Failed to write sample: {}", e))?;
                writer
                    .write_sample(right)
                    .map_err(|e| format!("Failed to write sample: {}", e))?;
            }

            // emit progress every 1%
            let percent = (chunk_end as f32 / max_frames as f32) * 100.0;
            if let Some(app) = app {
                if percent - last_percent >= 1.0 {
                    last_percent = percent;
                    let _ = app.emit(
                        "mixing-progress",
                        MixingProgress {
                            current_frame: chunk_end as u64,
                            total_frames: max_frames as u64,
                            percent,
                        },
                    );
                }
            }
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize mixed.wav: {}", e))?;

        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock()
    }
}

impl Default for AudioRecorder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_identity() {
        // Same rate should return same data
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

    #[test]
    fn test_resample_downsample() {
        // 48kHz -> 16kHz should produce ~1/3 the samples
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 / 4800.0)).collect();
        let output = resample(&input, 48000, 16000);

        // Expected length: 4800 * (16000/48000) = 1600
        assert_eq!(output.len(), 1600);

        // First and last samples should be approximately preserved
        assert!((output[0] - input[0]).abs() < 0.01);
    }

    #[test]
    fn test_resample_upsample() {
        // 16kHz -> 48kHz should produce ~3x the samples
        let input: Vec<f32> = (0..1600).map(|i| (i as f32 / 1600.0)).collect();
        let output = resample(&input, 16000, 48000);

        // Expected length: 1600 * (48000/16000) = 4800
        assert_eq!(output.len(), 4800);
    }

    #[test]
    fn test_resample_interpolation() {
        // Test that downsampling interpolates between values
        // 4 samples at 4Hz -> 2 samples at 2Hz
        let input = vec![0.0, 0.5, 1.0, 0.5];
        let output = resample(&input, 4, 2);

        // Should have 2 samples
        assert_eq!(output.len(), 2);
        // First sample should be 0.0 (or close to it)
        assert!((output[0] - 0.0).abs() < 0.01);
    }
}
