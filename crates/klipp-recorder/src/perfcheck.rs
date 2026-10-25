//! The first time a version starts (after an update, or after setup), before
//! the buffer starts: a few seconds of recording at the saved video settings,
//! measured like setup's system check. When they don't run smoothly on this
//! PC, the first lighter setting that does goes into settings.toml, and the
//! Klipp window says what changed: a card that can't keep up stutters in
//! every match (FIX-17). Once per version; while a game keeps the card busy
//! it waits for the next start, since the numbers would be the game's.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use klipp_media::VideoCodec;

use crate::log::{info, warning};
use crate::settings::{Codec, Resolution, Settings};
use crate::tuning::{Adjusted, Tuning};

/// The 3D engine busier than this (percent) before Klipp records: a game is running.
const BUSY_3D: f64 = 30.0;

/// Checks the saved video settings if this version hasn't yet. Returns the
/// settings to record with: lighter ones if it changed them.
pub fn run(path: &Path, settings: Settings) -> Settings {
    let mut tuning = Tuning::load();
    if tuning.checked {
        return settings;
    }
    if let Some(busy) = card_busy() {
        info!("check after the update: the graphics card is {busy:.0}% busy (a game?); checking at the next start");
        return settings;
    }
    let video = &settings.video;
    let (_, screen) = crate::monitors::find(&video.monitor);
    let screen_height = screen.map_or(1080, |m| m.height);
    let height = match video.resolution {
        Resolution::Native => screen_height,
        Resolution::P1440 => 1440.min(screen_height),
        Resolution::P1080 => 1080.min(screen_height),
        Resolution::P720 => 720.min(screen_height),
    };
    let codec = if video.codec == Codec::Av1 { VideoCodec::Av1 } else { VideoCodec::H264 };
    let measure = |h: u32, fps: u32| crate::probe::measure("check after the update", &video.monitor, &video.gpu, codec, h, screen_height, fps).smooth;
    if measure(height, video.fps) {
        tuning.mark_checked(None);
        return settings;
    }
    let Some((h, fps)) = lighter(height, video.fps).into_iter().find(|&(h, fps)| measure(h, fps)) else {
        info!("check after the update: nothing lighter ran smoothly either; keeping the settings (the watchdog steps in if it must)");
        tuning.mark_checked(None);
        return settings;
    };
    // The size only changes when it has to.
    let resolution = (h != height).then(|| if h >= screen_height { "native".to_string() } else { format!("{h}p") });
    let adjusted = Adjusted { from: describe(height, video.fps), to: describe(h, fps) };
    if let Err(e) = save(path, resolution.as_deref(), fps) {
        warning!("check after the update: couldn't change the settings: {e:#}");
        tuning.mark_checked(None);
        return settings;
    }
    info!("check after the update: {} wasn't smooth on this PC; recording {} from now on", adjusted.from, adjusted.to);
    tuning.mark_checked(Some(adjusted));
    Settings::load_or_create(path).unwrap_or(settings)
}

/// Lighter settings to try, best first, in setup's order: the bigger size
/// before the higher rate (the same size at 60 fps, then 1440p, then 1080p).
fn lighter(height: u32, fps: u32) -> Vec<(u32, u32)> {
    let sizes = [Some(height), (height > 1440).then_some(1440), (height > 1080).then_some(1080)];
    let rates = [Some(fps), (fps > 60).then_some(60)];
    sizes.into_iter().flatten().flat_map(|h| rates.into_iter().flatten().map(move |r| (h, r))).skip(1).collect()
}

fn describe(height: u32, fps: u32) -> String {
    format!("{height}p at {fps} fps")
}

/// How busy the 3D engine is over a second, if a game seems to be keeping it busy.
fn card_busy() -> Option<f64> {
    let mut load = crate::gpuload::GpuLoad::start()?;
    std::thread::sleep(Duration::from_secs(1));
    let busy = load.sample().into_iter().find(|e| e.kind == "3D").map_or(0.0, |e| e.total);
    (busy > BUSY_3D).then_some(busy)
}

/// Writes the new rate (and size) into settings.toml, keeping everything else (comments too).
fn save(path: &Path, resolution: Option<&str>, fps: u32) -> Result<()> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = text.parse().context("settings.toml")?;
    if !doc.contains_table("video") {
        doc["video"] = toml_edit::table();
    }
    if let Some(r) = resolution {
        doc["video"]["resolution"] = toml_edit::value(r);
    }
    doc["video"]["fps"] = toml_edit::value(fps as i64);
    std::fs::write(path, doc.to_string()).with_context(|| format!("writing {}", path.display()))
}
