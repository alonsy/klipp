//! `klipp-recorder --probe`: what this PC can record, as JSON, for the setup
//! in the Klipp window. Encoders are tested for real (opened at each size and
//! fed a few frames), since what a graphics card supports depends on its
//! generation and driver. Then the whole recording path (capture, convert,
//! encode) runs for a few seconds at each candidate setting, and setup picks
//! the best one that leaves the card plenty of room for a game.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Context, Result};
use klipp_media::{HwDevice, Packet, VideoCodec, VideoEncoder, VideoEncoderConfig, Vendor};
use serde::Serialize;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::gpu::Gpu;
use crate::monitors;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    gpus: Vec<GpuInfo>,
    /// Every monitor, the main display first.
    monitors: Vec<monitors::Monitor>,
    /// The monitor that was tested (the main display unless `--monitor`).
    monitor: Option<Monitor>,
    /// The graphics card that records it.
    recording_gpu: Option<String>,
    /// "nvidia", "amd" or "intel"; none if the card has no encoder Klipp can use.
    recording_vendor: Option<&'static str>,
    /// "NVENC", "AMD AMF" or "Intel Quick Sync".
    encoder: Option<&'static str>,
    encoders: Vec<EncoderCheck>,
    ram_mb: u64,
    /// A laptop (it has a battery): setup suggests a save key that needs no Fn.
    laptop: bool,
    /// CS2 is installed (auto-clips can work).
    cs2: bool,
    /// The recording path at each candidate setting, best first.
    performance: Vec<PerfCheck>,
    /// What setup should preselect: the best smooth candidate.
    recommended: Option<Recommended>,
    /// League of Legends is installed, and Rocket League has run here.
    league: bool,
    rocket_league: bool,
    /// Why encoders couldn't be tested at all, if so.
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PerfCheck {
    height: u32,
    fps: u32,
    codec: &'static str,
    /// Frames encoded, and how many were late or left out.
    frames: u64,
    late: u64,
    dropped: u64,
    /// Average and longest milliseconds a frame took to hand to the encoder, and to convert.
    send_ms: f64,
    send_max_ms: f64,
    convert_ms: f64,
    /// The part of a frame's time the card's work took: room left for a game.
    busy: f64,
    pub smooth: bool,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Recommended {
    height: u32,
    fps: u32,
}

/// How long each candidate records, after a moment to settle.
const PERF_SETTLE: Duration = Duration::from_millis(700);
const PERF_RUN: Duration = Duration::from_secs(3);
/// Smooth means nearly nothing late or left out, with the work taking at
/// most this share of a frame's time: an idle desktop is easy, and a game
/// at full load makes every step wait (FIX-17), so the room matters.
const MAX_BUSY: f64 = 0.25;
const MAX_LATE: f64 = 0.02;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuInfo {
    name: String,
    vendor: &'static str,
    vram_mb: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Monitor {
    width: u32,
    height: u32,
    refresh_hz: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EncoderCheck {
    codec: &'static str,
    width: u32,
    height: u32,
    ok: bool,
    error: Option<String>,
}

fn vendor_key(v: Vendor) -> &'static str {
    match v {
        Vendor::Nvidia => "nvidia",
        Vendor::Amd => "amd",
        Vendor::Intel => "intel",
    }
}

/// Prints the report as JSON. `monitor` is a `video.monitor` id (main display
/// if empty); `gpu` tests another card by name (the `video.gpu` setting).
pub fn run(monitor: &str, gpu: &str) -> Result<()> {
    // The report is read from standard output; the recording path's log lines stay in the file.
    crate::log::to_file_only();
    let (handle, chosen) = monitors::find(monitor);
    let mut report = Report {
        gpus: gpus(),
        monitors: monitors::list(),
        monitor: chosen.as_ref().map(|m| Monitor { width: m.width, height: m.height, refresh_hz: m.refresh_hz }),
        recording_gpu: None,
        recording_vendor: None,
        encoder: None,
        encoders: Vec::new(),
        ram_mb: ram_mb(),
        laptop: has_battery(),
        cs2: crate::cs2::installed(),
        performance: Vec::new(),
        recommended: None,
        league: crate::league::installed(),
        rocket_league: crate::rocketleague::installed(),
        error: None,
    };
    let size = chosen.map_or((1920, 1080), |m| (m.width, m.height));
    match test_encoders(handle, gpu, size, &mut report) {
        Ok(checks) => report.encoders = checks,
        Err(e) => report.error = Some(format!("{e:#}")),
    }
    let refresh = report.monitor.as_ref().map_or(60, |m| m.refresh_hz);
    test_performance(monitor, gpu, size.1, refresh, &mut report);
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

/// The recording path at each candidate: the screen's size (or 1440p on a
/// bigger screen) and 1080p, at 120 fps (on a 120 Hz screen or faster) and
/// 60. The first smooth one, in that order, is recommended.
fn test_performance(monitor: &str, gpu: &str, screen_height: u32, refresh: u32, report: &mut Report) {
    let works = |codec: &str, h: u32| report.encoders.iter().any(|e| e.codec == codec && e.height == h && e.ok);
    let mut heights = vec![screen_height.min(1440)];
    if screen_height > 1080 {
        heights.push(1080);
    }
    let rates: &[u32] = if refresh >= 120 { &[120, 60] } else { &[60] };
    for &height in &heights {
        let codec = if works("av1", height) {
            VideoCodec::Av1
        } else if works("h264", height) {
            VideoCodec::H264
        } else {
            continue;
        };
        for &fps in rates {
            let check = measure("probe", monitor, gpu, codec, height, screen_height, fps);
            if check.smooth && report.recommended.is_none() {
                report.recommended = Some(Recommended { height, fps });
            }
            report.performance.push(check);
        }
    }
    match &report.recommended {
        Some(r) => crate::log::info!("probe: recommended {}p at {} fps", r.height, r.fps),
        None => crate::log::info!("probe: nothing ran smoothly; setup keeps its usual choice"),
    }
}

/// The recording path at one setting, measured and logged (`who`: "probe", "check after the update").
pub(crate) fn measure(who: &str, monitor: &str, gpu: &str, codec: VideoCodec, height: u32, screen_height: u32, fps: u32) -> PerfCheck {
    let mut check = record_for_a_moment(monitor, gpu, codec, (height < screen_height).then_some(height), fps);
    check.height = height;
    crate::log::info!(
        "{who}: {height}p {fps} fps {}: {} frames, {} late, {} left out | send {:.1}/{:.0} ms, convert {:.1} ms | card busy {:.0}% of each frame{}{}",
        check.codec,
        check.frames,
        check.late,
        check.dropped,
        check.send_ms,
        check.send_max_ms,
        check.convert_ms,
        check.busy * 100.0,
        if check.smooth { " | smooth" } else { "" },
        check.error.as_ref().map(|e| format!(" | {e}")).unwrap_or_default()
    );
    check
}

/// Records into a throwaway buffer for a few seconds and measures it.
fn record_for_a_moment(monitor: &str, gpu: &str, codec: VideoCodec, height: Option<u32>, fps: u32) -> PerfCheck {
    use crate::video::{VideoPipeline, VideoSettings};
    let codec_name = if codec == VideoCodec::Av1 { "av1" } else { "h264" };
    let mut check = PerfCheck {
        height: 0,
        fps,
        codec: codec_name,
        frames: 0,
        late: 0,
        dropped: 0,
        send_ms: 0.0,
        send_max_ms: 0.0,
        convert_ms: 0.0,
        busy: 0.0,
        smooth: false,
        error: None,
    };
    let shared = Arc::new(crate::state::Shared::new(crate::replay::ReplayBuffer::new(10 * crate::clock::SECOND, 64 << 20)));
    let settings = VideoSettings {
        codec,
        fps,
        height,
        quality: 30,
        max_mbps: 40,
        monitor: monitor.to_string(),
        capture: crate::settings::CaptureMethod::Auto,
        gpu: gpu.to_string(),
    };
    let pipeline = match VideoPipeline::start(settings, shared.clone()) {
        Ok(p) => p,
        Err(e) => {
            check.error = Some(format!("{e:#}"));
            return check;
        }
    };
    std::thread::sleep(PERF_SETTLE);
    let counts = |s: &crate::state::Shared| {
        (s.frames.load(Ordering::Relaxed), s.late_frames.load(Ordering::Relaxed), s.skipped_ticks.load(Ordering::Relaxed))
    };
    let before = counts(&shared);
    shared.steps.take();
    std::thread::sleep(PERF_RUN);
    let after = counts(&shared);
    let steps = shared.steps.take();
    drop(pipeline);
    check.frames = after.0 - before.0;
    check.late = after.1 - before.1;
    check.dropped = after.2 - before.2;
    check.send_ms = steps.send.0;
    check.send_max_ms = steps.send.1;
    check.convert_ms = steps.convert.0;
    check.busy = (steps.send.0 + steps.convert.0) / (1000.0 / fps as f64);
    let due = (check.frames + check.dropped).max(1) as f64;
    check.smooth = check.frames > 0 && (check.late + check.dropped) as f64 / due <= MAX_LATE && check.busy <= MAX_BUSY;
    check
}

/// Each encoder at the screen's size and the common smaller ones.
fn test_encoders(monitor: HMONITOR, gpu_name: &str, (w, h): (u32, u32), report: &mut Report) -> Result<Vec<EncoderCheck>> {
    let gpu = if gpu_name.is_empty() { Gpu::for_monitor(monitor)? } else { Gpu::by_name(gpu_name)? };
    report.recording_gpu = Some(gpu.adapter_name.clone());
    let vendor = gpu.vendor.with_context(|| format!("{} has no video encoder Klipp can use", gpu.adapter_name))?;
    report.recording_vendor = Some(vendor_key(vendor));
    report.encoder = Some(vendor.encoder());
    let device = HwDevice::from_d3d11(&gpu.device, &gpu.context)?;
    let mut sizes = vec![(w & !1, h & !1)];
    for target in [1440, 1080, 720] {
        if target < h {
            sizes.push((((w as u64 * target as u64 / h as u64) as u32) & !1, target));
        }
    }
    let mut checks = Vec::new();
    for (codec, name) in [(VideoCodec::Av1, "av1"), (VideoCodec::H264, "h264")] {
        for &(width, height) in &sizes {
            let cfg = VideoEncoderConfig { vendor, codec, width, height, fps: 60, quality: 30, max_bitrate: 40_000_000 };
            let result = encode_some(&device, &cfg);
            checks.push(EncoderCheck { codec: name, width, height, ok: result.is_ok(), error: result.err().map(|e| format!("{e:#}")) });
        }
    }
    Ok(checks)
}

/// Opens the encoder and pushes a few frames through it: some cards open an
/// encoder they then can't run.
fn encode_some(device: &HwDevice, cfg: &VideoEncoderConfig) -> Result<()> {
    let mut encoder = VideoEncoder::new(device, cfg)?;
    let mut packets = 0;
    let mut count = |_: &mut Packet| -> Result<()> {
        packets += 1;
        Ok(())
    };
    let mut frame = encoder.alloc_frame()?;
    for i in 0..8 {
        encoder.encode(&mut frame, i, &mut count)?;
    }
    drop(frame);
    encoder.finish(&mut count)?;
    anyhow::ensure!(packets > 0, "the encoder produced no video");
    Ok(())
}

fn gpus() -> Vec<GpuInfo> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return Vec::new() };
    let mut out = Vec::new();
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else { break };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        let vendor = Vendor::from_pci_id(desc.VendorId).map_or("other", vendor_key);
        let gpu = GpuInfo { name: String::from_utf16_lossy(&desc.Description[..len]), vendor, vram_mb: desc.DedicatedVideoMemory as u64 / 1_048_576 };
        // The same card can be listed more than once (e.g. with virtual displays).
        if !out.iter().any(|g: &GpuInfo| g.name == gpu.name && g.vram_mb == gpu.vram_mb) {
            out.push(gpu);
        }
    }
    out
}

/// Windows reports a system battery: a laptop (or a tablet).
fn has_battery() -> bool {
    const NO_BATTERY: u8 = 128;
    const UNKNOWN: u8 = 255;
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && !matches!(status.BatteryFlag, NO_BATTERY | UNKNOWN)
}

fn ram_mb() -> u64 {
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() { status.ullTotalPhys / 1_048_576 } else { 0 }
}
