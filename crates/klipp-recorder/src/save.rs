//! Writes clips to disk on a worker thread, so saving never stalls capture.
//!
//! The clips folder holds only the clips themselves. What Klipp knows about
//! each one lives in a hidden `.klipp` folder next to them:
//! `.klipp\info\<game>\<clip>.json` (markers, tracks, map; the studio adds
//! trim points and track levels) and `.klipp\thumbs\<game>\<clip>.jpg`.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use klipp_media::{Chapter, Mp4Writer, SAMPLE_RATE, StreamInfo};
use serde::Serialize;
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW};
use windows::core::HSTRING;

use crate::arrivals::Arrivals;
use crate::clock;
use crate::cue::{CueHandle, Sound};
use crate::events::{EventKind, GameEvent};
use crate::log::{info, warning};
use crate::replay::{Clip, VIDEO};

/// The hidden folder inside the clips folder.
pub const META_DIR: &str = ".klipp";
/// Thumbnails are this wide (2x a library card).
const THUMB_WIDTH: u32 = 640;
/// A thumbnail that takes longer than this isn't worth waiting for.
const THUMB_TIMEOUT: Duration = Duration::from_secs(20);
/// Events closer together than this share a chapter.
const CHAPTER_MERGE: i64 = clock::SECOND / 2;
/// Chapters start this long before their event, so jumping to one shows the lead-up.
const CHAPTER_LEAD: i64 = clock::SECOND;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    /// Saved with the hotkey or the tray menu.
    Manual,
    /// Saved by the auto-clip engine.
    Auto,
}

pub struct SaveJob {
    pub clip: Clip,
    /// One per buffer stream id: video first, then the audio tracks.
    pub streams: Vec<StreamInfo>,
    /// Names of the audio tracks, in order (e.g. Mix, Game, Chat, Mic).
    pub tracks: Vec<String>,
    pub clips_dir: PathBuf,
    /// Folder name for the clip, e.g. "Counter-Strike 2" or "Desktop".
    pub game: String,
    pub kind: ClipKind,
    /// Why an auto-clip was saved; also added to the file name ("3k").
    pub reason: Option<EventKind>,
    /// Game events during the clip (absolute times), stored as markers.
    pub events: Vec<GameEvent>,
    /// The map and game mode, when the game reports them.
    pub map: String,
    pub mode: String,
    /// The moment the thumbnail shows (absolute time).
    pub thumb_at: i64,
    /// Its "arriving" entry, cleared once it's written (or failed).
    pub arrival: Option<u64>,
    pub requested_at: Instant,
}

/// The clip's info file. The studio reads it for markers, filters and
/// grouping, and adds its own fields (trim, track levels), so anything here
/// can also be rebuilt from the clip alone.
#[derive(Serialize)]
struct ClipInfo<'a> {
    version: u32,
    game: &'a str,
    kind: ClipKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "str::is_empty")]
    map: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    mode: &'a str,
    duration_secs: f64,
    tracks: &'a [String],
    events: Vec<EventInfo<'a>>,
}

#[derive(Serialize)]
struct EventInfo<'a> {
    kind: String,
    /// Seconds from the start of the clip.
    at_secs: f64,
    #[serde(skip_serializing_if = "str::is_empty")]
    detail: &'a str,
}

pub struct Saver {
    tx: Option<Sender<SaveJob>>,
    thread: Option<JoinHandle<()>>,
    last: Arc<Mutex<Option<PathBuf>>>,
}

impl Saver {
    pub fn start(cue: CueHandle, arrivals: Arc<Arrivals>) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<SaveJob>();
        let last = Arc::new(Mutex::new(None));
        let last_saved = last.clone();
        let thread = thread::Builder::new().name("saver".into()).spawn(move || {
            for job in rx {
                let requested_at = job.requested_at;
                let kind = job.kind;
                let result = save(&job);
                if let Some(id) = job.arrival {
                    arrivals.remove(id);
                }
                match result {
                    Ok(s) => {
                        *last_saved.lock().unwrap() = Some(s.path.clone());
                        // Hotkey clips played their sound at the press already.
                        if kind == ClipKind::Auto {
                            cue.play(Sound::AutoSaved);
                            crate::overlay::post(crate::overlay::Indicator::AutoSaved);
                        }
                        info!(
                            "saved {} ({:.1} s, {:.1} MB) in {} ms",
                            s.path.display(),
                            s.seconds,
                            s.bytes as f64 / 1e6,
                            requested_at.elapsed().as_millis()
                        );
                        // After the sound: the clip is already there, the thumbnail can follow.
                        let started = Instant::now();
                        match make_thumbnail(&s.path, &job, s.seconds) {
                            Ok(()) => info!("thumbnail made in {} ms", started.elapsed().as_millis()),
                            Err(e) => warning!("thumbnail for {} failed: {e:#}", s.path.display()),
                        }
                    }
                    Err(e) => {
                        cue.play(Sound::Error);
                        crate::overlay::post(crate::overlay::Indicator::Failed);
                        warning!("saving clip failed: {e:#}");
                    }
                }
            }
        })?;
        Ok(Self { tx: Some(tx), thread: Some(thread), last })
    }

    pub fn submit(&self, job: SaveJob) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(job);
        }
    }

    /// The most recently saved clip, if any (and if it still exists).
    pub fn last_clip(&self) -> Option<PathBuf> {
        self.last.lock().unwrap().clone().filter(|p| p.exists())
    }
}

