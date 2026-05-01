use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub setup_complete: bool,
    pub whisper_model: Option<String>,
    pub llm_model: Option<String>,
}

impl AppConfig {
    /// Get the app data directory
    pub fn data_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("meeting-recorder")
    }

    /// Get the models directory
    pub fn models_dir() -> PathBuf {
        Self::data_dir().join("models")
    }

    /// Get the config file path
    fn config_path() -> PathBuf {
        Self::data_dir().join("config.json")
    }

    /// Load config from disk, or return default if not found
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Save config to disk
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&path, json).map_err(|e| format!("Failed to write config: {}", e))?;

        Ok(())
    }

    /// Check if setup is needed
    pub fn needs_setup(&self) -> bool {
        !self.setup_complete
    }

    /// Get whisper model path if downloaded
    pub fn whisper_model_path(&self) -> Option<PathBuf> {
        self.whisper_model
            .as_ref()
            .map(|name| Self::models_dir().join(name))
            .filter(|p| p.exists())
    }
}

/// Model info for downloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub size_bytes: u64,
    pub url: String,
    pub filename: String,
}

impl ModelInfo {
    /// Check if dev mode is enabled (uses tiny test files)
    fn is_dev_mode() -> bool {
        std::env::var("DEV_MODELS").is_ok()
    }

    /// Available whisper models
    pub fn whisper_models() -> Vec<Self> {
        if Self::is_dev_mode() {
            // Tiny test files for development (~1KB each)
            return vec![
                Self {
                    id: "whisper-dev".to_string(),
                    name: "[DEV] Tiny Test File".to_string(),
                    size_bytes: 1_000,
                    url: "https://httpbin.org/bytes/1000".to_string(),
                    filename: "whisper-dev.bin".to_string(),
                },
            ];
        }

        vec![
            Self {
                id: "whisper-base-en".to_string(),
                name: "Whisper Base (English)".to_string(),
                size_bytes: 148_000_000, // ~148MB
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin".to_string(),
                filename: "ggml-base.en.bin".to_string(),
            },
            Self {
                id: "whisper-small-en".to_string(),
                name: "Whisper Small (English)".to_string(),
                size_bytes: 488_000_000, // ~488MB
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin".to_string(),
                filename: "ggml-small.en.bin".to_string(),
            },
            Self {
                id: "whisper-medium-en".to_string(),
                name: "Whisper Medium (English)".to_string(),
                size_bytes: 1_533_000_000, // ~1.5GB
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin".to_string(),
                filename: "ggml-medium.en.bin".to_string(),
            },
        ]
    }

    /// Available LLM models (GGUF format for llama.cpp)
    pub fn llm_models() -> Vec<Self> {
        if Self::is_dev_mode() {
            // Tiny test files for development (~1KB each)
            return vec![
                Self {
                    id: "llm-dev".to_string(),
                    name: "[DEV] Tiny Test File".to_string(),
                    size_bytes: 1_000,
                    url: "https://httpbin.org/bytes/1000".to_string(),
                    filename: "llm-dev.bin".to_string(),
                },
            ];
        }

        vec![
            Self {
                id: "qwen3.5-4b".to_string(),
                name: "Qwen 3.5 4B (Fast)".to_string(),
                size_bytes: 3_500_000_000, // ~3.5GB
                url: "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q6_K.gguf".to_string(),
                filename: "Qwen3.5-4B-Q6_K.gguf".to_string(),
            },
            Self {
                id: "gemma4-26b-a4b".to_string(),
                name: "Gemma 4 26B-A4B (Recommended, MoE)".to_string(),
                size_bytes: 17_000_000_000, // ~17GB
                url: "https://huggingface.co/bartowski/google_gemma-4-26B-A4B-it-GGUF/resolve/main/google_gemma-4-26B-A4B-it-Q4_K_M.gguf".to_string(),
                filename: "google_gemma-4-26B-A4B-it-Q4_K_M.gguf".to_string(),
            },
        ]
    }
}
