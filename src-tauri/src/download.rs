use crate::config::{AppConfig, ModelInfo};
use futures_util::StreamExt;
use std::fs::{self, File};
use std::io::Write;
use tauri::{AppHandle, Emitter};

/// Download progress event
#[derive(Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub percent: f32,
}

/// Download a model file with progress reporting
pub async fn download_model(
    app: &AppHandle,
    model: &ModelInfo,
) -> Result<std::path::PathBuf, String> {
    let models_dir = AppConfig::models_dir();

    // Ensure models directory exists
    fs::create_dir_all(&models_dir)
        .map_err(|e| format!("Failed to create models directory: {}", e))?;

    let dest_path = models_dir.join(&model.filename);

    // Skip if already downloaded
    if dest_path.exists() {
        let metadata = fs::metadata(&dest_path)
            .map_err(|e| format!("Failed to read file metadata: {}", e))?;

        // Check if file size matches (rough validation)
        if metadata.len() > model.size_bytes / 2 {
            println!("Model {} already exists, skipping download", model.id);
            return Ok(dest_path);
        }
    }

    println!("Downloading {} from {}", model.filename, model.url);

    let client = reqwest::Client::new();
    let response = client
        .get(&model.url)
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let total_size = response.content_length().unwrap_or(model.size_bytes);

    let mut file = File::create(&dest_path)
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut last_emit_percent: f32 = 0.0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download error: {}", e))?;

        file.write_all(&chunk)
            .map_err(|e| format!("Failed to write chunk: {}", e))?;

        downloaded += chunk.len() as u64;
        let percent = (downloaded as f32 / total_size as f32) * 100.0;

        // Only emit progress every 1% to avoid flooding
        if percent - last_emit_percent >= 1.0 || downloaded == total_size {
            last_emit_percent = percent;

            let progress = DownloadProgress {
                model_id: model.id.clone(),
                downloaded,
                total: total_size,
                percent,
            };

            // Emit progress event to frontend
            let _ = app.emit("download-progress", progress);
        }
    }

    file.flush()
        .map_err(|e| format!("Failed to flush file: {}", e))?;

    println!("Downloaded {} successfully", model.filename);
    Ok(dest_path)
}
