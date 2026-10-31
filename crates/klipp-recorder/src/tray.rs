//! The notification-area icon and its menu.
//!
//! Menu choices arrive in the main message loop as `WM_TRAY_COMMAND` thread
//! messages, so the tray never needs access to the recorder itself.

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::Result;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, w};

use crate::autostart;
use crate::settings::Performance;

/// Thread message posted for menu choices; `wParam` is a `Command`.
pub const WM_TRAY_COMMAND: u32 = WM_APP + 1;
/// Window message the shell sends for clicks on the icon.
const WM_TRAY_CALLBACK: u32 = WM_APP + 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Command {
    SaveClip = 1,
    OpenClips = 2,
    OpenSettings = 3,
    ToggleAutostart = 4,
    Quit = 5,
    /// Explorer restarted and the icon must be added again.
    ReAddIcon = 6,
    OpenStudio = 7,
    TogglePause = 8,
    OpenLastClip = 9,
    /// Windows switched between light and dark; redraw the icon.
    ThemeChanged = 10,
    /// A newer Klipp is out (see `update`).
    Update = 11,
    /// Performance mode off, on or plus (see `settings::Performance`).
    PerformanceOff = 12,
    PerformanceOn = 13,
    PerformancePlus = 14,
}

impl Command {
    pub fn from_wparam(w: usize) -> Option<Self> {
        [
            Self::SaveClip,
            Self::OpenClips,
            Self::OpenSettings,
            Self::ToggleAutostart,
            Self::Quit,
            Self::ReAddIcon,
            Self::OpenStudio,
            Self::TogglePause,
            Self::OpenLastClip,
            Self::ThemeChanged,
            Self::Update,
            Self::PerformanceOff,
            Self::PerformanceOn,
            Self::PerformancePlus,
        ]
        .into_iter()
        .find(|c| *c as usize == w)
    }
}

/// Id of the shell's "TaskbarCreated" message, read by the window procedure.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// Whether recording is paused, for the menu's wording.
static PAUSED: AtomicBool = AtomicBool::new(false);
/// Performance mode's command, for the menu's tick.
static PERFORMANCE: AtomicU32 = AtomicU32::new(Command::PerformanceOff as u32);

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
    paused_icon: HICON,
    paused: bool,
    tooltip: String,
}

impl Tray {
    pub fn create(tooltip: &str) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: w!("KlippTray"),
                ..Default::default()
            };
            RegisterClassW(&class);
            // A hidden top-level window (not message-only) so it receives
            // "TaskbarCreated" and can re-add the icon if Explorer restarts.
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("KlippTray"),
                w!("Klipp"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
            let (icon, paused_icon) = make_icons()?;
            let tray = Self { hwnd, icon, paused_icon, paused: false, tooltip: tooltip.to_string() };
            tray.add();
            Ok(tray)
        }
    }

    /// Redraws the icons for the current taskbar colour (light or dark).
    pub fn refresh_theme(&mut self) {
        let Ok((icon, paused_icon)) = make_icons() else { return };
        unsafe {
            let _ = DestroyIcon(std::mem::replace(&mut self.icon, icon));
            let _ = DestroyIcon(std::mem::replace(&mut self.paused_icon, paused_icon));
        }
        let mut data = self.data();
        data.uFlags = NIF_ICON;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    /// Shows the grey "paused" icon (or the red one again).
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        PAUSED.store(paused, Ordering::Relaxed);
        let mut data = self.data();
        data.uFlags = NIF_ICON;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    /// Which performance mode the menu ticks.
    pub fn set_performance(&self, mode: Performance) {
        let command = match mode {
            Performance::Off => Command::PerformanceOff,
            Performance::On => Command::PerformanceOn,
            Performance::Plus => Command::PerformancePlus,
        };
        PERFORMANCE.store(command as u32, Ordering::Relaxed);
    }

    pub fn set_tooltip(&mut self, tooltip: &str) {
        self.tooltip = tooltip.to_string();
        let mut data = self.data();
        data.uFlags = NIF_TIP;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    /// A Windows notification from the tray icon (hidden while a game is fullscreen).
    pub fn notify(&self, title: &str, text: &str) {
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_WARNING;
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, text);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    pub fn add(&self) {
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uCallbackMessage: WM_TRAY_CALLBACK,
            hIcon: if self.paused { self.paused_icon } else { self.icon },
            ..Default::default()
        };
        copy_wide(&mut data.szTip, &self.tooltip);
        data
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let data = self.data();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            let _ = DestroyWindow(self.hwnd);
            let _ = DestroyIcon(self.icon);
            let _ = DestroyIcon(self.paused_icon);
        }
    }
}

fn copy_wide<const N: usize>(dest: &mut [u16; N], text: &str) {
    let wide: Vec<u16> = text.encode_utf16().take(N - 1).collect();
    dest[..wide.len()].copy_from_slice(&wide);
    dest[wide.len()] = 0;
}

