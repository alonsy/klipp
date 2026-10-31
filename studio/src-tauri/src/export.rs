//! Exports: a lossless trimmed copy, a high-quality re-encode, and a
//! size-limited one for sharing, all with the editor's track levels mixed down.
//! Each runs ffmpeg.exe on its own thread and reports progress as events.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::ffmpeg;
use crate::meta;

/// Exports go here (not shown as a game in the library).
pub const EXPORTS_DIR: &str = "Exports";

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Same quality, no re-encode, all tracks kept. Saved next to the original.
    Trim,
    /// Re-encoded H.264 at high quality with the mixed-down audio.
    Quality,
    /// Re-encoded to fit under `target_mb`, for sharing in chat apps.
    Share,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub path: String,
    pub start: f64,
    pub end: f64,
    pub mode: Mode,
    pub target_mb: f64,
    /// Volume per separate audio track (Game, Chat, Mic…), 1.0 = unchanged, 0 = muted.
    /// Empty when the clip only has the premixed track; then `mix_volume` applies.
    pub volumes: Vec<f32>,
    pub mix_volume: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    id: u64,
    fraction: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Done {
    id: u64,
    path: String,
    size_bytes: u64,
}

#[derive(Serialize, Clone)]
struct Failed {
    id: u64,
    error: String,
}

/// The running ffmpeg per export, so it can be cancelled.
type Running = Arc<Mutex<HashMap<u64, Arc<Mutex<Option<Child>>>>>>;

#[derive(Default)]
pub struct Exports {
    next: AtomicU64,
    running: Running,
}

impl Exports {
    pub fn start(&self, app: AppHandle, clips_dir: PathBuf, req: ExportRequest) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let slot = Arc::new(Mutex::new(None));
        self.running.lock().unwrap().insert(id, slot.clone());
        let running = self.running.clone();
        std::thread::spawn(move || {
            let progress = |fraction: f64| {
                let _ = app.emit("export-progress", Progress { id, fraction });
            };
            let result = run(&clips_dir, &req, ffmpeg::h264_encoder(), &slot, &progress);
            running.lock().unwrap().remove(&id);
            let _ = match result {
                Ok(path) => {
                    let size_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    app.emit("export-done", Done { id, path: path.to_string_lossy().into_owned(), size_bytes })
                }
                Err(e) => app.emit("export-failed", Failed { id, error: format!("{e:#}") }),
            };
        });
        id
    }

    /// Some export is still running.
    pub fn busy(&self) -> bool {
        !self.running.lock().unwrap().is_empty()
    }

    pub fn cancel(&self, id: u64) {
        if let Some(slot) = self.running.lock().unwrap().get(&id)
            && let Some(child) = slot.lock().unwrap().as_mut()
        {
            let _ = child.kill();
        }
    }
}

struct Probe {
    height: u32,
    fps: f64,
    audio_streams: usize,
}

fn probe(path: &Path) -> Result<Probe> {
    let out = ffmpeg::probe_command()
        .args(["-show_entries", "stream=codec_type,height,avg_frame_rate", "-of", "json"])
        .arg(path)
        .output()?;
    if !out.status.success() {
        bail!("couldn't read {}", path.display());
    }
    let json: Value = serde_json::from_slice(&out.stdout)?;
    let streams = json["streams"].as_array().context("no streams")?;
    let video = streams.iter().find(|s| s["codec_type"] == "video").context("no video stream")?;
    let fps = video["avg_frame_rate"]
        .as_str()
        .and_then(|r| r.split_once('/'))
        .and_then(|(n, d)| Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok()?.max(1.0)))
        .unwrap_or(60.0);
    Ok(Probe {
        height: video["height"].as_u64().unwrap_or(1080) as u32,
        fps,
        audio_streams: streams.iter().filter(|s| s["codec_type"] == "audio").count(),
    })
}

/// The audio part of the filter graph, producing `[aout]`, or `None` for no audio.
/// Track 0 is the premix; separate tracks (if any) are 1..n.
fn audio_filter(audio_streams: usize, volumes: &[f32], mix_volume: f32) -> Option<String> {
    audio_filter_from(0, audio_streams, volumes, mix_volume)
}

/// `audio_filter` for the clip as ffmpeg's input number `input` (shorts take
/// their pictures from input 0 and the clip's audio from input 1).
pub(crate) fn audio_filter_from(input: usize, audio_streams: usize, volumes: &[f32], mix_volume: f32) -> Option<String> {
    if audio_streams == 0 {
        return None;
    }
    if audio_streams == 1 || volumes.is_empty() {
        return (mix_volume > 0.0).then(|| format!("[{input}:a:0]volume={mix_volume:.3}[aout]"));
    }
    let parts: Vec<(usize, f32)> =
        volumes.iter().copied().enumerate().filter(|&(k, v)| v > 0.0 && k + 1 < audio_streams).collect();
    if parts.is_empty() {
        return None;
    }
    let mut f = String::new();
    for &(k, v) in &parts {
        f.push_str(&format!("[{input}:a:{}]volume={v:.3}[t{k}];", k + 1));
    }
    for &(k, _) in &parts {
        f.push_str(&format!("[t{k}]"));
    }
    // Sum without amix's automatic level reduction; the limiter catches peaks.
    f.push_str(&format!("amix=inputs={}:normalize=0:dropout_transition=0,alimiter=limit=0.95:level=false[aout]", parts.len()));
    Some(f)
}

