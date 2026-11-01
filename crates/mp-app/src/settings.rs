//! Preferences for this computer, as opposed to anything about a project.

use mp_analysis::stems::StemSettings;
use mp_analysis::transcribe::app_data_dir;
use mp_engine::AudioConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub audio: AudioConfig,
    pub stems: StemSettings,
    pub download_dir: Option<PathBuf>,
    pub download_kind: mp_analysis::online::DownloadKind,
}

fn path() -> PathBuf {
    app_data_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::create_dir_all(app_data_dir());
            if let Err(e) = std::fs::write(path(), text) {
                eprintln!("could not save settings: {e}");
            }
        }
    }
}
