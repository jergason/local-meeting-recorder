mod audio;
mod config;
mod download;
mod summarize;
mod transcribe;

use audio::{AudioRecorder, RecordingOutput, RecordingStats};
use config::{AppConfig, ModelInfo};
use summarize::SummaryResult;
use transcribe::TranscriptionResult;
use parking_lot::Mutex;
use std::path::PathBuf;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_positioner::{Position, WindowExt};

struct AppState {
    recorder: Mutex<AudioRecorder>,
    recordings_dir: PathBuf,
    config: Mutex<AppConfig>,
}

// === Recording Commands ===

#[tauri::command]
fn start_recording(state: State<AppState>) -> Result<(), String> {
    // Generate timestamp for directory name
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let recording_dir = state.recordings_dir.join(&timestamp);

    // Ensure base recordings directory exists
    std::fs::create_dir_all(&state.recordings_dir)
        .map_err(|e| format!("Failed to create recordings directory: {}", e))?;

    let mut recorder = state.recorder.lock();
    recorder.start_recording(&recording_dir)
}

#[tauri::command]
fn stop_recording(app: AppHandle, state: State<AppState>) -> Result<RecordingOutput, String> {
    let mut recorder = state.recorder.lock();
    recorder.stop_recording(Some(&app))
}

#[tauri::command]
fn is_recording(state: State<AppState>) -> bool {
    state.recorder.lock().is_recording()
}

#[tauri::command]
fn get_recording_stats(state: State<AppState>) -> Option<RecordingStats> {
    state.recorder.lock().get_stats()
}

// === Setup/Config Commands ===

#[tauri::command]
fn check_setup_needed(state: State<AppState>) -> bool {
    state.config.lock().needs_setup()
}

#[tauri::command]
fn get_whisper_models() -> Vec<ModelInfo> {
    ModelInfo::whisper_models()
}

#[tauri::command]
fn get_llm_models() -> Vec<ModelInfo> {
    ModelInfo::llm_models()
}