/// The share export never needs more than this (kbps).
const MAX_SHARE_KBPS: f64 = 12_000.0;
/// A high quality copy of a 1080p clip on encoders that work to a bitrate (kbps).
const QUALITY_KBPS_1080P: f64 = 16_000.0;

/// Resolution and frame rate for a video bitrate, so a small file still looks sharp.
fn size_ladder(video_kbps: f64, src: &Probe) -> (u32, f64) {
    let (height, fps): (u32, f64) = match video_kbps {
        k if k >= 5000.0 => (1080, 60.0),
        k if k >= 2800.0 => (720, 60.0),
        k if k >= 1500.0 => (720, 30.0),
        k if k >= 800.0 => (540, 30.0),
        _ => (480, 30.0),
    };
    (height.min(src.height), fps.min(src.fps))
}

#[derive(Clone, Copy, PartialEq)]
enum Aim {
    /// High quality; `kbps` is the guide on encoders that need one.
    Quality,
    /// A bitrate around `kbps`.
    Size,
    /// The same, for a try that came out too big.
    SmallerSize,
}

/// `encoder`'s options for `aim`.
fn video_args(encoder: &str, kbps: f64, aim: Aim) -> Vec<String> {
    let (b, max, buf) = (format!("{kbps:.0}k"), format!("{:.0}k", kbps * 1.4), format!("{:.0}k", kbps * 2.0));
    let rates = ["-b:v", &b, "-maxrate", &max, "-bufsize", &buf];
    let mut a = vec!["-c:v", encoder];
    match encoder {
        "h264_nvenc" if aim == Aim::Quality => a.extend(["-preset", "p7", "-tune", "hq", "-rc", "vbr", "-cq", "21", "-b:v", "0"]),
        "h264_nvenc" => {
            a.extend(["-preset", "p7", "-tune", "hq", "-rc", "vbr", "-multipass", "fullres"]);
            a.extend(rates);
        }
        // Not qvbr, as the recorder: it goes far past any bitrate. vbr_peak looks
        // best, but on busy clips it stops shrinking when asked for less.
        "h264_amf" => {
            a.extend(["-quality", "quality", "-rc", if aim == Aim::SmallerSize { "vbr_latency" } else { "vbr_peak" }]);
            a.extend(rates);
        }
        "h264_qsv" => {
            a.extend(["-preset", "slow"]);
            a.extend(rates);
        }
        _ => a.extend(["-rate_control", "pc_vbr", "-b:v", &b]),
    }
    // Windows' own encoder refuses a profile by name.
    if encoder != "h264_mf" {
        a.extend(["-profile:v", "high"]);
    }
    a.into_iter().map(String::from).collect()
}