fn post(command: Command) {
    unsafe {
        let _ = PostThreadMessageW(GetCurrentThreadId(), WM_TRAY_COMMAND, WPARAM(command as usize), LPARAM(0));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar_created = TASKBAR_CREATED.load(Ordering::Relaxed);
    if taskbar_created != 0 && msg == taskbar_created {
        post(Command::ReAddIcon);
        return LRESULT(0);
    }
    if msg == WM_SETTINGCHANGE {
        // Light/dark switches arrive as "ImmersiveColorSet".
        let changed = lparam.0 != 0 && unsafe { PCWSTR(lparam.0 as *const u16).to_string() }.is_ok_and(|s| s == "ImmersiveColorSet");
        if changed {
            post(Command::ThemeChanged);
        }
    }
    if msg == WM_TRAY_CALLBACK {
        match lparam.0 as u32 {
            WM_RBUTTONUP | WM_CONTEXTMENU => unsafe { show_menu(hwnd) },
            WM_LBUTTONDBLCLK => post(Command::OpenStudio),
            _ => {}
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe fn show_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let item = |id: Command, text: PCWSTR, flags: MENU_ITEM_FLAGS| {
            let _ = AppendMenuW(menu, MF_STRING | flags, id as usize, text);
        };
        let paused = PAUSED.load(Ordering::Relaxed);
        let update = crate::update::available().map(|v| HSTRING::from(format!("Update to Klipp {v}")));
        if let Some(text) = &update {
            item(Command::Update, PCWSTR(text.as_ptr()), MF_ENABLED);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        }
        item(Command::OpenStudio, w!("Open Klipp"), MF_DEFAULT);
        item(Command::OpenLastClip, w!("Open last clip"), MF_ENABLED);
        item(Command::SaveClip, w!("Save clip now"), if paused { MF_GRAYED } else { MF_ENABLED });
        item(Command::TogglePause, if paused { w!("Resume recording") } else { w!("Pause recording") }, MF_ENABLED);
        if let Ok(modes) = CreatePopupMenu() {
            let now = PERFORMANCE.load(Ordering::Relaxed);
            for (c, text) in [
                (Command::PerformanceOff, w!("Off")),
                (Command::PerformanceOn, w!("Performance: lighter in busy games")),
                (Command::PerformancePlus, w!("Performance+: also the lightest capture")),
            ] {
                let check = if c as u32 == now { MF_CHECKED } else { MF_UNCHECKED };
                let _ = AppendMenuW(modes, MF_STRING | check, c as usize, text);
            }
            let _ = AppendMenuW(menu, MF_POPUP, modes.0 as usize, w!("Performance mode"));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        item(Command::OpenClips, w!("Open clips folder"), MF_ENABLED);
        item(Command::OpenSettings, w!("Settings"), MF_ENABLED);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let check = if autostart::is_enabled() { MF_CHECKED } else { MF_UNCHECKED };
        item(Command::ToggleAutostart, w!("Start with Windows"), check);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        item(Command::Quit, w!("Quit Klipp"), MF_ENABLED);

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu closes when clicking elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
        let _ = DestroyMenu(menu);
        if let Some(c) = Command::from_wparam(chosen.0 as usize) {
            post(c);
        }
    }
}

type Rgb = (f32, f32, f32);
/// Brand colours: the accent dot while recording, muted grey while paused, and
/// the ring in off-white on a dark taskbar or near-black on a light one.
const ACCENT: Rgb = (255.0, 90.0, 46.0); // #FF5A2E
const PAUSED_DOT: Rgb = (138.0, 141.0, 150.0); // #8A8D96
const RING_ON_DARK: Rgb = (244.0, 242.0, 236.0); // #F4F2EC
const RING_ON_LIGHT: Rgb = (20.0, 20.0, 20.0); // #141414

/// The recording and paused icons at the tray's size and in the taskbar's colours.
fn make_icons() -> Result<(HICON, HICON)> {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let ring = if taskbar_is_light() { RING_ON_LIGHT } else { RING_ON_DARK };
    Ok((make_icon(size, ring, ACCENT)?, make_icon(size, ring, PAUSED_DOT)?))
}

fn taskbar_is_light() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let (mut value, mut len) = (0u32, 4u32);
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut len),
        )
    }
    .is_ok();
    found && value == 1
}

/// Coverage (0–1) of the Klipp mark at a point on its 100-unit grid: a ring of
/// radius 32 (stroke 12, round caps) open at the upper right, and the dot.
/// `px` is the size of one pixel in grid units, for anti-aliasing.
fn mark_coverage(x: f32, y: f32, px: f32) -> (f32, f32) {
    let (dx, dy) = (x - 50.0, y - 50.0);
    // The arc runs clockwise (y points down) from -15° round to 285°.
    let angle = dy.atan2(dx).to_degrees();
    let angle = if angle < -15.0 { angle + 360.0 } else { angle };
    let ring_dist = if angle <= 285.0 {
        ((dx * dx + dy * dy).sqrt() - 32.0).abs()
    } else {
        let cap = |cx: f32, cy: f32| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        cap(80.9, 41.7).min(cap(58.3, 19.1))
    };
    let dot_dist = ((x - 79.7).powi(2) + (y - 20.3).powi(2)).sqrt();
    let cover = |dist: f32, half: f32| ((half - dist) / px + 0.5).clamp(0.0, 1.0);
    (cover(ring_dist, 6.0), cover(dot_dist, 9.0))
}

/// The tray icon, drawn in code from the logo's geometry.
fn make_icon(size: i32, ring: Rgb, dot: Rgb) -> Result<HICON> {
    let px = 100.0 / size as f32;
    let mut pixels = vec![0u32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let (ring_a, dot_a) = mark_coverage((x as f32 + 0.5) * px, (y as f32 + 0.5) * px, px);
            // The dot sits on top of the ring.
            let a = dot_a + ring_a * (1.0 - dot_a);
            let channel = |r: f32, d: f32| r * ring_a * (1.0 - dot_a) + d * dot_a;
            let (r, g, b) = (channel(ring.0, dot.0), channel(ring.1, dot.1), channel(ring.2, dot.2));
            // Premultiplied BGRA.
            pixels[(y * size + x) as usize] =
                (((a * 255.0) as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());
        let mask_bits = vec![0u8; ((size + 15) / 16 * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr() as *const _));
        let icon = CreateIconIndirect(&ICONINFO { fIcon: true.into(), hbmMask: mask, hbmColor: color, ..Default::default() });
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        Ok(icon?)
    }
}
