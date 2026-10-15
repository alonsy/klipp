//! The studio's own preferences (`%APPDATA%\Klipp\studio.json`): window and
//! playback behavior that the recorder doesn't care about. The page fills in
//! defaults; this only stores what was changed.

use std::fs;
use std::path::Path;

use anyhow::Result;
use serde_json::{Map, Value};

pub fn read(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

pub fn set(path: &Path, key: &str, value: Value) -> Result<()> {
    let mut prefs = read(path);
    prefs.insert(key.to_string(), value);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let part = path.with_extension("json.part");
    fs::write(&part, serde_json::to_string_pretty(&prefs)?)?;
    fs::rename(&part, path)?;
    Ok(())
}
