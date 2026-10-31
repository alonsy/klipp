//! The clip library: every `<game>\<clip>.mp4` under the clips folder, with
//! what its info file in `.klipp` says about it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};

use crate::export::EXPORTS_DIR;
use crate::meta;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub path: String,
    /// File name without extension, e.g. "2026-10-13_21-29-01_2k".
    pub name: String,
    pub game: String,
    /// "manual", "auto", or "" when the clip has no info file.
    pub kind: String,
    /// The event the clip is named after, e.g. "2k".
    pub label: Option<String>,
    /// Milliseconds since 1970, from the file's modification time.
    pub created: i64,
    pub duration_secs: Option<f64>,
    pub size_bytes: u64,
    pub favorite: bool,
    pub tracks: Vec<String>,
    /// Which tracks after Mix carry voices. Empty on older clips.
    pub voices: Vec<bool>,
    pub events: Vec<Event>,
    /// e.g. "de_mirage" and "competitive", when the game reported them.
    pub map: String,
    pub mode: String,
    /// The saved thumbnail, if there is one.
    pub thumb: Option<String>,
    /// Trim points and track levels saved by the editor.
    pub edit: Option<Value>,
    /// Content Creator Mode: the clip has a camera track.
    pub camera: bool,
    /// The short made from it in the Content Creation Center.
    pub short: Option<Value>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub kind: String,
    pub at_secs: f64,
    pub detail: String,
}

pub fn list(clips_dir: &Path) -> Vec<Clip> {
    let mut clips = Vec::new();
    let Ok(entries) = fs::read_dir(clips_dir) else { return clips };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            // .klipp, .trash and anything else hidden, and exports.
            if name.starts_with('.') || name == EXPORTS_DIR {
                continue;
            }
            meta::adopt_sidecars(clips_dir, &path);
            for file in fs::read_dir(&path).into_iter().flatten().flatten() {
                if is_mp4(&file.path()) {
                    clips.extend(read_clip(clips_dir, &file.path(), &name));
                }
            }
        } else if is_mp4(&path) {
            clips.extend(read_clip(clips_dir, &path, "Other"));
        }
    }
    clips.sort_by_key(|c| std::cmp::Reverse(c.created));
    clips
}

fn is_mp4(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
}

fn read_clip(clips_dir: &Path, path: &Path, game: &str) -> Option<Clip> {
    let meta = fs::metadata(path).ok()?;
    let created = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
    let name = path.file_stem()?.to_string_lossy().into_owned();
    let info = meta::read_info(clips_dir, path).unwrap_or(Value::Null);
    let text = |key: &str| info[key].as_str().unwrap_or_default().to_string();
    let events = info["events"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|e| Event {
                    kind: e["kind"].as_str().unwrap_or_default().to_string(),
                    at_secs: e["at_secs"].as_f64().unwrap_or(0.0),
                    detail: e["detail"].as_str().unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let thumb = meta::thumb_path(clips_dir, path);
    Some(Clip {
        path: path.to_string_lossy().into_owned(),
        label: info["reason"].as_str().map(String::from).or_else(|| label_from_name(&name)),
        name,
        game: info["game"].as_str().unwrap_or(game).to_string(),
        kind: text("kind"),
        created,
        duration_secs: info["duration_secs"].as_f64(),
        size_bytes: meta.len(),
        favorite: info["favorite"].as_bool().unwrap_or(false),
        tracks: info["tracks"].as_array().map(|t| t.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default(),
        voices: info["voices"].as_array().map(|v| v.iter().map(|b| b.as_bool().unwrap_or(false)).collect()).unwrap_or_default(),
        events,
        map: text("map"),
        mode: text("mode"),
        thumb: thumb.exists().then(|| thumb.to_string_lossy().into_owned()),
        edit: Some(info["edit"].clone()).filter(Value::is_object),
        camera: info["camera"].as_bool().unwrap_or(false),
        short: Some(info["short"].clone()).filter(Value::is_object),
    })
}

/// "2026-10-13_21-29-01_2k" → "2k". Names are a 19-character timestamp, then
/// optionally "_<label>" (and "_<n>" if a clip with the same name existed).
fn label_from_name(name: &str) -> Option<String> {
    let rest = name.get(19..)?.strip_prefix('_')?;
    let label = rest.split('_').next()?;
    (!label.is_empty() && !label.chars().all(|c| c.is_ascii_digit())).then(|| label.to_string())
}

/// Sets or clears the favorite flag of each clip.
pub fn set_favorite(clips_dir: &Path, paths: &[PathBuf], favorite: bool) -> Result<()> {
    for clip in paths {
        meta::update_info(clips_dir, clip, |info| info["favorite"] = json!(favorite))?;
    }
    Ok(())
}

/// Remembers the editor's trim points and track levels for a clip.
pub fn save_edit(clips_dir: &Path, clip: &Path, edit: Value) -> Result<()> {
    meta::update_info(clips_dir, clip, |info| {
        if edit.is_null() {
            if let Some(o) = info.as_object_mut() {
                o.remove("edit");
            }
        } else {
            info["edit"] = edit;
        }
    })
}
