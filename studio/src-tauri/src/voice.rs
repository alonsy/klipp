//! "Klipp that": the choice the recorder reads (voice.json), and
//! teaching it a voice, which the recorder does beside the running one.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// What the recorder reads from `%APPDATA%\Klipp\voice.json` (see its `voice`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub personal: Vec<String>,
}

fn path(settings_dir: &Path) -> PathBuf {
    settings_dir.join("voice.json")
}

pub fn read(settings_dir: &Path) -> VoiceSettings {
    fs::read(path(settings_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// On or off; what it learned stays either way.
pub fn set_enabled(settings_dir: &Path, enabled: bool) -> Result<()> {
    let settings = VoiceSettings { enabled, ..read(settings_dir) };
    fs::write(path(settings_dir), serde_json::to_vec_pretty(&settings)?)?;
    Ok(())
}
