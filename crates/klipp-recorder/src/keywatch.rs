//! A second way to see the save key, for games that keep it from Windows.
//!
//! `RegisterHotKey` has Windows tell Klipp when the key goes down. A game that
//! reads the keyboard through DirectInput or raw input can switch that off
//! while it's in front (`RIDEV_NOHOTKEYS`), and League of Legends does during
//! a match: the key reaches the game and Klipp never hears of it. Raw input
//! still reaches programs in the background, so Klipp listens there too.
//!
//! It only listens: no key is blocked, changed or delayed, and nothing waits
//! on Klipp, so a busy Klipp can't slow the keyboard. Of what it sees it keeps
//! which modifiers are down and whether the save key is; nothing is logged or
//! stored, and it listens only while Klipp holds the save key.
//!
//! Most of the time both ways arrive. A press seen here waits `GRACE_MS` for
//! the hotkey; when none comes, Windows didn't pass it on and Klipp saves anyway.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEKEYBOARD,
    RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetMessageTime, HWND_MESSAGE, KillTimer, PostThreadMessageW, RegisterClassW, SetTimer, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_INPUT, WNDCLASSW,
};
use windows::core::w;

use crate::hotkey::Hotkey;
use crate::log::{info, warning};

/// Thread message to the main thread: raw input saw the save key go down;
/// `lParam` is the press's message time.
pub const WM_SAVE_KEY_SEEN: u32 = WM_APP + 10;

/// How long a press seen here waits for the hotkey. Both come from the same
/// key event, a few milliseconds apart at most.
const GRACE_MS: u32 = 300;

/// The save key being listened for: `ON | modifiers << 16 | vk`, or 0.
static TARGET: AtomicU32 = AtomicU32::new(0);
const ON: u32 = 1 << 31;
/// Which modifiers (and whether the save key) are down, as raw input showed them.
static HELD: AtomicU32 = AtomicU32::new(0);

const L_CTRL: u32 = 1;
const R_CTRL: u32 = 1 << 1;
const L_ALT: u32 = 1 << 2;
const R_ALT: u32 = 1 << 3;
const L_SHIFT: u32 = 1 << 4;
const R_SHIFT: u32 = 1 << 5;
const L_WIN: u32 = 1 << 6;
const R_WIN: u32 = 1 << 7;
const SAVE_KEY: u32 = 1 << 8;

pub struct SaveKey {
    /// A message-only window of the main thread, where raw input arrives.
    hwnd: Option<HWND>,
    watching: Option<Hotkey>,
    pairing: Pairing,
    timer: usize,
    told: bool,
}

impl SaveKey {
    /// Must be called on the thread that runs the message loop.
    pub fn new() -> Self {
        let hwnd = unsafe {
            GetModuleHandleW(None).ok().and_then(|instance| {
                let class = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: w!("KlippKeyWatch"), ..Default::default() };
                RegisterClassW(&class);
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("KlippKeyWatch"),
                    w!("Klipp"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    Some(instance.into()),
                    None,
                )
                .ok()
            })
        };
        if hwnd.is_none() {
            warning!("hotkey: couldn't make the window for raw input; the save key works only where Windows passes it on");
        }
        Self { hwnd, watching: None, pairing: Pairing::default(), timer: 0, told: false }
    }

    /// Listens for `key` (the save key while Klipp holds it), or stops.
    pub fn watch(&mut self, key: Option<Hotkey>) {
        let Some(hwnd) = self.hwnd else { return };
        if key == self.watching {
            return;
        }
        let device = |flags, target| RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x06, dwFlags: flags, hwndTarget: target };
        let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
        match key {
            Some(k) => {
                TARGET.store(ON | k.modifiers.0 << 16 | k.vk, Ordering::Relaxed);
                if self.watching.is_none() {
                    HELD.store(0, Ordering::Relaxed);
                    if let Err(e) = unsafe { RegisterRawInputDevices(&[device(RIDEV_INPUTSINK, hwnd)], size) } {
                        TARGET.store(0, Ordering::Relaxed);
                        warning!("hotkey: couldn't listen to raw input ({e}); the save key works only where Windows passes it on");
                        return;
                    }
                    if !std::mem::replace(&mut self.told, true) {
                        info!("hotkey: also listening through raw input, for games that keep the save key from Windows");
                    }
                }
            }
            None => {
                TARGET.store(0, Ordering::Relaxed);
                let _ = unsafe { RegisterRawInputDevices(&[device(RIDEV_REMOVE, HWND::default())], size) };
            }
        }
        self.watching = key;
    }

    /// The hotkey arrived (message time `time`). False if it belongs to a
    /// press that was already saved without it.
    pub fn hotkey(&mut self, time: u32) -> bool {
        let answer = self.pairing.hotkey(time);
        if self.pairing.seen_at.is_none() {
            self.stop_timer();
        }
        answer
    }

    /// Raw input saw the save key go down.
    pub fn seen(&mut self, time: u32) {
        if self.watching.is_some() && self.pairing.seen(time) {
            self.stop_timer();
            self.timer = unsafe { SetTimer(None, 0, GRACE_MS, None) };
        }
    }

    pub fn owns(&self, timer: usize) -> bool {
        self.timer != 0 && timer == self.timer
    }

    /// The wait is over: true if the hotkey never came, so the press should
    /// be saved now.
    pub fn expired(&mut self) -> bool {
        self.stop_timer();
        self.pairing.expired().is_some()
    }

    fn stop_timer(&mut self) {
        if self.timer != 0 {
            let _ = unsafe { KillTimer(None, std::mem::take(&mut self.timer)) };
        }
    }
}

