//! Fortnite's auto-clips on or off: what the recorder reads from
//! `%APPDATA%\Klipp\fortnite.json` (see its `fortnite`). On unless it says otherwise.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Choice {
    enabled: bool,
}

impl Default for Choice {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn path(settings_dir: &Path) -> PathBuf {
    settings_dir.join("fortnite.json")
}

pub fn enabled(settings_dir: &Path) -> bool {
    fs::read(path(settings_dir)).ok().and_then(|b| serde_json::from_slice::<Choice>(&b).ok()).unwrap_or_default().enabled
}

pub fn set_enabled(settings_dir: &Path, enabled: bool) -> Result<()> {
    fs::write(path(settings_dir), serde_json::to_vec_pretty(&Choice { enabled })?)?;
    Ok(())
}