impl Drop for Saver {
    /// Finishes any queued saves before returning.
    fn drop(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Saved {
    path: PathBuf,
    seconds: f64,
    bytes: usize,
}

fn save(job: &SaveJob) -> Result<Saved> {
    let folder = job.clips_dir.join(folder_name(&job.game));
    fs::create_dir_all(&folder).with_context(|| format!("creating {}", folder.display()))?;
    let stem = match job.reason {
        Some(reason) => format!("{}_{}", timestamp(), reason.label()),
        None => timestamp(),
    };
    let path = unique_path(&folder, &stem);
    // Written under a temporary name so nothing ever sees a half-written clip.
    let part = path.with_extension("mp4.part");
    let result = write_mp4(&part, job);
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    let seconds = result?;
    fs::rename(&part, &path)?;
    // The clip is usable without its info file, so a failure here only warns.
    if let Err(e) = write_info(&meta_path(job, &path, "info", "json")?, job, seconds) {
        warning!("writing clip info for {} failed: {e:#}", path.display());
    }
    Ok(Saved { path, seconds, bytes: job.clip.data.len() })
}

/// `<clips>\.klipp\<kind>\<game>\<clip stem>.<ext>`, creating the folders.
fn meta_path(job: &SaveJob, clip: &Path, kind: &str, ext: &str) -> Result<PathBuf> {
    let root = job.clips_dir.join(META_DIR);
    let dir = root.join(kind).join(folder_name(&job.game));
    if !root.exists() {
        fs::create_dir_all(&root)?;
        // Hidden like .git, so the clips folder only shows clips.
        let _ = unsafe { SetFileAttributesW(&HSTRING::from(root.as_os_str()), FILE_ATTRIBUTE_HIDDEN) };
    }
    fs::create_dir_all(&dir)?;
    let stem = clip.file_stem().context("clip has no file name")?;
    Ok(dir.join(stem).with_extension(ext))
}

fn seconds_into(job: &SaveJob, time: i64) -> f64 {
    ((time - job.clip.start_time) as f64 / clock::SECOND as f64 * 1000.0).round() / 1000.0
}

fn write_info(path: &Path, job: &SaveJob, seconds: f64) -> Result<()> {
    let start = job.clip.start_time;
    let events = job
        .events
        .iter()
        .filter(|e| e.time >= start)
        .map(|e| EventInfo { kind: e.kind.label(), at_secs: seconds_into(job, e.time), detail: &e.detail })
        .collect();
    let info = ClipInfo {
        version: 2,
        game: &job.game,
        kind: job.kind,
        reason: job.reason.map(EventKind::label),
        map: &job.map,
        mode: &job.mode,
        duration_secs: (seconds * 1000.0).round() / 1000.0,
        tracks: &job.tracks,
        events,
    };
    let part = path.with_extension("json.part");
    fs::write(&part, serde_json::to_string_pretty(&info)?)?;
    fs::rename(&part, path)?;
    Ok(())
}

/// One chapter per moment: events less than `CHAPTER_MERGE` apart share one
/// ("2K · Headshot"). Each runs until the next, the last until the end.
fn chapters(job: &SaveJob, end: i64) -> Vec<Chapter> {
    let start = job.clip.start_time;
    let mut groups: Vec<Vec<EventKind>> = Vec::new();
    let mut times: Vec<i64> = Vec::new();
    for e in job.events.iter().filter(|e| (start..=end).contains(&e.time)) {
        match times.last() {
            Some(&t) if e.time - t < CHAPTER_MERGE => groups.last_mut().unwrap().push(e.kind),
            _ => {
                times.push(e.time);
                groups.push(vec![e.kind]);
            }
        }
    }
    let ms = |t: i64| (t - start) * 1000 / clock::SECOND;
    let starts: Vec<i64> = times.iter().map(|&t| ms((t - CHAPTER_LEAD).max(start))).collect();
    // Players stretch the first chapter back to 0:00, so name that stretch.
    let intro = starts.first().filter(|&&s| s > 0).map(|&s| Chapter { start_ms: 0, end_ms: s, title: "Start".into() });
    intro
        .into_iter()
        .chain(groups.into_iter()
        .enumerate()
        .map(|(i, mut kinds)| {
            kinds.sort_by_key(|k| std::cmp::Reverse(k.rank()));
            kinds.dedup();
            // "Kill" adds nothing next to a headshot, multi-kill, knife or Zeus kill.
            if kinds.len() > 1 {
                kinds.retain(|&k| k != EventKind::Kill);
            }
            let title = kinds.iter().map(|k| k.title()).collect::<Vec<_>>().join(" · ");
            let end_ms = starts.get(i + 1).copied().unwrap_or(ms(end)).max(starts[i]);
            Chapter { start_ms: starts[i], end_ms, title }
        }))
        .collect()
}

/// Writes the clip with video starting at 0 and each audio track placed by
/// its capture time relative to the first video frame. Returns the length in seconds.
fn write_mp4(path: &Path, job: &SaveJob) -> Result<f64> {
    let start = job.clip.start_time;
    let last_video = job.clip.packets.iter().rev().find(|(p, _)| p.stream == VIDEO).context("clip has no video")?;
    let end = last_video.0.time;

    let mut writer = Mp4Writer::create(path, &job.streams, &chapters(job, end))?;
    // Per stream: the value subtracted from its timestamps.
    let mut base: Vec<Option<i64>> = vec![None; job.streams.len()];
    for (info, range) in &job.clip.packets {
        let s = info.stream as usize;
        if s >= job.streams.len() {
            continue;
        }
        let b = *base[s].get_or_insert_with(|| {
            if info.stream == VIDEO {
                info.pts
            } else {
                // Audio timestamps count samples. Start this track where it actually began.
                info.pts - (info.time - start) * SAMPLE_RATE as i64 / clock::SECOND
            }
        });
        writer.write(s, &job.clip.data[range.clone()], info.pts - b, info.dts - b, info.duration, info.key)?;
    }
    writer.finish()?;
    Ok((end - start) as f64 / clock::SECOND as f64)
}

/// ffmpeg.exe next to the recorder.
fn ffmpeg_exe() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("ffmpeg.exe"))).unwrap_or_else(|| "ffmpeg.exe".into())
}

