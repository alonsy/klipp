//! `klipp-recorder --probe`: what this PC can record, as JSON, for the setup
//! in the Klipp window. Encoders are tested for real (opened at each size and
//! fed a few frames), since what a graphics card supports depends on its
//! generation and driver. Then the whole recording path (capture, convert,
//! encode) runs for a few seconds at each candidate setting, and setup picks
//! the best one that leaves the card plenty of room for a game.
//!
//! Each run also measures what the recording itself costs this PC (the PC
//! test): Klipp's share of the graphics card's engines, of the CPU, its
//! memory and the bitrate it made, next to a snapshot of how busy the PC
//! was before. The Klipp window keeps it (pc-test.json) to show what other
//! settings would cost.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use klipp_media::{HwDevice, Packet, VideoCodec, VideoEncoder, VideoEncoderConfig, Vendor};
use serde::Serialize;
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetSystemTimes};
use windows::core::BOOL;

use crate::gpu::Gpu;
use crate::gpuload::{Engine, GpuLoad};
use crate::monitors;
use crate::settings::CaptureMethod;
use crate::stats;

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
    /// The PC just before the test: memory, CPU and graphics card, every app included.
    system: Snapshot,
    /// How long the whole check took, in seconds.
    seconds: f64,
}

/// How busy the PC was over a second before the test (every app, so a
/// recorder that is already running counts too).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    ram_total_mb: u64,
    /// In use by everything now (total minus available), MB.
    ram_used_mb: u64,
    cpu_name: String,
    logical_cores: u32,
    /// The whole machine's CPU use, percent.
    cpu: f64,
    /// The card that records, and its own memory (MB).
    gpu_name: Option<String>,
    vram_mb: Option<u64>,
    /// Graphics memory in use now (the busiest card), MB.
    vram_used_mb: Option<u64>,
    /// The whole card's busiest 3D engine and video encoder, percent (none without GPU counters).
    gpu_3d: Option<f64>,
    gpu_encode: Option<f64>,
    /// This test's own memory before it recorded anything (private bytes, MB).
    process_mb: f64,
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
    /// The recorded width (the height is above).
    width: u32,
    /// "auto", or "light" (the light capture with the encoder's fastest preset: Performance+).
    capture: &'static str,
    light_encoder: bool,
    /// What recording cost while it was measured; none if it didn't start.
    usage: Option<Usage>,
}

/// This process's own use while it recorded (nothing else runs in it then).
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Usage {
    /// How long it was measured.
    seconds: f64,
    /// Klipp's share of the card's 3D engine (capture and colour conversion),
    /// video encoder and copy engine, percent; none without GPU counters.
    gpu_3d: Option<f64>,
    gpu_encode: Option<f64>,
    gpu_copy: Option<f64>,
    /// The whole card's busiest 3D engine meanwhile (every app, this test included).
    card_3d: Option<f64>,
    /// CPU time used: percent of the whole machine, and of one core.
    cpu: f64,
    cpu_core: f64,
    /// Memory (MB) before the recording started, and while it ran: private
    /// bytes, private working set (Task Manager's "Memory"), the replay
    /// buffer's part, and the rest (what Klipp needs without the buffer).
    before_mb: f64,
    private_mb: f64,
    working_set_mb: f64,
    buffer_mb: f64,
    base_mb: f64,
    /// Graphics memory the recording used, MB.
    vram_mb: u64,
    /// The bitrate it made, Mbit/s: a still desktop makes far less than a game.
    mbps: f64,
    /// Frames with something new on screen (the rest repeat the one before).
    new_frames: u64,
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
    let started = Instant::now();
    // Before anything of the test's own runs on the card.
    let system = snapshot();
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
        system,
        seconds: 0.0,
    };
    let size = chosen.map_or((1920, 1080), |m| (m.width, m.height));
    match test_encoders(handle, gpu, size, &mut report) {
        Ok(checks) => report.encoders = checks,
        Err(e) => report.error = Some(format!("{e:#}")),
    }
    // The card that records (or the first one), with its memory.
    let card = report.gpus.iter().find(|g| Some(&g.name) == report.recording_gpu.as_ref()).or(report.gpus.first());
    report.system.gpu_name = card.map(|g| g.name.clone());
    report.system.vram_mb = card.map(|g| g.vram_mb);
    report.system.process_mb = round(stats::mb(stats::sample().commit), 0);
    let refresh = report.monitor.as_ref().map_or(60, |m| m.refresh_hz);
    test_performance(monitor, gpu, size.1, refresh, &mut report);
    report.seconds = round(started.elapsed().as_secs_f64(), 1);
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