/// Runs one export, reporting overall progress from 0 to 1. Returns the output file.
fn run(clips_dir: &Path, req: &ExportRequest, encoder: &str, slot: &Mutex<Option<Child>>, on_progress: &dyn Fn(f64)) -> Result<PathBuf> {
    let src = PathBuf::from(&req.path);
    let duration = (req.end - req.start).max(0.1);
    let stem = src.file_stem().context("no file name")?.to_string_lossy().into_owned();
    // Progress of one ffmpeg run, mapped into a slice of the overall bar.
    let progress = |base: f64, span: f64| move |f: f64| on_progress((base + f * span).min(1.0));
    let seek = |args: &mut Vec<String>| {
        args.extend(["-ss".into(), format!("{:.3}", req.start), "-i".into(), src.to_string_lossy().into_owned()]);
        args.extend(["-t".into(), format!("{duration:.3}")]);
    };

    if req.mode == Mode::Trim {
        let out = unique(&src.with_file_name(format!("{stem}_trim.mp4")));
        let mut args = Vec::new();
        seek(&mut args);
        // Video and audio only: the chapter track can't be copied, but ffmpeg
        // rewrites the chapters themselves for the trimmed range.
        args.extend(["-map", "0:v", "-map", "0:a", "-c", "copy", "-movflags", "+faststart"].map(String::from));
        run_ffmpeg(args, &out, duration, slot, progress(0.0, 1.0))?;
        copy_info(clips_dir, &src, &out, req.start, req.end);
        return Ok(out);
    }

    let probe = probe(&src)?;
    let audio = audio_filter(probe.audio_streams, &req.volumes, req.mix_volume);
    let dir = clips_dir.join(EXPORTS_DIR);
    fs::create_dir_all(&dir)?;
    let base_args = |height: u32, fps: f64, video: &[String], audio_kbps: u32| {
        let mut args = Vec::new();
        seek(&mut args);
        let mut graph = format!("[0:v:0]scale=-2:{height}:flags=lanczos,fps={fps}[vout]");
        if let Some(a) = &audio {
            graph = format!("{graph};{a}");
        }
        args.extend(["-filter_complex".into(), graph, "-map".into(), "[vout]".into()]);
        args.extend(video.iter().cloned());
        args.extend(["-pix_fmt", "yuv420p"].map(String::from));
        if audio.is_some() {
            args.extend(["-map".into(), "[aout]".into(), "-c:a".into(), "aac".into(), "-b:a".into(), format!("{audio_kbps}k")]);
        }
        args.extend(["-movflags", "+faststart"].map(String::from));
        args
    };

    if req.mode == Mode::Quality {
        let out = unique(&dir.join(format!("{stem}_export.mp4")));
        let video = video_args(encoder, QUALITY_KBPS_1080P * probe.height as f64 / 1080.0, Aim::Quality);
        run_ffmpeg(base_args(probe.height, probe.fps, &video, 192), &out, duration, slot, progress(0.0, 1.0))?;
        return Ok(out);
    }

    // Share: fit under the target size. Aim a little under it, check, and
    // retry smaller if the encoder overshot.
    let target_bytes = req.target_mb * 1e6;
    let audio_kbps = if audio.is_none() { 0 } else if req.target_mb <= 10.0 { 96 } else { 128 };
    let out = unique(&dir.join(format!("{stem}_share.mp4")));
    let mut budget = 0.94;
    for attempt in 0..3 {
        // Short clips would get a huge budget the encoder overshoots; past this
        // 1080p60 looks no better, so they come out small on the first try.
        let video_kbps = (target_bytes * 8.0 * budget / duration / 1000.0 - audio_kbps as f64).clamp(150.0, MAX_SHARE_KBPS);
        let (height, fps) = size_ladder(video_kbps, &probe);
        let video = video_args(encoder, video_kbps, if attempt == 0 { Aim::Size } else { Aim::SmallerSize });
        let span = 1.0 / (attempt + 1) as f64;
        run_ffmpeg(base_args(height, fps, &video, audio_kbps), &out, duration, slot, progress(1.0 - span, span))?;
        if fs::metadata(&out)?.len() as f64 <= target_bytes {
            return Ok(out);
        }
        budget *= 0.85;
    }
    let mb = fs::metadata(&out)?.len() as f64 / 1e6;
    bail!("couldn't get it under {} MB (best: {mb:.1} MB); try trimming it shorter", req.target_mb)
}

/// Runs ffmpeg writing `out` (via a temporary name), reporting progress from 0 to 1.
fn run_ffmpeg(args: Vec<String>, out: &Path, duration: f64, slot: &Mutex<Option<Child>>, on_progress: impl Fn(f64)) -> Result<()> {
    let part = out.with_extension("part.mp4");
    let mut child = ffmpeg::command()
        .args(["-progress", "pipe:1", "-nostats", "-y"])
        .args(&args)
        .arg(&part)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().context("no stdout")?;
    let mut stderr = child.stderr.take().context("no stderr")?;
    *slot.lock().unwrap() = Some(child);
    // Read errors on the side so a chatty ffmpeg can't block on a full pipe.
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let mut last = Instant::now() - Duration::from_secs(1);
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok())
            && last.elapsed() > Duration::from_millis(100)
        {
            on_progress((us / 1e6 / duration).clamp(0.0, 1.0));
            last = Instant::now();
        }
    }
    let status = slot.lock().unwrap().take().context("export was cancelled")?.wait()?;
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        let _ = fs::remove_file(&part);
        let detail = errors.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("export was cancelled");
        bail!("{detail}");
    }
    fs::rename(&part, out)?;
    on_progress(1.0);
    Ok(())
}

/// The trimmed copy gets the original's info with events moved and cut to the new range.
fn copy_info(clips_dir: &Path, src: &Path, out: &Path, start: f64, end: f64) {
    let Some(mut info) = meta::read_info(clips_dir, src) else { return };
    if let Some(events) = info["events"].as_array_mut() {
        events.retain(|e| e["at_secs"].as_f64().is_some_and(|t| (start..=end).contains(&t)));
        for e in events.iter_mut() {
            let t = e["at_secs"].as_f64().unwrap_or(0.0) - start;
            e["at_secs"] = serde_json::json!((t * 1000.0).round() / 1000.0);
        }
    }
    info["duration_secs"] = serde_json::json!(((end - start) * 1000.0).round() / 1000.0);
    info["favorite"] = serde_json::json!(false);
    if let Some(o) = info.as_object_mut() {
        // The copy is already trimmed; its levels start fresh too.
        o.remove("edit");
    }
    let _ = meta::update_info(clips_dir, out, |v| *v = info);
}
pub(crate) fn unique(path: &Path) -> PathBuf {
    let (stem, ext) = (path.file_stem().unwrap_or_default().to_string_lossy(), path.extension().unwrap_or_default().to_string_lossy());
    let mut candidate = path.to_path_buf();
    let mut n = 2;
    while candidate.exists() {
        candidate = path.with_file_name(format!("{stem}_{n}.{ext}"));
        n += 1;
    }
    candidate
}
