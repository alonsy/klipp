//! Squeeze Space (Settings → Storage): re-encodes clips smaller on this PC's
//! graphics card, at one of four levels (Low, Mid, High, Ultra). Every audio
//! track, the chapters and Klipp's info are kept. The new file replaces the
//! clip under the same name, with the same dates, and only once it checks
//! out; if anything goes wrong the original stays as it was.
//!
//! A clip can only be squeezed again at a stronger level, and at most
//! `MAX_SQUEEZES` times, so quality isn't lost a little more every time. What
//! was done is written to the clip's info (`"squeeze": { level, count, … }`)
//! and to the file itself (`klipp_squeeze*` tags), for clips without info.

use std::collections::HashMap;
use std::fs::{self, FileTimes};
use std::io::{BufRead, BufReader, Read};
use std::os::windows::fs::FileTimesExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};
use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW};

use crate::export::EXPORTS_DIR;
use crate::ffmpeg;
use crate::meta;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Low,
    Mid,
    High,
    Ultra,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Low, Level::Mid, Level::High, Level::Ultra];

    pub fn name(self) -> &'static str {
        match self {
            Level::Low => "low",
            Level::Mid => "mid",
            Level::High => "high",
            Level::Ultra => "ultra",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.name() == s.trim())
    }

    pub fn params(self) -> &'static Params {
        &LEVELS[self as usize]
    }
}

// ─── The levels: the one place to tune them ────────────────────────────────

pub struct Params {
    /// Taller videos are scaled down to this height (0: same size).
    pub max_height: u32,
    /// Higher frame rates come down to this (0: same frame rate).
    pub max_fps: f64,
    /// Target video bitrate, as a share of the clip's original video bitrate.
    pub ratio: f64,
    /// The target never goes below this (kbps), for clips that were small already.
    pub min_kbps: f64,
}

/// Low, Mid, High, Ultra, in that order (indexed by `Level`). Measured on 10 of
/// the owner's clips (1440p at 120 and 60 fps, AV1, 18-40 Mbit/s; VMAF against
/// the original at 1440p, ~99 for a clip against itself; 2026-10-28): files
/// come out at about 53%, 28%, 17% and 8% (audio is copied), at VMAF ~95.5,
/// ~95 per frame, ~91 and ~87. Scaling down cost more than it saved: at the
/// same bitrate High scored 91 at 1440p and 86 at 1080p, Ultra 87 at 1440p and
/// 74 at 720p (HUD text ghosted). 4K clips come down to 1440p (untested).
/// Slower presets, multipass, lookahead and AQ gained under half a point;
/// H.264 at `H264_FOR_AV1` matched AV1 within 1-2 points.
pub const LEVELS: [Params; 4] = [
    // Low: same size and frame rate, about half the bitrate.
    Params { max_height: 0, max_fps: 0.0, ratio: 0.50, min_kbps: 4000.0 },
    // Mid: same size, at most 60 fps: each frame keeps about Low's quality.
    Params { max_height: 0, max_fps: 60.0, ratio: 0.25, min_kbps: 3000.0 },
    // High: at most 1440p and 60 fps, a much lower bitrate.
    Params { max_height: 1440, max_fps: 60.0, ratio: 0.15, min_kbps: 2000.0 },
    // Ultra: at most 1440p and 30 fps.
    Params { max_height: 1440, max_fps: 30.0, ratio: 0.06, min_kbps: 1000.0 },
];
/// VBR peak (maxrate) and buffer size, as multiples of the target bitrate.
pub const MAXRATE: f64 = 1.5;
pub const BUFSIZE: f64 = 2.0;
/// H.264 needs more bits than AV1 to look the same: the target for an AV1 clip
/// on a PC that can only encode H.264 is this much higher.
pub const H264_FOR_AV1: f64 = 1.6;
/// NVENC's speed/quality preset (p1 fastest … p7 best).
pub const NVENC_PRESET: &str = "p5";
/// A clip is squeezed at most this many times in all.
pub const MAX_SQUEEZES: u32 = 2;
/// Skipped unless the result is at most this share of the clip's size now.
pub const WORTH_IT: f64 = 0.9;
/// On top of the video and audio estimate: the container and the encoder's
/// usual overshoot (real clips came out 3–5% over the bare estimate at every level).
pub const OVERHEAD: f64 = 1.04;

