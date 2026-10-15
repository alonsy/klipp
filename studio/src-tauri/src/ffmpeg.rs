//! Running the bundled ffmpeg.exe: thumbnails, audio for the editor, exports.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Condvar, Mutex};
use std::time::UNIX_EPOCH;
use std::fs;

use anyhow::{Result, bail};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::meta;

/// ffmpeg.exe next to the studio executable.
pub fn exe() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("ffmpeg.exe"))).unwrap_or_else(|| "ffmpeg.exe".into())
}

/// An ffmpeg command that never opens a console window.
pub fn command() -> Command {
    let mut c = Command::new(exe());
    c.creation_flags(CREATE_NO_WINDOW.0).stdin(Stdio::null()).args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    c
}

/// An ffprobe command (next to ffmpeg.exe) that never opens a console window.
pub fn probe_command() -> Command {
    let mut c = Command::new(exe().with_file_name("ffprobe.exe"));
    c.creation_flags(CREATE_NO_WINDOW.0).stdin(Stdio::null()).args(["-v", "error"]);
    c
}

/// Copies every audio track out of the clip into the cache, without
/// re-encoding, for the editor's waveforms and track mixer. Track 0 is the
/// premix ("Mix"); separate tracks follow.
pub fn extract_tracks(clip: &Path, cache_dir: &Path) -> Result<Vec<PathBuf>> {
    let out = probe_command().args(["-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0"]).arg(clip).output()?;
    let count = String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.trim().is_empty()).count();
    let meta = fs::metadata(clip)?;
    let mut h = DefaultHasher::new();
    (clip, meta.len(), meta.modified()?.duration_since(UNIX_EPOCH)?.as_secs()).hash(&mut h);
    let dir = cache_dir.join("audio");
    fs::create_dir_all(&dir)?;
    let mut tracks = Vec::new();
    for k in 0..count {
        let path = dir.join(format!("{:016x}_{k}.m4a", h.finish()));
        if !path.exists() {
            let part = path.with_extension("part.m4a");
            let status = command().arg("-i").arg(clip).args(["-map", &format!("0:a:{k}"), "-c", "copy", "-y"]).arg(&part).status()?;
            if !status.success() {
                bail!("couldn't extract audio track {k}");
            }
            fs::rename(&part, &path)?;
        }
        tracks.push(path);
    }
    Ok(tracks)
}

/// Deletes extracted audio not used for a week.
pub fn trim_cache(cache_dir: &Path) {
    let week = std::time::Duration::from_secs(7 * 24 * 3600);
    for sub in ["thumbs", "audio"] {
        for entry in fs::read_dir(cache_dir.join(sub)).into_iter().flatten().flatten() {
            let old = entry.metadata().and_then(|m| m.accessed()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > week);
            if old {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// At most this many thumbnails are made at once, so scrolling a big library
/// doesn't start dozens of decoders.
const MAX_THUMB_JOBS: usize = 2;
static THUMB_JOBS: Mutex<usize> = Mutex::new(0);
static THUMB_DONE: Condvar = Condvar::new();

/// The clip's thumbnail in `.klipp`, made now if the recorder didn't make one
/// (older clips, trimmed copies, clips from elsewhere).
pub fn thumbnail(clips_dir: &Path, clip: &Path) -> Result<PathBuf> {
    let out = meta::thumb_path(clips_dir, clip);
    if out.exists() {
        return Ok(out);
    }
    meta::prepare(clips_dir, &out)?;
    let mut jobs = THUMB_JOBS.lock().unwrap();
    while *jobs >= MAX_THUMB_JOBS {
        jobs = THUMB_DONE.wait(jobs).unwrap();
    }
    *jobs += 1;
    drop(jobs);
    let at = meta::read_info(clips_dir, clip).as_ref().and_then(highlight).unwrap_or(1.0);
    let result = make_thumbnail(clip, &out, at);
    *THUMB_JOBS.lock().unwrap() -= 1;
    THUMB_DONE.notify_one();
    result.map(|()| out)
}

/// The moment worth showing: the event the clip is named after, or for a
/// clip saved with the hotkey, just before the end.
fn highlight(info: &serde_json::Value) -> Option<f64> {
    let reason = info["reason"].as_str();
    let events = info["events"].as_array()?;
    let at = |e: &serde_json::Value| e["at_secs"].as_f64();
    match reason {
        Some(r) => events.iter().rev().find(|e| e["kind"].as_str() == Some(r)).and_then(at),
        None => info["duration_secs"].as_f64().map(|d| (d - 3.0).max(0.0)),
    }
}

fn make_thumbnail(clip: &Path, out: &Path, at: f64) -> Result<()> {
    let part = out.with_extension("part.jpg");
    // Very short clips fall back to the start.
    for seek in [format!("{at:.3}"), "0".to_string()] {
        let status = command()
            .args(["-threads", "2", "-ss", &seek, "-i"])
            .arg(clip)
            .args(["-map", "0:v:0", "-frames:v", "1", "-vf", "scale=640:-2", "-q:v", "4", "-y"])
            .arg(&part)
            .status()?;
        if status.success() && part.exists() {
            fs::rename(&part, out)?;
            return Ok(());
        }
    }
    bail!("ffmpeg couldn't read a frame from {}", clip.display())
}