#[tauri::command]
async fn download_whisper_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<String, String> {
    let model = ModelInfo::whisper_models()
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Unknown whisper model: {}", model_id))?;

    let path = download::download_model(&app, &model).await?;

    // Update config
    {
        let mut config = state.config.lock();
        config.whisper_model = Some(model.filename.clone());
        config.save()?;
    }

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn download_llm_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<String, String> {
    let model = ModelInfo::llm_models()
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Unknown LLM model: {}", model_id))?;

    let path = download::download_model(&app, &model).await?;

    // Update config
    {
        let mut config = state.config.lock();
        config.llm_model = Some(model.filename.clone());
        config.save()?;
    }

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn complete_setup(state: State<AppState>) -> Result<(), String> {
    let mut config = state.config.lock();
    config.setup_complete = true;
    config.save()
}

#[tauri::command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.config.lock().clone()
}

// === Transcription Commands ===

#[tauri::command]
async fn transcribe_recording(app: AppHandle, recording_dir: String) -> Result<TranscriptionResult, String> {
    let (tx, rx) = std::sync::mpsc::channel::<transcribe::TranscriptionProgress>();

    // spawn thread to forward progress to frontend
    let app_clone = app.clone();
    std::thread::spawn(move || {
        while let Ok(progress) = rx.recv() {
            let _ = app_clone.emit("transcription-progress", progress);
        }
    });

    tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&recording_dir);
        transcribe::transcribe_recording_dir_with_progress(path, Some(tx))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// === Summarization Commands ===

#[tauri::command]
async fn summarize_transcript(transcript: TranscriptionResult) -> Result<SummaryResult, String> {
    summarize::summarize_transcript(&transcript).await
}

// === Editor Window Commands ===

#[derive(Clone, serde::Serialize)]
struct EditorPayload {
    recording_dir: String,
    transcript: TranscriptionResult,
    summary: Option<SummaryResult>,
}

#[tauri::command]
async fn open_editor(
    app: AppHandle,
    recording_dir: String,
    transcript: TranscriptionResult,
    summary: Option<SummaryResult>,
) -> Result<(), String> {
    // Check if editor window already exists
    if let Some(window) = app.get_webview_window("editor") {
        // Window exists, just show it and send new data
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;

        // Emit the data to the existing window
        window
            .emit("editor-data", EditorPayload {
                recording_dir,
                transcript,
                summary,
            })
            .map_err(|e| e.to_string())?;
    } else {
        // Create new editor window
        let editor = WebviewWindowBuilder::new(&app, "editor", WebviewUrl::App("index.html".into()))
            .title("Transcript Editor")
            .inner_size(900.0, 700.0)
            .min_inner_size(600.0, 400.0)
            .center()
            .build()
            .map_err(|e| format!("Failed to create editor window: {}", e))?;

        // Give it a moment to initialize, then send data
        let payload = EditorPayload {
            recording_dir,
            transcript,
            summary,
        };

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        editor
            .emit("editor-data", payload)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
async fn save_edited_transcript(
    recording_dir: String,
    transcript: TranscriptionResult,
) -> Result<(), String> {
    let path = std::path::Path::new(&recording_dir).join("transcript_edited.json");
    let json = serde_json::to_string_pretty(&transcript)
        .map_err(|e| format!("Failed to serialize transcript: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write transcript: {}", e))?;
    Ok(())
}

fn update_tray_menu(_app: &AppHandle, is_recording: bool) {
    // We'll update menu item enabled states based on recording status
    // For now, just log the state
    println!("Recording state: {}", is_recording);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState {
            recorder: Mutex::new(AudioRecorder::new()),
            recordings_dir: dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("MeetingRecordings"),
            config: Mutex::new(AppConfig::load()),
        })
        .setup(|app| {
            // Hide from dock on macOS
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }

            // Build tray menu
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let start = MenuItem::with_id(app, "start", "Start Recording", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "Stop Recording", false, None::<&str>)?;

            let menu = Menu::with_items(app, &[&start, &stop, &quit])?;

            let app_handle = app.handle().clone();

            // Build tray icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| {
                    let state: State<AppState> = app.state();
                    match event.id.as_ref() {
                        "quit" => {
                            app.exit(0);
                        }
                        "start" => {
                            let timestamp =
                                chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
                            let recording_dir = state.recordings_dir.join(&timestamp);

                            if let Err(e) = std::fs::create_dir_all(&state.recordings_dir) {
                                eprintln!("Failed to create directory: {}", e);
                                return;
                            }

                            let mut recorder = state.recorder.lock();
                            match recorder.start_recording(&recording_dir) {
                                Ok(_) => {
                                    println!("Recording started");
                                    update_tray_menu(&app_handle, true);
                                }
                                Err(e) => eprintln!("Failed to start recording: {}", e),
                            }
                        }
                        "stop" => {
                            let mut recorder = state.recorder.lock();
                            match recorder.stop_recording(Some(app)) {
                                Ok(output) => {
                                    println!("Recording saved to: {:?}", output.directory);
                                    update_tray_menu(&app_handle, false);
                                }
                                Err(e) => eprintln!("Failed to stop recording: {}", e),
                            }
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.move_window(Position::TrayCenter);
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Hide window when it loses focus
            let main_window = app.get_webview_window("main").unwrap();
            let main_window_clone = main_window.clone();
            main_window.on_window_event(move |event| {
                if let WindowEvent::Focused(false) = event {
                    let _ = main_window_clone.hide();
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            is_recording,
            get_recording_stats,
            check_setup_needed,
            get_whisper_models,
            get_llm_models,
            download_whisper_model,
            download_llm_model,
            complete_setup,
            get_config,
            transcribe_recording,
            summarize_transcript,
            open_editor,
            save_edited_transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