// ───────────────────────────────────────────────────────────────────────────

/// Clips modified this recently are left alone (the recorder may be writing).
const RECENT: Duration = Duration::from_secs(600);
/// The new file's length may differ from the original's by this much (s).
const DURATION_SLACK: f64 = 0.3;
/// The squeezed file is written next to the clip under this extension first.
const PART_EXT: &str = "mp4.squeeze";
/// GPU encoders tried in this order: AV1 first, then H.264.
const ENCODERS: [&str; 6] = ["av1_nvenc", "av1_amf", "av1_qsv", "h264_nvenc", "h264_amf", "h264_qsv"];

/// What was done to a clip before.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Squeezed {
    pub level: Option<Level>,
    pub count: u32,
    /// Its size before the first squeeze.
    pub original_bytes: Option<u64>,
    /// Its video bitrate before the first squeeze (kbps): levels are shares of this.
    pub source_kbps: Option<f64>,
}

/// The rule that stops clips from being squeezed over and over: only a
/// stronger level than before, and `MAX_SQUEEZES` times at most.
pub fn can_squeeze(done: &Squeezed, level: Level) -> bool {
    done.count < MAX_SQUEEZES && done.level.is_none_or(|l| level > l)
}

/// From the clip's info, or (without info) from the tags in the file.
fn squeezed(info: Option<&Value>, tags: &Value) -> Squeezed {
    if let Some(s) = info.map(|i| &i["squeeze"]).filter(|s| s.is_object()) {
        let level = s["level"].as_str().and_then(Level::from_name);
        return Squeezed {
            level,
            count: s["count"].as_u64().map_or(level.is_some() as u32, |c| c as u32),
            original_bytes: s["original_bytes"].as_u64(),
            source_kbps: s["source_kbps"].as_f64(),
        };
    }
    let tag = |key: &str| tags[key].as_str().map(str::trim);
    let level = tag("klipp_squeeze").and_then(Level::from_name);
    Squeezed {
        level,
        count: tag("klipp_squeeze_count").and_then(|c| c.parse().ok()).unwrap_or(level.is_some() as u32),
        original_bytes: tag("klipp_squeeze_original_bytes").and_then(|b| b.parse().ok()),
        source_kbps: tag("klipp_squeeze_source_kbps").and_then(|k| k.parse().ok()),
    }
}

/// What ffprobe says about a clip.
#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub duration: f64,
    pub height: u32,
    pub fps: f64,
    pub codec: String,
    pub video_kbps: f64,
    pub audio_streams: usize,
    pub audio_bytes: u64,
    /// The file's own tags (format tags).
    pub tags: Value,
}

fn probe(path: &Path) -> Result<Probe> {
    let out = ffmpeg::probe_command()
        .args(["-show_entries", "format=duration,size:format_tags:stream=codec_type,codec_name,height,avg_frame_rate,bit_rate", "-of", "json"])
        .arg(path)
        .output()
        .context("starting ffprobe.exe")?;
    ensure!(out.status.success(), "couldn't read {}", path.display());
    let json: Value = serde_json::from_slice(&out.stdout)?;
    let streams = json["streams"].as_array().context("no streams")?;
    let video = streams.iter().find(|s| s["codec_type"] == "video").context("no video")?;
    let number = |v: &Value| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64());
    let duration = number(&json["format"]["duration"]).filter(|d| *d > 0.0).context("no duration")?;
    let size = number(&json["format"]["size"]).unwrap_or(0.0);
    let audio: Vec<&Value> = streams.iter().filter(|s| s["codec_type"] == "audio").collect();
    let audio_bps: f64 = audio.iter().filter_map(|s| number(&s["bit_rate"])).sum();
    let video_kbps = number(&video["bit_rate"]).unwrap_or_else(|| (size * 8.0 / duration - audio_bps).max(0.0)) / 1000.0;
    let fps = video["avg_frame_rate"]
        .as_str()
        .and_then(|r| r.split_once('/'))
        .and_then(|(n, d)| Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok().filter(|d| *d > 0.0)?))
        .unwrap_or(60.0);
    Ok(Probe {
        duration,
        height: video["height"].as_u64().unwrap_or(0) as u32,
        fps,
        codec: video["codec_name"].as_str().unwrap_or_default().to_string(),
        video_kbps,
        audio_streams: audio.len(),
        audio_bytes: (audio_bps * duration / 8.0) as u64,
        tags: json["format"]["tags"].clone(),
    })
}