/// Matches presses seen through raw input with the hotkeys Windows sent.
#[derive(Default)]
struct Pairing {
    hotkey_at: Option<u32>,
    seen_at: Option<u32>,
    saved_at: Option<u32>,
}

/// Message times are milliseconds since Windows started; they wrap.
fn near(a: u32, b: u32) -> bool {
    a.wrapping_sub(b) <= GRACE_MS || b.wrapping_sub(a) <= GRACE_MS
}

impl Pairing {
    fn hotkey(&mut self, t: u32) -> bool {
        self.hotkey_at = Some(t);
        if self.seen_at.is_some_and(|s| near(s, t)) {
            self.seen_at = None;
        }
        !self.saved_at.is_some_and(|s| near(s, t))
    }

    /// True: wait for the hotkey.
    fn seen(&mut self, t: u32) -> bool {
        if self.hotkey_at.is_some_and(|h| near(h, t)) {
            return false;
        }
        self.seen_at = Some(t);
        true
    }

    fn expired(&mut self) -> Option<u32> {
        let t = self.seen_at.take()?;
        self.saved_at = Some(t);
        Some(t)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        on_input(lparam);
    }
    // Lets Windows free the input.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn on_input(lparam: LPARAM) {
    let target = TARGET.load(Ordering::Relaxed);
    if target & ON == 0 {
        return;
    }
    let mut raw = RAWINPUT::default();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let header = std::mem::size_of::<RAWINPUTHEADER>() as u32;
    let read = unsafe { GetRawInputData(HRAWINPUT(lparam.0 as _), RID_INPUT, Some(&mut raw as *mut RAWINPUT as *mut _), &mut size, header) };
    if read == u32::MAX || read < header || raw.header.dwType != RIM_TYPEKEYBOARD.0 {
        return;
    }
    let key = unsafe { raw.data.keyboard };
    let vk = target & 0xFFFF;
    let mut held = HELD.load(Ordering::Relaxed);
    let pressed = step(&mut held, key.VKey, key.MakeCode, key.Flags, vk);
    HELD.store(held, Ordering::Relaxed);
    if pressed && async_modifiers(vk).unwrap_or(modifiers(held)) == HOT_KEY_MODIFIERS((target >> 16) & 0xF) {
        let time = unsafe { GetMessageTime() } as u32;
        let _ = unsafe { PostThreadMessageW(GetCurrentThreadId(), WM_SAVE_KEY_SEEN, WPARAM(0), LPARAM(time as isize)) };
    }
}

/// One raw key event: updates `held`, and says whether the save key has just
/// gone down (not a repeat from holding it).
fn step(held: &mut u32, vk: u16, make: u16, flags: u16, save_vk: u32) -> bool {
    const BREAK: u16 = 1;
    const E0: u16 = 2;
    let up = flags & BREAK != 0;
    let bit = match VIRTUAL_KEY(vk) {
        VK_CONTROL if flags & E0 != 0 => R_CTRL,
        VK_CONTROL => L_CTRL,
        VK_MENU if flags & E0 != 0 => R_ALT,
        VK_MENU => L_ALT,
        // E0 2A and E0 36: the Shift Windows fakes around Print Screen and some keys with Num Lock.
        VK_SHIFT if flags & E0 != 0 => return false,
        VK_SHIFT if make == 0x36 => R_SHIFT,
        VK_SHIFT => L_SHIFT,
        VK_LWIN => L_WIN,
        VK_RWIN => R_WIN,
        _ if vk as u32 == save_vk => {
            let was = *held & SAVE_KEY != 0;
            if up {
                *held &= !SAVE_KEY;
                return false;
            }
            *held |= SAVE_KEY;
            return !was;
        }
        _ => return false,
    };
    if up {
        *held &= !bit;
    } else {
        *held |= bit;
    }
    false
}

fn modifiers(held: u32) -> HOT_KEY_MODIFIERS {
    let mut m = HOT_KEY_MODIFIERS(0);
    for (bits, modifier) in [(L_CTRL | R_CTRL, MOD_CONTROL), (L_ALT | R_ALT, MOD_ALT), (L_SHIFT | R_SHIFT, MOD_SHIFT), (L_WIN | R_WIN, MOD_WIN)] {
        if held & bits != 0 {
            m |= modifier;
        }
    }
    m
}

/// The modifiers down right now, as Windows has them. Only trusted while it
/// shows the save key down too (then it's current); it knows of releases raw
/// input missed, such as keys let go on the lock screen.
fn async_modifiers(vk: u32) -> Option<HOT_KEY_MODIFIERS> {
    let down = |k: u16| unsafe { GetAsyncKeyState(k as i32) } as u16 & 0x8000 != 0;
    if !down(vk as u16) {
        return None;
    }
    let mut m = HOT_KEY_MODIFIERS(0);
    for (key, modifier) in [(VK_CONTROL, MOD_CONTROL), (VK_MENU, MOD_ALT), (VK_SHIFT, MOD_SHIFT)] {
        if down(key.0) {
            m |= modifier;
        }
    }
    if down(VK_LWIN.0) || down(VK_RWIN.0) {
        m |= MOD_WIN;
    }
    Some(m)
}
