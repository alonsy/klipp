//! Content Creator Mode (experimental): the choice the
//! recorder reads (creator.json), the cameras it can use, what its camera is
//! doing, each clip's camera track for the Content Creation Center, and the
//! short made from a clip (kept with the clip's info).

use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::{ffmpeg, meta};

/// What the recorder reads from `%APPDATA%\Klipp\creator.json` (see its `camera`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CreatorSettings {
    pub enabled: bool,
    /// The camera's name ("" = the first one).
    pub camera: String,
    pub height: u32,
    pub fps: u32,
}

impl Default for CreatorSettings {
    fn default() -> Self {
        Self { enabled: false, camera: String::new(), height: 1080, fps: 30 }
    }
}

fn path(settings_dir: &Path) -> PathBuf {
    settings_dir.join("creator.json")
}

pub fn read(settings_dir: &Path) -> CreatorSettings {
    fs::read(path(settings_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Saves the choice; the recorder picks it up within a couple of seconds.
pub fn write(settings_dir: &Path, s: &CreatorSettings) -> Result<()> {
    fs::create_dir_all(settings_dir)?;
    let file = path(settings_dir);
    let part = file.with_extension("json.part");
    fs::write(&part, serde_json::to_vec_pretty(s)?)?;
    fs::rename(&part, &file)?;
    Ok(())
}

/// The cameras Windows offers, as the recorder lists them (it has the camera code).
pub fn cameras() -> Result<Vec<String>> {
    let recorder = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let out = Command::new(&recorder).arg("--list-cameras").creation_flags(CREATE_NO_WINDOW.0).output().context("couldn't run klipp-recorder.exe")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("listing the cameras failed"));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().rev().find(|l| l.trim_start().starts_with('[')).context("the recorder listed no cameras")?;
    Ok(serde_json::from_str(line)?)
}

/// What the recorder's camera is doing (its camera.json), or off when it isn't running.
pub fn status(log_dir: &Path, running: bool) -> Value {
    let off = json!({ "state": "off" });
    if !running {
        return off;
    }
    fs::read(log_dir.join("camera.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(off)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraTrack {
    /// The camera track alone, timed from 0, in the studio's cache.
    pub path: String,
    /// Where in the clip its first picture is, in seconds.
    pub offset: f64,
    pub width: u32,
    pub height: u32,
}

/// The clip's camera track (its second video track), copied into the cache
/// without re-encoding for the preview, or `None` if the clip has none.
pub fn camera_track(clip: &Path, cache_dir: &Path) -> Result<Option<CameraTrack>> {
    let out = ffmpeg::probe_command().args(["-select_streams", "v", "-show_entries", "stream=start_time,width,height", "-of", "json"]).arg(clip).output()?;
    let json: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let (Some(game), Some(camera)) = (streams.first(), streams.get(1)) else { return Ok(None) };
    let start = |s: &Value| s["start_time"].as_str().and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0);
    let meta = fs::metadata(clip)?;
    let mut h = DefaultHasher::new();
    (clip, meta.len(), meta.modified()?.duration_since(UNIX_EPOCH)?.as_secs()).hash(&mut h);
    let dir = cache_dir.join("camera");
    fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{:016x}.mp4", h.finish()));
    if !file.exists() {
        let part = file.with_extension("part.mp4");
        let status = ffmpeg::command()
            .arg("-i")
            .arg(clip)
            .args(["-map", "0:v:1", "-c", "copy", "-bsf:v", "setts=pts=PTS-STARTPTS:dts=DTS-STARTDTS", "-movflags", "+faststart", "-y"])
            .arg(&part)
            .status()?;
        if !status.success() {
            let _ = fs::remove_file(&part);
            bail!("couldn't copy the camera track out of the clip");
        }
        fs::rename(&part, &file)?;
    }
    Ok(Some(CameraTrack {
        path: file.to_string_lossy().into_owned(),
        offset: ((start(camera) - start(game)) * 1000.0).round() / 1000.0,
        width: camera["width"].as_u64().unwrap_or(0) as u32,
        height: camera["height"].as_u64().unwrap_or(0) as u32,
    }))
}

/// Remembers the short made from a clip (`null` forgets it), with the clip's info.
pub fn save_short(clips_dir: &Path, clip: &Path, project: Value) -> Result<()> {
    meta::update_info(clips_dir, clip, |info| {
        if project.is_null() {
            if let Some(o) = info.as_object_mut() {
                o.remove("short");
            }
        } else {
            info["short"] = project;
        }
    })
}