/// Probes by path, with the size and modified time they were made for.
type ProbeCache = HashMap<PathBuf, (u64, SystemTime, Probe)>;

/// So opening the dialog again is instant.
static PROBES: LazyLock<Mutex<ProbeCache>> = LazyLock::new(Default::default);

fn probe_cached(path: &Path, bytes: u64, modified: SystemTime) -> Option<Probe> {
    if let Some((b, m, p)) = PROBES.lock().unwrap().get(path)
        && *b == bytes
        && *m == modified
    {
        return Some(p.clone());
    }
    let p = probe(path).ok()?;
    PROBES.lock().unwrap().insert(path.to_path_buf(), (bytes, modified, p.clone()));
    Some(p)
}

/// The GPU encoder this PC has (tried once with a tiny encode; remembered once found).
static ENCODER: Mutex<Option<&'static str>> = Mutex::new(None);

pub fn encoder() -> Option<&'static str> {
    let mut found = ENCODER.lock().unwrap();
    if found.is_none() {
        *found = ENCODERS.into_iter().find(|e| {
            ffmpeg::command()
                .args(["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30", "-t", "0.2", "-c:v", e, "-f", "null", "-"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        });
    }
    *found
}

/// A clip that could be squeezed, as the plan sees it.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub bytes: u64,
    pub probe: Probe,
    pub done: Squeezed,
}

impl Candidate {
    /// Its video bitrate before any squeeze (kbps).
    fn source_kbps(&self) -> f64 {
        self.done
            .source_kbps
            .or_else(|| self.done.level.map(|l| self.probe.video_kbps / l.params().ratio))
            .unwrap_or(self.probe.video_kbps)
    }

    /// The video bitrate `level` aims for (kbps). `h264`: this PC encodes H.264, not AV1.
    pub fn target_kbps(&self, level: Level, h264: bool) -> f64 {
        let p = level.params();
        let factor = if h264 && self.probe.codec == "av1" { H264_FOR_AV1 } else { 1.0 };
        (self.source_kbps() * p.ratio * factor).max(p.min_kbps)
    }

    /// Its size after squeezing at `level`, or `None` if that wouldn't save enough.
    pub fn estimate(&self, level: Level, h264: bool) -> Option<u64> {
        let kbps = self.target_kbps(level, h264);
        let video = kbps * 1000.0 / 8.0 * self.probe.duration;
        let after = (video + self.probe.audio_bytes as f64) * OVERHEAD;
        (kbps < self.probe.video_kbps * WORTH_IT && after < self.bytes as f64 * WORTH_IT).then_some(after as u64)
    }
}

/// One level of the plan: what squeezing everything at it would do.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LevelPlan {
    pub level: Level,
    /// What it does, from `LEVELS`, for the dialog to describe (0: unchanged).
    pub max_height: u32,
    pub max_fps: f64,
    /// Clips it would squeeze.
    pub clips: usize,
    /// Clips it skips because they were squeezed before (as strong, or twice).
    pub already: usize,
    /// Clips it skips because they're small enough already.
    pub small: usize,
    /// What the clips it would squeeze take now, and about what they'd take after.
    pub bytes_before: u64,
    pub bytes_after: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The ffmpeg encoder ("av1_nvenc"), or `None` if this PC has no GPU encoder.
    pub encoder: Option<String>,
    /// Clips in scope and what they take.
    pub clips: usize,
    pub bytes: u64,
    /// Left out: favorites (when skipped), clips saved in the last minutes, unreadable files.
    pub favorites: usize,
    pub recent: usize,
    pub unreadable: usize,
    pub levels: Vec<LevelPlan>,
}

pub fn plan_levels(clips: &[Candidate], h264: bool) -> Vec<LevelPlan> {
    Level::ALL
        .into_iter()
        .map(|level| {
            let params = level.params();
            let mut p = LevelPlan {
                level,
                max_height: params.max_height,
                max_fps: params.max_fps,
                clips: 0,
                already: 0,
                small: 0,
                bytes_before: 0,
                bytes_after: 0,
            };
            for c in clips {
                if !can_squeeze(&c.done, level) {
                    p.already += 1;
                } else if let Some(after) = c.estimate(level, h264) {
                    p.clips += 1;
                    p.bytes_before += c.bytes;
                    p.bytes_after += after;
                } else {
                    p.small += 1;
                }
            }
            p
        })
        .collect()
}

