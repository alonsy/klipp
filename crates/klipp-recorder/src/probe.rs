//! `klipp-recorder --probe`: what this PC can record, as JSON, for the setup
//! in the Klipp window. Encoders are tested for real (opened at each size),
//! since what a graphics card supports depends on its generation and driver.

use anyhow::Result;
use klipp_media::{HwDevice, VideoCodec, VideoEncoder, VideoEncoderConfig};
use serde::Serialize;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFOEXW, MonitorFromPoint,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::core::PCWSTR;

use crate::gpu::Gpu;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    gpus: Vec<GpuInfo>,
    /// The primary monitor, which is what Klipp records.
    monitor: Option<Monitor>,
    /// The graphics card that drives it.
    recording_gpu: Option<String>,
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

/// Prints the report as JSON.
pub fn run() -> Result<()> {
    let monitor_handle = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let monitor = primary_monitor(monitor_handle);
    let mut report = Report {
        gpus: gpus(),
        monitor: monitor.as_ref().map(|m| Monitor { width: m.0, height: m.1, refresh_hz: m.2 }),
        recording_gpu: None,
        encoders: Vec::new(),
        ram_mb: ram_mb(),
        cs2: crate::cs2::installed(),
        error: None,
    };
    match test_encoders(monitor_handle, monitor.map_or((1920, 1080), |m| (m.0, m.1))) {
        Ok((gpu, checks)) => {
            report.recording_gpu = Some(gpu);
            report.encoders = checks;
        }
        Err(e) => report.error = Some(format!("{e:#}")),
    }
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

/// Opens each encoder at the screen's size and the common smaller ones.
fn test_encoders(monitor: windows::Win32::Graphics::Gdi::HMONITOR, (w, h): (u32, u32)) -> Result<(String, Vec<EncoderCheck>)> {
    let gpu = Gpu::for_monitor(monitor)?;
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
            let cfg = VideoEncoderConfig { codec, width, height, fps: 60, quality: 30, max_bitrate: 40_000_000 };
            let result = VideoEncoder::new(&device, &cfg);
            checks.push(EncoderCheck { codec: name, width, height, ok: result.is_ok(), error: result.err().map(|e| format!("{e:#}")) });
        }
    }
    Ok((gpu.adapter_name, checks))
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
        let vendor = match desc.VendorId {
            0x10DE => "nvidia",
            0x1002 | 0x1022 => "amd",
            0x8086 => "intel",
            _ => "other",
        };
        let gpu = GpuInfo { name: String::from_utf16_lossy(&desc.Description[..len]), vendor, vram_mb: desc.DedicatedVideoMemory as u64 / 1_048_576 };
        // The same card can be listed more than once (e.g. with virtual displays).
        if !out.iter().any(|g: &GpuInfo| g.name == gpu.name && g.vram_mb == gpu.vram_mb) {
            out.push(gpu);
        }
    }
    out
}

/// (width, height, refresh rate) of the primary monitor's current mode.
fn primary_monitor(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> Option<(u32, u32, u32)> {
    unsafe {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info as *mut _ as *mut _).as_bool() {
            return None;
        }
        let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        if !EnumDisplaySettingsW(PCWSTR(info.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode).as_bool() {
            return None;
        }
        Some((mode.dmPelsWidth, mode.dmPelsHeight, mode.dmDisplayFrequency))
    }
}

fn ram_mb() -> u64 {
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() { status.ullTotalPhys / 1_048_576 } else { 0 }
}