/// Memory, CPU and the graphics card over one second, before the test's own work.
fn snapshot() -> Snapshot {
    let mut load = GpuLoad::start();
    let before = system_times();
    std::thread::sleep(Duration::from_secs(1));
    let after = system_times();
    let engines = load.as_mut().map(|l| l.sample());
    let engine = |kind: &str| engines.as_ref().map(|list| round(list.iter().find(|e| e.kind == kind).map_or(0.0, |e| e.total), 1));
    let cpu = match (before, after) {
        (Some((idle0, all0)), Some((idle1, all1))) if all1 > all0 => 100.0 * (1.0 - (idle1 - idle0) as f64 / (all1 - all0) as f64),
        _ => 0.0,
    };
    let (ram_total_mb, ram_used_mb) = ram_use_mb();
    Snapshot {
        ram_total_mb,
        ram_used_mb,
        cpu_name: crate::sysinfo::cpu_name(),
        logical_cores: logical_cores(),
        cpu: round(cpu.clamp(0.0, 100.0), 1),
        gpu_name: None,
        vram_mb: None,
        vram_used_mb: crate::gpuload::vram_in_use_mb(),
        gpu_3d: engine("3D"),
        gpu_encode: engine("VideoEncode"),
        process_mb: 0.0,
    }
}

/// The machine's idle time and all its time so far (kernel time includes idle), in 100 ns.
fn system_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    let ticks = |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
    Some((ticks(idle), ticks(kernel) + ticks(user)))
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryProcessCycleTime(process: HANDLE, cycles: *mut u64) -> BOOL;
}

/// The CPU cycles this process has used so far and the time-stamp counter now,
/// in the same unit: exact, where the process times only count the clock
/// ticks (every 15.6 ms) at which one of its threads happened to be running.
fn cycles() -> Option<(u64, u64)> {
    let mut used = 0u64;
    unsafe { QueryProcessCycleTime(GetCurrentProcess(), &mut used) }.ok().ok()?;
    #[cfg(target_arch = "x86_64")]
    return Some((used, unsafe { core::arch::x86_64::_rdtsc() }));
    #[cfg(not(target_arch = "x86_64"))]
    None
}

fn logical_cores() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

fn round(x: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    (x * f).round() / f
}

/// The recording path at each candidate: the screen's size (or 1440p on a
/// bigger screen) and 1080p, at 120 fps (on a 120 Hz screen or faster) and
/// 60. The first smooth one, in that order, is recommended. Then once as
/// Performance+ records (1080p at 60 fps, the light capture, the encoder's
/// fastest preset), for the PC test only: setup's pick stays one of the others.
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
            let check = measure("probe", monitor, gpu, CaptureMethod::Auto, codec, height, screen_height, fps);
            if check.smooth && report.recommended.is_none() {
                report.recommended = Some(Recommended { height, fps });
            }
            report.performance.push(check);
        }
    }
    let light = screen_height.min(1080);
    let codec = if works("av1", light) { Some(VideoCodec::Av1) } else if works("h264", light) { Some(VideoCodec::H264) } else { None };
    if let Some(codec) = codec {
        let check = measure("probe", monitor, gpu, CaptureMethod::Light, codec, light, screen_height, 60);
        report.performance.push(check);
    }
    match &report.recommended {
        Some(r) => crate::log::info!("probe: recommended {}p at {} fps", r.height, r.fps),
        None => crate::log::info!("probe: nothing ran smoothly; setup keeps its usual choice"),
    }
}