/// What each level would do to the clips in the clips folder.
pub fn plan(clips_dir: &Path, skip_favorites: bool) -> Plan {
    let encoder = encoder();
    let (clips, left_out) = candidates(clips_dir, skip_favorites);
    let h264 = encoder.is_some_and(|e| e.starts_with("h264"));
    Plan {
        encoder: encoder.map(String::from),
        clips: clips.len(),
        bytes: clips.iter().map(|c| c.bytes).sum(),
        favorites: left_out.favorites,
        recent: left_out.recent,
        unreadable: left_out.unreadable,
        levels: plan_levels(&clips, h264),
    }
}

#[derive(Default)]
struct LeftOut {
    favorites: usize,
    recent: usize,
    unreadable: usize,
}

/// Every clip in the clips folder: in the game folders and loose in the
/// folder itself, not exports, not `.klipp` or `.trash`.
fn clip_files(clips_dir: &Path) -> Vec<PathBuf> {
    let is_mp4 = |p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4"));
    let mut files = Vec::new();
    for entry in fs::read_dir(clips_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if name.starts_with('.') || name == EXPORTS_DIR {
                continue;
            }
            files.extend(fs::read_dir(&path).into_iter().flatten().flatten().map(|f| f.path()).filter(|p| is_mp4(p) && p.is_file()));
        } else if is_mp4(&path) {
            files.push(path);
        }
    }
    files.sort();
    files
}

fn is_recent(modified: SystemTime) -> bool {
    modified.elapsed().map_or(true, |age| age < RECENT)
}

/// The clips in scope with what ffprobe and their info say (probed a few at a time).
fn candidates(clips_dir: &Path, skip_favorites: bool) -> (Vec<Candidate>, LeftOut) {
    let mut left_out = LeftOut::default();
    let mut todo = Vec::new();
    for path in clip_files(clips_dir) {
        let Some((bytes, modified)) = fs::metadata(&path).ok().and_then(|m| Some((m.len(), m.modified().ok()?))) else { continue };
        // Only clips Klipp saved (they have its info): never another app's
        // videos in a clips folder that's shared, like Videos.
        let Some(info) = meta::read_info(clips_dir, &path) else { continue };
        let info = Some(info);
        if skip_favorites && info.as_ref().is_some_and(|i| i["favorite"] == true) {
            left_out.favorites += 1;
        } else if is_recent(modified) {
            left_out.recent += 1;
        } else {
            todo.push((path, bytes, modified, info));
        }
    }
    let next = AtomicUsize::new(0);
    let probes: Mutex<Vec<Option<Probe>>> = Mutex::new(vec![None; todo.len()]);
    std::thread::scope(|s| {
        for _ in 0..6 {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((path, bytes, modified, _)) = todo.get(i) else { break };
                    let p = probe_cached(path, *bytes, *modified);
                    probes.lock().unwrap()[i] = p;
                }
            });
        }
    });
    let mut clips = Vec::new();
    for ((path, bytes, _, info), probe) in todo.into_iter().zip(probes.into_inner().unwrap()) {
        match probe {
            Some(probe) => {
                let done = squeezed(info.as_ref(), &probe.tags);
                clips.push(Candidate { path, bytes, probe, done });
            }
            None => left_out.unreadable += 1,
        }
    }
    (clips, left_out)
}

// ─── Running ───────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub level: Level,
    /// Clips finished (squeezed, skipped or failed) and to do in all.
    pub done: usize,
    pub total: usize,
    /// The clip being squeezed now (file name without extension).
    pub current: String,
    /// The whole run, 0 to 1.
    pub fraction: f64,
    pub saved_bytes: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub level: Option<Level>,
    pub squeezed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub saved_bytes: u64,
    pub cancelled: bool,
    /// Why it couldn't start, if it couldn't.
    pub error: Option<String>,
    /// What went wrong with some clips ("<clip>: <why>"), the first few.
    pub errors: Vec<String>,
}

#[derive(Default)]
struct Control {
    cancel: AtomicBool,
    child: Mutex<Option<Child>>,
}

impl Control {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }
}