/// Saves a JPEG of the clip's `thumb_at` moment, so the library never has to
/// decode clips to show them. Runs ffmpeg.exe at low priority on two threads,
/// so it stays out of the game's way.
fn make_thumbnail(clip: &Path, job: &SaveJob, seconds: f64) -> Result<()> {
    let out = meta_path(job, clip, "thumbs", "jpg")?;
    let part = out.with_extension("part.jpg");
    let at = seconds_into(job, job.thumb_at).clamp(0.0, (seconds - 0.1).max(0.0));
    let mut child = Command::new(ffmpeg_exe())
        .creation_flags(CREATE_NO_WINDOW.0 | BELOW_NORMAL_PRIORITY_CLASS.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-threads", "2", "-ss", &format!("{at:.3}"), "-i"])
        .arg(clip)
        .args(["-map", "0:v:0", "-frames:v", "1", "-vf", &format!("scale={THUMB_WIDTH}:-2"), "-q:v", "4", "-y"])
        .arg(&part)
        .spawn()
        .context("starting ffmpeg.exe")?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > THUMB_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&part);
            bail!("ffmpeg took too long");
        }
        thread::sleep(Duration::from_millis(25));
    };
    if !status.success() || !part.exists() {
        let _ = fs::remove_file(&part);
        bail!("ffmpeg exited with {status}");
    }
    fs::rename(&part, &out)?;
    Ok(())
}

/// A folder name Windows accepts; falls back to "Desktop".
fn folder_name(game: &str) -> String {
    let cleaned: String =
        game.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() { "Desktop".to_string() } else { trimmed }
}

fn timestamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

fn unique_path(folder: &Path, stem: &str) -> PathBuf {
    let mut path = folder.join(format!("{stem}.mp4"));
    let mut n = 2;
    while path.exists() || path.with_extension("mp4.part").exists() {
        path = folder.join(format!("{stem}_{n}.mp4"));
        n += 1;
    }
    path
}
