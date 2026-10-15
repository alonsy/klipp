//! Klipp is one app in two processes. Opening Klipp (this window) starts the
//! recorder if it isn't running; closing the window leaves the recorder in
//! the tray; "Quit Klipp" in the tray closes both.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SAM_FLAGS, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW, CreateEventW, INFINITE, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject,
};
use windows::core::{PCWSTR, w};

/// Held by the running recorder.
const RECORDER_MUTEX: PCWSTR = w!("Local\\KlippRecorder");
/// Set by the recorder's "Quit Klipp".
const QUIT_EVENT: PCWSTR = w!("Local\\KlippQuit");

pub fn recorder_running() -> bool {
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, RECORDER_MUTEX) } {
        Ok(h) => {
            let _ = unsafe { CloseHandle(h) };
            true
        }
        Err(_) => false,
    }
}

/// Starts klipp-recorder.exe (next to this exe) unless it's already running.
pub fn ensure_recorder() -> Result<()> {
    if recorder_running() {
        return Ok(());
    }
    let exe = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let dir = exe.parent().context("no folder")?.to_path_buf();
    let spawn = |flags: u32| {
        Command::new(&exe).current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(flags).spawn()
    };
    // Outside any job this process is in, so it keeps recording after the
    // window closes even if whatever started us cleans up its job. Some jobs
    // don't allow that; then start it normally.
    spawn(CREATE_NO_WINDOW.0 | CREATE_BREAKAWAY_FROM_JOB.0)
        .or_else(|_| spawn(CREATE_NO_WINDOW.0))
        .with_context(|| format!("starting {}", exe.display()))?;
    Ok(())
}

/// Calls `quit` (once) when the tray's "Quit Klipp" asks the window to close.
pub fn on_quit_request(quit: impl FnOnce() + Send + 'static) -> Result<()> {
    // Manual reset, not signaled. It only exists while the window is open, so
    // a quit while it's closed can't linger and close the next window.
    let event = unsafe { CreateEventW(None, true, false, QUIT_EVENT)? };
    let raw = event.0 as usize;
    std::thread::Builder::new().name("quit-listener".into()).spawn(move || {
        let event = HANDLE(raw as *mut _);
        unsafe { WaitForSingleObject(event, INFINITE) };
        quit();
    })?;
    Ok(())
}

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const RUN_VALUE: PCWSTR = w!("Klipp");

fn run_key(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, access, &mut key) }.is_ok().then_some(key)
}

/// "Start with Windows" starts the recorder only; the window opens when you open Klipp.
pub fn autostart_enabled() -> bool {
    let Some(key) = run_key(KEY_QUERY_VALUE) else { return false };
    let found = unsafe { RegQueryValueExW(key, RUN_VALUE, None, None, None, None) }.is_ok();
    let _ = unsafe { RegCloseKey(key) };
    found
}

pub fn set_autostart(enabled: bool) -> Result<()> {
    let key = run_key(KEY_SET_VALUE).context("can't open the Run registry key")?;
    let result = if enabled {
        let exe = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
        let wide: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
        let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
        unsafe { RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes)) }
    } else {
        unsafe { RegDeleteValueW(key, RUN_VALUE) }
    };
    let _ = unsafe { RegCloseKey(key) };
    result.ok()?;
    Ok(())
}