/// The running squeeze (one at a time), for cancelling and for a dialog opened midway.
#[derive(Default)]
pub struct Squeezer {
    busy: Arc<AtomicBool>,
    control: Arc<Control>,
    last: Arc<Mutex<Option<Progress>>>,
}

impl Squeezer {
    /// Squeezes every clip that `level` would (see `plan`) on its own thread.
    /// Progress arrives as "squeeze-progress" events, the end as "squeeze-done".
    pub fn start(&self, app: AppHandle, clips_dir: PathBuf, level: Level, skip_favorites: bool) -> Result<()> {
        if self.busy.swap(true, Ordering::SeqCst) {
            bail!("Klipp is squeezing clips already");
        }
        self.control.cancel.store(false, Ordering::SeqCst);
        // Until the clips are looked at, a dialog opened now shows "getting ready".
        *self.last.lock().unwrap() = Some(Progress { level, done: 0, total: 0, current: String::new(), fraction: 0.0, saved_bytes: 0 });
        let (busy, control, last) = (self.busy.clone(), self.control.clone(), self.last.clone());
        std::thread::spawn(move || {
            let summary = run(&clips_dir, level, skip_favorites, &control, &|p: &Progress| {
                *last.lock().unwrap() = Some(p.clone());
                let _ = app.emit("squeeze-progress", p);
            });
            *last.lock().unwrap() = None;
            busy.store(false, Ordering::SeqCst);
            let _ = app.emit("squeeze-done", summary);
        });
        Ok(())
    }

