//! The on-screen indicators: "Auto-clips ON" when a match starts, and a small
//! mark when a clip is saved (or can't be). A window of Klipp's own on top of
//! everything, the game included: click-through, never takes focus (which
//! would minimize a fullscreen game), not in the taskbar, and left out of
//! every screen capture, so it can't end up in a clip. Nothing touches the game.
//!
//! The pills are images rendered from the design (scripts/indicators), at
//! three sizes for 1080p, 1440p and 4K screens.

use std::collections::HashMap;
use std::mem::size_of;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetMonitorInfoW, HMONITOR, MONITORINFO, SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use crate::log::{info, warning};

/// Thread message to the main thread asking for an indicator; `wParam` is an `Indicator`.
pub const WM_INDICATOR: u32 = WM_APP + 3;

/// What happened, as other threads report it; the main thread decides
/// whether the settings want it shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Indicator {
    /// A CS2 match started.
    MatchStarted = 1,
    /// A hotkey clip was saved.
    Saved = 2,
    /// An auto-clip was saved.
    AutoSaved = 3,
    /// Nothing could be saved (paused, nothing recorded yet, or the save failed).
    Failed = 4,
}

impl Indicator {
    pub fn from_wparam(w: usize) -> Option<Self> {
        [Self::MatchStarted, Self::Saved, Self::AutoSaved, Self::Failed].into_iter().find(|i| *i as usize == w)
    }
}

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

