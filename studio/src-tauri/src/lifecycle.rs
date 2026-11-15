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
    CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW, CreateEventW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE,
    SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

/// True for an address the browser should get: http or https, with nothing
/// in it that could end the address early.
pub fn is_web_address(s: &str) -> bool {
    (s.starts_with("https://") || s.starts_with("http://")) && s.len() <= 2000 && !s.chars().any(|c| c.is_control() || c == ' ' || c == '"')
}

/// Opens a web address in the default browser. Never through explorer.exe:
/// Explorer reads "=" and "," in its arguments as its own syntax, so an
/// address with a query ("…/link?code=AB12-CD34") opened the Documents folder
/// instead of the page. Only web addresses are passed on; anything else is
/// refused, because "open" would also start a program.
pub fn open_url(url: &str) -> Result<()> {
    anyhow::ensure!(is_web_address(url), "not a web address");
    let wide: Vec<u16> = url.encode_utf16().chain([0]).collect();
    let result = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // ShellExecute answers with a number above 32 when it worked.
    anyhow::ensure!(result.0 as usize > 32, "Windows couldn't open your browser");
    Ok(())
}

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

/// Plays the save sound (or the error sound) once through the recorder, on
/// the device and at the volume it will use for real (the ▶ buttons).
pub fn play_sound(device: &str, volume: f64, error: bool) -> Result<()> {
    let exe = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let mut cmd = Command::new(&exe);
    cmd.arg("--play-sound");
    if error {
        cmd.arg("error");
    }
    cmd.args(["--device", device, "--volume", &volume.clamp(0.0, 1.0).to_string()]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW.0);
    cmd.spawn().with_context(|| format!("starting {}", exe.display()))?;
    Ok(())
}

/// Set to make the running recorder let go of its save key for 15 s.
const LEND_EVENT: PCWSTR = w!("Local\\KlippLendHotkey");

/// Before a key test: a running recorder holds the save key, so it lends it
/// (it checks once a second). Returns once the key is free to test.
pub fn borrow_hotkey() {
    if !recorder_running() {
        return;
    }
    let lent = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, LEND_EVENT) }.is_ok_and(|event| unsafe {
        let set = SetEvent(event).is_ok();
        let _ = CloseHandle(event);
        set
    });
    if lent {
        std::thread::sleep(std::time::Duration::from_millis(1300));
    }
}

/// Setup's key test: the recorder holds `key` for 10 s and waits for one
/// press. "works", "taken" (another app holds it), "nothing" (no press
/// arrived) or "invalid". Call `borrow_hotkey` first.
pub fn test_hotkey(key: &str) -> Result<String> {
    let exe = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let out = Command::new(&exe)
        .args(["--test-hotkey", key, "10"])
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
        .with_context(|| format!("starting {}", exe.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().rev().find(|l| l.trim_start().starts_with('{')).context("the key test said nothing")?;
    let result: serde_json::Value = serde_json::from_str(line)?;
    result["result"].as_str().map(String::from).context("the key test said nothing")
}

/// The level bars: runs `klipp-recorder --levels SECONDS` beside the running
/// recorder (it opens the same sources) and hands each report, ten a second,
/// to `on_report`.
pub fn audio_levels(seconds: u32, mut on_report: impl FnMut(serde_json::Value)) -> Result<()> {
    use std::io::{BufRead, BufReader};
    let exe = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let mut child = Command::new(&exe)
        .args(["--levels", &seconds.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    let out = child.stdout.take().context("the recorder gave no output")?;
    for line in BufReader::new(out).lines() {
        let line = line?;
        if let Ok(report) = serde_json::from_str(line.trim_start()) {
            on_report(report);
        }
    }
    child.wait()?;
    Ok(())
}

/// Teaching "Klipp that": runs `klipp-recorder --voice-teach` beside the
/// running recorder and hands each step to `on_step`.
pub fn voice_teach(mut on_step: impl FnMut(serde_json::Value)) -> Result<()> {
    use std::io::{BufRead, BufReader};
    let exe = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let mut child = Command::new(&exe)
        .arg("--voice-teach")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    let out = child.stdout.take().context("the recorder gave no output")?;
    for line in BufReader::new(out).lines() {
        let line = line?;
        if let Ok(step) = serde_json::from_str(line.trim_start()) {
            on_step(step);
        }
    }
    child.wait()?;
    Ok(())
}

/// Shows an on-screen indicator once through the recorder (`on`, `saved` or
/// `failed`), on the screen Klipp records.
pub fn preview_indicator(kind: &str) -> Result<()> {
    anyhow::ensure!(["on", "saved", "failed"].contains(&kind), "unknown indicator {kind}");
    let exe = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    Command::new(&exe)
        .args(["--show-indicator", kind])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    Ok(())
}

/// Whether a program with this file name (e.g. "cs2.exe") is running.
pub fn process_running(exe: &str) -> bool {
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return false };
    let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut found = false;
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more && !found {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        found = String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe);
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snapshot) };
    found
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