/// The recording path at one setting, measured and logged (`who`: "probe", "check after the update").
#[allow(clippy::too_many_arguments)]
pub(crate) fn measure(who: &str, monitor: &str, gpu: &str, capture: CaptureMethod, codec: VideoCodec, height: u32, screen_height: u32, fps: u32) -> PerfCheck {
    let mut check = record_for_a_moment(monitor, gpu, capture, codec, (height < screen_height).then_some(height), fps);
    check.height = height;
    let usage = check.usage.as_ref().map_or(String::new(), |u| {
        let gpu = match (u.gpu_3d, u.gpu_encode) {
            (Some(d3), Some(enc)) => format!("3D {d3:.1}%, encoder {enc:.0}%, "),
            _ => String::new(),
        };
        format!(
            " | Klipp: {gpu}CPU {:.2}% ({:.1}% of a core), {:.0} MB in RAM ({:.0} MB private, {:.0} MB buffer), {} MB graphics memory, {:.1} Mbit/s, {} new frames",
            u.cpu, u.cpu_core, u.working_set_mb, u.private_mb, u.buffer_mb, u.vram_mb, u.mbps, u.new_frames
        )
    });
    crate::log::info!(
        "{who}: {height}p {fps} fps {}{}: {} frames, {} late, {} left out | send {:.1}/{:.0} ms, convert {:.1} ms | card busy {:.0}% of each frame{}{usage}{}",
        check.codec,
        if check.light_encoder { ", light" } else { "" },
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
fn record_for_a_moment(monitor: &str, gpu: &str, capture: CaptureMethod, codec: VideoCodec, height: Option<u32>, fps: u32) -> PerfCheck {
    use crate::video::{VideoPipeline, VideoSettings};
    let codec_name = if codec == VideoCodec::Av1 { "av1" } else { "h264" };
    let light = capture == CaptureMethod::Light;
    let (_, screen) = monitors::find(monitor);
    let (screen_w, screen_h) = screen.map_or((1920, 1080), |m| (m.width, m.height));
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
        width: crate::video::output_size(screen_w, screen_h, height).0,
        capture: match capture {
            CaptureMethod::Auto => "auto",
            CaptureMethod::Light => "light",
            CaptureMethod::Wgc => "wgc",
            CaptureMethod::Duplication => "duplication",
        },
        light_encoder: light,
        usage: None,
    };
    let memory_before = stats::sample();
    let shared = Arc::new(crate::state::Shared::new(crate::replay::ReplayBuffer::new(10 * crate::clock::SECOND, 64 << 20)));
    let settings = VideoSettings {
        codec,
        fps,
        height,
        quality: 30,
        max_mbps: 40,
        monitor: monitor.to_string(),
        capture,
        gpu: gpu.to_string(),
        light_encoder: light,
    };
    let pipeline = match VideoPipeline::start(settings, shared.clone()) {
        Ok(p) => p,
        Err(e) => {
            check.error = Some(format!("{e:#}"));
            return check;
        }
    };
    std::thread::sleep(PERF_SETTLE);
    // Utilization is a rate: the counters start now and are read at the end.
    let mut load = GpuLoad::start();
    let counts = |s: &crate::state::Shared| {
        (
            s.frames.load(Ordering::Relaxed),
            s.late_frames.load(Ordering::Relaxed),
            s.skipped_ticks.load(Ordering::Relaxed),
            s.new_frames.load(Ordering::Relaxed),
            s.buffer.lock().unwrap().written_bytes(),
        )
    };
    let before = counts(&shared);
    shared.steps.take();
    let (cpu_before, cycles_before, started) = (stats::sample().cpu, cycles(), Instant::now());
    std::thread::sleep(PERF_RUN);
    let (during, cycles_after, wall) = (stats::sample(), cycles(), started.elapsed().as_secs_f64());
    let after = counts(&shared);
    let steps = shared.steps.take();
    let engines = load.as_mut().map(|l| l.sample());
    let buffer = shared.buffer.lock().unwrap().allocated_bytes();
    let vram = crate::gpuload::own_vram_mb().unwrap_or_else(|| shared.vram_used_mb.load(Ordering::Relaxed));
    drop(pipeline);
    // Percent of one core.
    let core = match (cycles_before, cycles_after) {
        (Some((used0, tsc0)), Some((used1, tsc1))) if tsc1 > tsc0 => used1.saturating_sub(used0) as f64 / (tsc1 - tsc0) as f64 * 100.0,
        _ => during.cpu.saturating_sub(cpu_before).as_secs_f64() / wall * 100.0,
    };    let mine = |kind: &str, of: fn(&Engine) -> f64| engines.as_ref().map(|list| round(list.iter().find(|e| e.kind == kind).map_or(0.0, of), 1));
    let (private, buffer) = (stats::mb(during.commit), buffer as f64 / 1_048_576.0);
    check.usage = Some(Usage {
        seconds: round(wall, 2),
        gpu_3d: mine("3D", |e| e.mine),
        gpu_encode: mine("VideoEncode", |e| e.mine),
        gpu_copy: mine("Copy", |e| e.mine),
        card_3d: mine("3D", |e| e.total),
        cpu: round(core / logical_cores() as f64, 2),
        cpu_core: round(core, 1),
        before_mb: round(stats::mb(memory_before.commit), 0),
        private_mb: round(private, 0),
        working_set_mb: round(stats::mb(during.private_working_set), 0),
        buffer_mb: round(buffer, 0),
        base_mb: round(private - buffer, 0),
        vram_mb: vram,
        mbps: round((after.4 - before.4) as f64 * 8.0 / wall / 1e6, 2),
        new_frames: after.3 - before.3,
    });
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
            let cfg = VideoEncoderConfig { vendor, codec, width, height, fps: 60, quality: 30, max_bitrate: 40_000_000, light: false };
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
    ram_use_mb().0
}

/// The PC's memory and how much of it is in use now (total minus available), MB.
fn ram_use_mb() -> (u64, u64) {
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
        (status.ullTotalPhys / 1_048_576, status.ullTotalPhys.saturating_sub(status.ullAvailPhys) / 1_048_576)
    } else {
        (0, 0)
    }
}
