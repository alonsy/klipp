//! Where things are: the recorder's settings file and the clips folder it names.

use std::fs;
use std::path::PathBuf;

pub struct Config {
    pub settings_path: PathBuf,
    pub clips_dir: PathBuf,
    /// The studio's own preferences (window behavior, playback).
    pub prefs_path: PathBuf,
    /// Studio caches (extracted audio) under %LOCALAPPDATA%.
    pub cache_dir: PathBuf,
    /// The recorder's logs, and setup's last check of this PC.
    pub log_dir: PathBuf,
}

pub fn load() -> Config {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
    let settings_path = appdata.join("Klipp").join("settings.toml");
    let clips_dir = fs::read_to_string(&settings_path)
        .ok()
        .and_then(|text| text.trim_start_matches('\u{feff}').parse::<toml::Table>().ok())
        .and_then(|t| t.get("storage")?.get("clips_dir")?.as_str().map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty())
        // Same default as the recorder's.
        .unwrap_or_else(|| {
            let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
            home.join("Videos").join("Klipp")
        });
    Config {
        settings_path,
        clips_dir,
        prefs_path: appdata.join("Klipp").join("studio.json"),
        cache_dir: local.join("Klipp").join("cache"),
        log_dir: local.join("Klipp"),
    }
}