pub fn set_main_thread() {
    MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

/// From any thread: report something the indicator may show.
pub fn post(what: Indicator) {
    let thread = MAIN_THREAD.load(Ordering::Relaxed);
    if thread != 0 {
        let _ = unsafe { PostThreadMessageW(thread, WM_INDICATOR, WPARAM(what as usize), LPARAM(0)) };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pill {
    AutoClipsOn,
    Saved,
    Failed,
}

impl Pill {
    fn hold(self) -> Duration {
        match self {
            Pill::AutoClipsOn => Duration::from_millis(3000),
            Pill::Saved | Pill::Failed => Duration::from_millis(1500),
        }
    }
}

/// Image sizes: 1x for screens up to 1200 px tall, 1.33x up to 1800, 2x above.
const SCALES: [f32; 3] = [1.0, 4.0 / 3.0, 2.0];
/// 1em at 1x in the rendered images; each image has 1em of room around the pill for its shadow.
const BASE_EM: f32 = 12.0;

const IMAGES: [[&[u8]; 3]; 3] = [
    [
        include_bytes!("../../../assets/overlay/on@1x.png"),
        include_bytes!("../../../assets/overlay/on@1.33x.png"),
        include_bytes!("../../../assets/overlay/on@2x.png"),
    ],
    [
        include_bytes!("../../../assets/overlay/saved@1x.png"),
        include_bytes!("../../../assets/overlay/saved@1.33x.png"),
        include_bytes!("../../../assets/overlay/saved@2x.png"),
    ],
    [
        include_bytes!("../../../assets/overlay/failed@1x.png"),
        include_bytes!("../../../assets/overlay/failed@1.33x.png"),
        include_bytes!("../../../assets/overlay/failed@2x.png"),
    ],
];

const FADE_IN: Duration = Duration::from_millis(150);
const FADE_OUT: Duration = Duration::from_millis(220);
const FRAME_MS: u32 = 15;

/// Premultiplied BGRA pixels.
struct Image {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}

struct Showing {
    since: Instant,
    hold: Duration,
}

pub struct Overlay {
    hwnd: HWND,
    images: HashMap<(Pill, usize), Image>,
    showing: Option<Showing>,
    timer: usize,
}

impl Overlay {
    pub fn new() -> Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: w!("KlippIndicator"), ..Default::default() };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("KlippIndicator"),
                w!("Klipp"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            // Windows 10 2004 and newer leave it out of every capture. Without
            // that it would be recorded, so it isn't shown at all.
            if let Err(e) = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) {
                let _ = DestroyWindow(hwnd);
                bail!("this Windows can't keep a window out of recordings ({e})");
            }
            Ok(Self { hwnd, images: HashMap::new(), showing: None, timer: 0 })
        }
    }

    /// The animation timer's id, while one is running.
    pub fn timer(&self) -> usize {
        self.timer
    }

    pub fn visible(&self) -> bool {
        self.showing.is_some()
    }

    /// Shows `pill` on `monitor`, top right, just below CS2's kill feed. A
    /// pill already showing is replaced and its time starts over.
    pub fn show(&mut self, pill: Pill, monitor: HMONITOR) -> Result<()> {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe { GetMonitorInfoW(monitor, &mut info) }.ok().context("no monitor info")?;
        let screen = info.rcMonitor;
        let (sw, sh) = (screen.right - screen.left, screen.bottom - screen.top);
        let scale = if sh <= 1200 { 0 } else if sh <= 1800 { 1 } else { 2 };
        let pad = (BASE_EM * SCALES[scale]).round() as i32;
        if !self.images.contains_key(&(pill, scale)) {
            let index = match pill {
                Pill::AutoClipsOn => 0,
                Pill::Saved => 1,
                Pill::Failed => 2,
            };
            self.images.insert((pill, scale), decode_png(IMAGES[index][scale])?);
        }
        let image = &self.images[&(pill, scale)];
        // The pill's right edge 1.2% in from the screen's, its top at 26% of the height.
        let x = screen.right - (sw as f32 * 0.012).round() as i32 - image.width + pad;
        let y = screen.top + (sh as f32 * 0.26).round() as i32 - pad;
        let already = self.showing.is_some();
        draw(self.hwnd, image, POINT { x, y }, if already { 255 } else { 0 })?;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        // Already up: no second fade-in, just the full time again.
        let since = if already { Instant::now() - FADE_IN } else { Instant::now() };
        self.showing = Some(Showing { since, hold: pill.hold() });
        if self.timer == 0 {
            self.timer = unsafe { SetTimer(None, 0, FRAME_MS, None) };
        }
        Ok(())
    }

    /// One animation step (the timer from `timer()` fired).
    pub fn tick(&mut self) {
        let Some(s) = &self.showing else { return self.stop_timer() };
        let t = s.since.elapsed();
        let alpha = if t < FADE_IN {
            t.as_secs_f32() / FADE_IN.as_secs_f32()
        } else if t < FADE_IN + s.hold {
            1.0
        } else {
            1.0 - (t - FADE_IN - s.hold).as_secs_f32() / FADE_OUT.as_secs_f32()
        };
        if alpha <= 0.0 {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.showing = None;
            return self.stop_timer();
        }
        let blend = blend((alpha.min(1.0) * 255.0) as u8);
        unsafe {
            let _ = UpdateLayeredWindow(self.hwnd, None, None, None, None, None, COLORREF(0), Some(&blend), ULW_ALPHA);
        }
    }

    fn stop_timer(&mut self) {
        if self.timer != 0 {
            unsafe {
                let _ = KillTimer(None, self.timer);
            }
            self.timer = 0;
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        self.stop_timer();
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> windows::Win32::Foundation::LRESULT {
    // Never take focus, even when clicked through to.
    if msg == WM_MOUSEACTIVATE {
        return windows::Win32::Foundation::LRESULT(MA_NOACTIVATE as isize);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 }
}

/// Puts `image` in the window at `at` (screen pixels) with an overall opacity.
fn draw(hwnd: HWND, image: &Image, at: POINT, alpha: u8) -> Result<()> {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: image.width,
                biHeight: -image.height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        std::ptr::copy_nonoverlapping(image.pixels.as_ptr(), bits as *mut u8, image.pixels.len());
        let old = SelectObject(dc, bitmap.into());
        let size = SIZE { cx: image.width, cy: image.height };
        let blend = blend(alpha);
        let result = UpdateLayeredWindow(hwnd, None, Some(&at), Some(&size), Some(dc), Some(&POINT::default()), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        result?;
        Ok(())
    }
}

/// PNG → premultiplied BGRA, with Windows' own image decoder.
fn decode_png(png: &[u8]) -> Result<Image> {
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(png)?;
        let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let (mut width, mut height) = (0u32, 0u32);
        converter.GetSize(&mut width, &mut height)?;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
        Ok(Image { width: width as i32, height: height as i32, pixels })
    }
}

/// `klipp-recorder --show-indicator on|saved|failed`: shows one pill on the
/// recorded screen and returns once it has faded (Settings' Preview buttons).
pub fn preview(pill: Pill, monitor: HMONITOR) -> Result<()> {
    let mut overlay = Overlay::new()?;
    overlay.show(pill, monitor)?;
    let mut msg = MSG::default();
    while overlay.visible() && unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        if msg.message == WM_TIMER && msg.wParam.0 == overlay.timer() {
            overlay.tick();
        } else {
            unsafe { DispatchMessageW(&msg) };
        }
    }
    Ok(())
}

/// Created on first use, so a recorder with indicators off never makes the window.
#[derive(Default)]
pub struct Indicators {
    overlay: Option<Overlay>,
    unavailable: bool,
}

impl Indicators {
    pub fn show(&mut self, pill: Pill, monitor: HMONITOR) {
        if self.unavailable {
            return;
        }
        if self.overlay.is_none() {
            match Overlay::new() {
                Ok(o) => {
                    info!("indicators: ready");
                    self.overlay = Some(o);
                }
                Err(e) => {
                    warning!("indicators: {e:#}; not shown");
                    self.unavailable = true;
                    return;
                }
            }
        }
        if let Some(o) = &mut self.overlay
            && let Err(e) = o.show(pill, monitor)
        {
            warning!("indicators: {e:#}");
        }
    }

    /// The timer is the overlay's animation timer.
    pub fn owns(&self, timer: usize) -> bool {
        self.overlay.as_ref().is_some_and(|o| timer != 0 && o.timer() == timer)
    }

    pub fn tick(&mut self) {
        if let Some(o) = &mut self.overlay {
            o.tick();
        }
    }
}
