//! User settings, persisted as JSON next to the models.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Tauri accelerator syntax, e.g. "Ctrl+Alt+Space".
    pub shortcut: String,
    /// ISO language code, or "auto" for whisper's own detection.
    pub language: String,
    /// Input device name. `None` follows the OS default, which is only a good
    /// idea when the OS default is actually a working microphone.
    pub input_device: Option<String>,
    /// Filename inside the models directory.
    pub model_file: String,
    /// Write the resampled audio of each utterance to disk for debugging.
    pub save_debug_wav: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: "Ctrl+Alt+Space".to_string(),
            language: "en".to_string(),
            input_device: None,
            model_file: "ggml-base.en.bin".to_string(),
            save_debug_wav: false,
        }
    }
}

impl Settings {
    pub fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(config_dir))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(config_dir)?;
        std::fs::write(Self::path(config_dir), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    fn path(config_dir: &Path) -> PathBuf {
        config_dir.join("settings.json")
    }
}