    /// Stops after killing the clip in progress (it stays as it was); squeezed clips stay squeezed.
    pub fn cancel(&self) {
        self.control.stop();
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// Where a running squeeze is (`None` when none runs).
    pub fn status(&self) -> Option<Progress> {
        self.last.lock().unwrap().clone().filter(|_| self.busy())
    }
}

fn run(clips_dir: &Path, level: Level, skip_favorites: bool, control: &Control, progress: &dyn Fn(&Progress)) -> Summary {
    let mut summary = Summary { level: Some(level), ..Summary::default() };
    let Some(encoder) = encoder() else {
        summary.error = Some("This PC's graphics card can't re-encode video, so clips can't be squeezed here.".into());
        return summary;
    };
    remove_leftovers(clips_dir);
    let h264 = encoder.starts_with("h264");
    let (clips, _) = candidates(clips_dir, skip_favorites);
    let todo: Vec<Candidate> = clips.into_iter().filter(|c| can_squeeze(&c.done, level) && c.estimate(level, h264).is_some()).collect();
    let total = todo.len();
    for (i, clip) in todo.iter().enumerate() {
        if control.cancelled() {
            summary.cancelled = true;
            break;
        }
        let name = clip.path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let saved = summary.saved_bytes;
        let report = |f: f64| {
            progress(&Progress { level, done: i, total, current: name.clone(), fraction: (i as f64 + f) / total as f64, saved_bytes: saved })
        };
        report(0.0);
        match squeeze_clip(clips_dir, &clip.path, level, encoder, control, &report) {
            Ok(Outcome::Squeezed { before, after }) => {
                summary.squeezed += 1;
                summary.saved_bytes += before.saturating_sub(after);
            }
            Ok(Outcome::Skipped) => summary.skipped += 1,
            Err(_) if control.cancelled() => {
                summary.cancelled = true;
                break;
            }
            Err(e) => {
                summary.failed += 1;
                if summary.errors.len() < 5 {
                    summary.errors.push(format!("{name}: {e:#}"));
                }
            }
        }
    }
    if !summary.cancelled && total > 0 {
        progress(&Progress { level, done: total, total, current: String::new(), fraction: 1.0, saved_bytes: summary.saved_bytes });
    }
    summary
}

/// Squeezed files a crash left behind (the clips themselves were never touched).
fn remove_leftovers(clips_dir: &Path) {
    let mut folders = vec![clips_dir.to_path_buf()];
    folders.extend(fs::read_dir(clips_dir).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()));
    for folder in folders {
        for file in fs::read_dir(folder).into_iter().flatten().flatten() {
            if file.file_name().to_string_lossy().to_lowercase().ends_with(&format!(".{PART_EXT}")) {
                let _ = fs::remove_file(file.path());
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Squeezed { before: u64, after: u64 },
    /// Squeezed before, small enough, too new, or the result wasn't smaller.
    Skipped,
}

/// Squeezes one clip in place. The rules are checked again first: the clip
/// may have changed since the plan.
fn squeeze_clip(clips_dir: &Path, clip: &Path, level: Level, encoder: &str, control: &Control, on_progress: &dyn Fn(f64)) -> Result<Outcome> {
    let meta = fs::metadata(clip)?;
    let (modified, created) = (meta.modified()?, meta.created().ok());
    if is_recent(modified) {
        return Ok(Outcome::Skipped);
    }
    let probe = probe(clip)?;
    let info = meta::read_info(clips_dir, clip);
    let done = squeezed(info.as_ref(), &probe.tags);
    let c = Candidate { path: clip.to_path_buf(), bytes: meta.len(), probe, done };
    let h264 = encoder.starts_with("h264");
    if !can_squeeze(&c.done, level) || c.estimate(level, h264).is_none() {
        return Ok(Outcome::Skipped);
    }
    let kbps = c.target_kbps(level, h264);
    let count = c.done.count + 1;
    let original_bytes = c.done.original_bytes.unwrap_or(c.bytes);
    let source_kbps = c.source_kbps().round();
    let tags = [
        ("klipp_squeeze", level.name().to_string()),
        ("klipp_squeeze_count", count.to_string()),
        ("klipp_squeeze_original_bytes", original_bytes.to_string()),
        ("klipp_squeeze_source_kbps", format!("{source_kbps:.0}")),
    ];
    let part = clip.with_extension(PART_EXT);
    let result: Result<Option<u64>> = (|| {
        // Decoding on the graphics card is much faster; if it can't, the CPU does it.
        let mut encoded = Err(anyhow!("not started"));
        for gpu_decode in [true, false] {
            let args = ffmpeg_args(clip, &part, level, &c.probe, encoder, kbps, gpu_decode, &tags);
            encoded = run_ffmpeg(&args, c.probe.duration, control, on_progress);
            if encoded.is_ok() || control.cancelled() {
                break;
            }
        }
        encoded?;
        let after = verify(&part, &c.probe)?;
        if after >= c.bytes {
            return Ok(None);
        }
        // Nothing may have changed the clip meanwhile.
        let now = fs::metadata(clip)?;
        ensure!(now.len() == c.bytes && now.modified()? == modified, "it changed while it was being squeezed");
        let mut times = FileTimes::new().set_modified(modified);
        if let Some(created) = created {
            times = times.set_created(created);
        }
        fs::File::options().write(true).open(&part)?.set_times(times)?;
        fs::rename(&part, clip).context("couldn't replace it (open in another app?)")?;
        Ok(Some(after))
    })();
    let after = match result {
        Ok(Some(after)) => after,
        Ok(None) => {
            let _ = fs::remove_file(&part);
            return Ok(Outcome::Skipped);
        }
        Err(e) => {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
    };
    let record = json!({
        "level": level.name(),
        "count": count,
        "original_bytes": original_bytes,
        "source_kbps": source_kbps,
        "at": iso_now(),
    });
    // The file carries the same in its tags, so this failing loses nothing.
    if let Err(e) = meta::update_info(clips_dir, clip, |info| info["squeeze"] = record) {
        eprintln!("squeeze: couldn't update the info of {}: {e:#}", clip.display());
    }
    Ok(Outcome::Squeezed { before: c.bytes, after })
}

/// Resizing and frame rate for `level`, or `None` if the clip keeps both.
fn video_filter(p: &Params, probe: &Probe, cuda: bool) -> Option<String> {
    let mut f = Vec::new();
    if p.max_fps > 0.0 && probe.fps > p.max_fps + 0.5 {
        f.push(format!("fps={}", p.max_fps));
    }
    if p.max_height > 0 && probe.height > p.max_height {
        f.push(if cuda {
            format!("scale_cuda=w=-2:h={}:interp_algo=lanczos", p.max_height)
        } else {
            format!("scale=-2:{}:flags=lanczos", p.max_height)
        });
    }
    (!f.is_empty()).then(|| f.join(","))
}

/// The encoder's options for a variable bitrate around `kbps`.
fn encoder_args(encoder: &str, kbps: f64) -> Vec<String> {
    let mut a: Vec<&str> = vec!["-c:v", encoder];
    if encoder.ends_with("_nvenc") {
        a.extend(["-preset", NVENC_PRESET, "-tune", "hq", "-rc", "vbr"]);
    } else if encoder.ends_with("_amf") {
        a.extend(["-rc", "vbr_peak", "-quality", "quality"]);
    } else if encoder.ends_with("_qsv") {
        a.extend(["-preset", "slow"]);
    }
    if encoder.starts_with("h264") {
        a.extend(["-profile:v", "high"]);
    }
    let mut a: Vec<String> = a.into_iter().map(String::from).collect();
    a.extend(["-b:v".into(), format!("{kbps:.0}k"), "-maxrate".into(), format!("{:.0}k", kbps * MAXRATE), "-bufsize".into(), format!("{:.0}k", kbps * BUFSIZE)]);
    a
}

#[allow(clippy::too_many_arguments)]
fn ffmpeg_args(clip: &Path, part: &Path, level: Level, probe: &Probe, encoder: &str, kbps: f64, gpu_decode: bool, tags: &[(&str, String)]) -> Vec<String> {
    let cuda = gpu_decode && encoder.ends_with("_nvenc");
    let mut a: Vec<String> = Vec::new();
    if cuda {
        // Frames stay on the graphics card from decoding to encoding.
        a.extend(["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"].map(String::from));
    } else if gpu_decode {
        a.extend(["-hwaccel", "auto"].map(String::from));
    }
    a.extend(["-i".into(), clip.to_string_lossy().into_owned()]);
    // The video, every audio track as it is, the clip's tags and chapters.
    a.extend(["-map", "0:v:0", "-map", "0:a?", "-c:a", "copy", "-map_metadata", "0", "-map_chapters", "0"].map(String::from));
    if let Some(vf) = video_filter(level.params(), probe, cuda) {
        a.extend(["-vf".into(), vf]);
    }
    a.extend(encoder_args(encoder, kbps));
    a.extend(["-movflags", "+faststart+use_metadata_tags"].map(String::from));
    for (key, value) in tags {
        a.extend(["-metadata".into(), format!("{key}={value}")]);
    }
    a.extend(["-f".into(), "mp4".into(), part.to_string_lossy().into_owned()]);
    a
}

/// Runs ffmpeg at below-normal priority, reporting progress from 0 to 1.
fn run_ffmpeg(args: &[String], duration: f64, control: &Control, on_progress: &dyn Fn(f64)) -> Result<()> {
    if control.cancelled() {
        bail!("cancelled");
    }
    let mut child = ffmpeg::command()
        .creation_flags(CREATE_NO_WINDOW.0 | BELOW_NORMAL_PRIORITY_CLASS.0)
        .args(["-progress", "pipe:1", "-nostats", "-y"])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting ffmpeg.exe")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let mut stderr = child.stderr.take().context("no stderr")?;
    *control.child.lock().unwrap() = Some(child);
    // Cancelled while starting: the cancel found no ffmpeg to stop.
    if control.cancelled()
        && let Some(c) = control.child.lock().unwrap().as_mut()
    {
        let _ = c.kill();
    }
    // Errors are read on the side so a chatty ffmpeg can't block on a full pipe.
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let mut last = Instant::now() - Duration::from_secs(1);
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok())
            && last.elapsed() > Duration::from_millis(250)
        {
            on_progress((us / 1e6 / duration).clamp(0.0, 1.0));
            last = Instant::now();
        }
    }
    let status = control.child.lock().unwrap().take().context("cancelled")?.wait()?;
    let errors = errors.join().unwrap_or_default();
    if control.cancelled() {
        bail!("cancelled");
    }
    if !status.success() {
        bail!("{}", errors.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("ffmpeg failed"));
    }
    Ok(())
}

/// Checks the squeezed file before it replaces anything: a video, every audio
/// track, the same length. Returns its size.
fn verify(part: &Path, original: &Probe) -> Result<u64> {
    let p = probe(part).context("the squeezed file can't be read")?;
    ensure!(
        (p.duration - original.duration).abs() <= DURATION_SLACK,
        "the squeezed file is {:.2} s long instead of {:.2} s",
        p.duration,
        original.duration
    );
    ensure!(p.audio_streams == original.audio_streams, "the squeezed file has {} audio tracks instead of {}", p.audio_streams, original.audio_streams);
    Ok(fs::metadata(part)?.len())
}

/// Now as "2026-10-28T02:50:00Z".
fn iso_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}
