//! The PC and the settings, in two lines at startup, so any log (a tester's
//! sent one, or one pasted into an issue) says what it was recorded on.

use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIDevice, IDXGIFactory1};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::core::{HSTRING, Interface, PCWSTR, w};

use crate::log::info;
use crate::settings::Settings;

pub fn log_summary(s: &Settings) {
    let cpu = registry(w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"), "ProcessorNameString").map(|c| c.trim().to_string()).unwrap_or_else(|| "?".into());
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    let screens: Vec<String> = crate::monitors::list()
        .iter()
        .map(|m| format!("{} {}x{} {} Hz{}", m.name, m.width, m.height, m.refresh_hz, if m.primary { " (main)" } else { "" }))
        .collect();
    info!(
        "system: {} | {cpu}, {threads} threads | {} GB memory | {} | {}",
        windows_version(),
        (ram_mb() + 512) / 1024,
        gpus().join(", "),
        screens.join(", ")
    );
    let v = &s.video;
    let sources: Vec<String> = s.audio.sources.iter().map(|src| format!("{:?}", src.kind).to_lowercase()).collect();
    info!(
        "settings: {:?} {:?} {} fps, quality {}, up to {} Mbit/s, {:?} capture{}, performance mode {} | buffer {} s | audio {} | auto-clips {}",
        v.codec,
        v.resolution,
        v.fps,
        v.quality,
        v.max_bitrate_mbps,
        v.capture,
        if v.gpu.is_empty() { String::new() } else { format!(" on {}", v.gpu) },
        format!("{:?}", v.performance).to_lowercase(),
        s.replay.length_secs,
        if sources.is_empty() { "none".into() } else { sources.join(", ") },
        if s.auto_clip.enabled { "on" } else { "off" }
    );
}

/// Every graphics card with its memory and driver version.
fn gpus() -> Vec<String> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else { break };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        // The user-mode driver's version, a.b.c.d (NVIDIA's 32.0.15.8164 is their 581.64).
        let driver = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) }
            .map(|v| format!("{}.{}.{}.{}", (v >> 48) & 0xffff, (v >> 32) & 0xffff, (v >> 16) & 0xffff, v & 0xffff))
            .unwrap_or_else(|_| "?".into());
        let gpu = format!("{} ({} GB, driver {driver})", String::from_utf16_lossy(&desc.Description[..len]), (desc.DedicatedVideoMemory as u64 + (512 << 20)) >> 30);
        // The same card can be listed more than once (e.g. with virtual displays).
        if !out.contains(&gpu) {
            out.push(gpu);
        }
    }
    out
}

fn ram_mb() -> u64 {
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() { status.ullTotalPhys / 1_048_576 } else { 0 }
}

fn registry(key: PCWSTR, name: &str) -> Option<String> {
    let mut buf = [0u16; 128];
    let mut len = (buf.len() * 2) as u32;
    unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key, &HSTRING::from(name), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) }
        .ok()
        .ok()?;
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// "Windows 11 24H2 (build 26200)".
pub fn windows_version() -> String {
    let key = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let build = registry(key, "CurrentBuildNumber").unwrap_or_default();
    // Windows 11 still calls itself "Windows 10" in ProductName; the build tells them apart.
    let name = if build.parse::<u32>().is_ok_and(|b| b >= 22000) { "Windows 11" } else { "Windows 10" };
    match registry(key, "DisplayVersion") {
        Some(v) => format!("{name} {v} (build {build})"),
        None => format!("{name} (build {build})"),
    }
}
