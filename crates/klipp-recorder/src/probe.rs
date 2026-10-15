//! `klipp-recorder --probe`: what this PC can record, as JSON, for the setup
//! in the Klipp window. Encoders are tested for real (opened at each size and
//! fed a few frames), since what a graphics card supports depends on its
//! generation and driver.

use anyhow::{Context, Result};
use klipp_media::{HwDevice, Packet, VideoCodec, VideoEncoder, VideoEncoderConfig, Vendor};
use serde::Serialize;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::HMONITOR;
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
    /// CS2 is installed (auto-clips can work).
    cs2: bool,
    /// Why encoders couldn't be tested at all, if so.
    error: Option<String>,
}

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
        cs2: crate::cs2::installed(),
        error: None,
    };
    let size = chosen.map_or((1920, 1080), |m| (m.width, m.height));
    match test_encoders(handle, gpu, size, &mut report) {
        Ok(checks) => report.encoders = checks,
        Err(e) => report.error = Some(format!("{e:#}")),
    }
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
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

fn ram_mb() -> u64 {
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() { status.ullTotalPhys / 1_048_576 } else { 0 }
}
