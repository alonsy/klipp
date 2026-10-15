//! The monitors Klipp can record, with the names people know them by
//! ("LG ULTRAGEAR") and an id that survives reboots: the monitor's device
//! path (which screen, on which port).

use std::mem::size_of;

use serde::Serialize;
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};
use windows::Win32::Foundation::{LPARAM, POINT, RECT, WIN32_ERROR};
use windows::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFOEXW, MonitorFromPoint,
};
use windows::core::{BOOL, PCWSTR};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    /// What `video.monitor` stores.
    pub id: String,
    /// "LG ULTRAGEAR", or "Display 2" when Windows has no name for it.
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    /// Windows' main display.
    pub primary: bool,
    #[serde(skip)]
    pub handle: HMONITOR,
}

fn wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}

/// Every active monitor, the main display first.
pub fn list() -> Vec<Monitor> {
    let names = display_names();
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let handles = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        handles.push(monitor);
        true.into()
    }
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let mut out: Vec<Monitor> = handles
        .into_iter()
        .enumerate()
        .filter_map(|(i, handle)| {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            unsafe { GetMonitorInfoW(handle, &mut info.monitorInfo) }.ok().ok()?;
            let gdi = wide(&info.szDevice);
            let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
            let _ = unsafe { EnumDisplaySettingsW(PCWSTR(info.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode) };
            let (name, path) = names.iter().find(|n| n.0.eq_ignore_ascii_case(&gdi)).map(|n| (n.1.clone(), n.2.clone())).unwrap_or_default();
            Some(Monitor {
                id: if path.is_empty() { gdi.clone() } else { path },
                name: if name.is_empty() { format!("Display {}", i + 1) } else { name },
                width: mode.dmPelsWidth,
                height: mode.dmPelsHeight,
                refresh_hz: mode.dmDisplayFrequency,
                primary: info.monitorInfo.dwFlags & 1 != 0, // MONITORINFOF_PRIMARY
                handle,
            })
        })
        .collect();
    out.sort_by_key(|m| !m.primary);
    out
}

/// The monitor `id` names, or the main display if it's empty or gone.
pub fn find(id: &str) -> (HMONITOR, Option<Monitor>) {
    let all = list();
    if !id.is_empty() {
        if let Some(m) = all.iter().find(|m| m.id.eq_ignore_ascii_case(id)) {
            return (m.handle, Some(m.clone()));
        }
        crate::log::warning!("video: the chosen monitor isn't connected; recording the main display");
    }
    let main = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    (main, all.into_iter().find(|m| m.handle == main))
}

/// (GDI device name like `\\.\DISPLAY1`, friendly name, device path) per active display.
fn display_names() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    unsafe {
        let (mut paths_len, mut modes_len) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths_len, &mut modes_len) != WIN32_ERROR(0) {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_len as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_len as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut paths_len, paths.as_mut_ptr(), &mut modes_len, modes.as_mut_ptr(), None) != WIN32_ERROR(0) {
            return out;
        }
        for path in &paths[..paths_len as usize] {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 || DisplayConfigGetDeviceInfo(&mut target.header) != 0 {
                continue;
            }
            out.push((wide(&source.viewGdiDeviceName), wide(&target.monitorFriendlyDeviceName), wide(&target.monitorDevicePath)));
        }
    }
    out
}
