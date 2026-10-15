// Release builds have no console window; started from a terminal they still print there.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Klipp recorder: keeps the last N seconds of the screen and audio in
//! RAM and saves them as a clip when you press the hotkey. Lives in the tray.

mod arrivals;
mod audio;
mod autoclip;
mod autostart;
mod capture;
mod clock;
mod cs2;
mod cue;
mod duplication;
mod events;
mod games;
mod gpu;
mod hotkey;
mod log;
mod mmcss;
mod monitors;
mod overlay;
mod probe;
mod process_loopback;
mod recorder;
mod replay;
mod save;
mod selftest;
mod settings;
mod state;
mod stats;
mod timeline;
mod tray;
mod update;
mod video;
mod watchdog;
mod wasapi;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Instant, SystemTime};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, LPARAM, WPARAM};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetConsoleCtrlHandler, SetStdHandle};
use windows::Win32::System::Threading::{CreateMutexW, EVENT_MODIFY_STATE, GetCurrentThreadId, OpenEventW, SetEvent};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MESSAGEBOX_STYLE, MSG, MessageBoxW, PostThreadMessageW,
    SW_SHOWNORMAL, SetTimer, WM_HOTKEY, WM_QUIT, WM_TIMER,
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

use crate::log::{info, warning};
use crate::recorder::{AUDIO_CATCH_UP, HOTKEY_SAVE, Recorder};
use crate::settings::Settings;
use crate::state::Shared;
use crate::stats::mb;
use crate::tray::{Command, Tray, WM_TRAY_COMMAND};
use crate::wasapi::SourceKind;

struct Args {
    settings: Option<PathBuf>,
}

const USAGE: &str = "usage: klipp-recorder [--settings FILE]\n       \
klipp-recorder --list-audio | --list-monitors | --cleanup | --check-update\n       \
klipp-recorder --probe [--monitor ID] [--gpu NAME]\n       \
klipp-recorder --test-recording SECONDS FILE [--settings FILE]\n       \
klipp-recorder --show-indicator on|saved|failed\n       \
klipp-recorder --play-sound [error] [--device NAME] [--volume 0-1]\n\n\
Settings live in %APPDATA%\\Klipp\\settings.toml (created on first run).";

fn parse_args() -> Result<Args> {
    let mut a = Args { settings: None };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().with_context(|| format!("{flag} needs a value\n{USAGE}"));
        match flag.as_str() {
            "--settings" => a.settings = Some(PathBuf::from(value()?)),
            "--list-audio" => {
                list_audio()?;
                std::process::exit(0);
            }
            "--probe" => {
                // Optional: --monitor ID (default the main display), --gpu NAME (test another card).
                let (mut monitor, mut gpu) = (String::new(), String::new());
                while let Some(opt) = it.next() {
                    match opt.as_str() {
                        "--monitor" => monitor = it.next().context("--monitor needs a value")?,
                        "--gpu" => gpu = it.next().context("--gpu needs a value")?,
                        other => bail!("unknown argument {other}\n{USAGE}"),
                    }
                }
                probe::run(&monitor, &gpu)?;
                std::process::exit(0);
            }
            "--test-recording" => {
                let seconds: u32 = it.next().context("--test-recording needs SECONDS FILE")?.parse()?;
                let out = PathBuf::from(it.next().context("--test-recording needs SECONDS FILE")?);
                let mut path = a.settings.clone();
                while let Some(opt) = it.next() {
                    match opt.as_str() {
                        "--settings" => path = Some(PathBuf::from(it.next().context("--settings needs a value")?)),
                        other => bail!("unknown argument {other}\n{USAGE}"),
                    }
                }
                let settings = match path {
                    Some(p) => Settings::load_or_create(&p)?,
                    None => Settings::load_or_create(&Settings::default_path()?)?,
                };
                selftest::run(seconds.clamp(1, 120), &out, &settings)?;
                std::process::exit(0);
            }
            "--test-audio" => {
                let seconds: u32 = it.next().context("--test-audio needs SECONDS FILE")?.parse()?;
                let out = PathBuf::from(it.next().context("--test-audio needs SECONDS FILE")?);
                selftest::run_audio(seconds.clamp(2, 60), &out)?;
                std::process::exit(0);
            }
            "--show-indicator" => {
                let pill = match it.next().as_deref() {
                    Some("on") => overlay::Pill::AutoClipsOn,
                    Some("saved") => overlay::Pill::Saved,
                    Some("failed") => overlay::Pill::Failed,
                    _ => bail!("--show-indicator needs on, saved or failed\n{USAGE}"),
                };
                let settings = Settings::load_or_create(&a.settings.clone().map_or_else(Settings::default_path, Ok)?).unwrap_or_default();
                overlay::preview(pill, monitors::find(&settings.video.monitor).0)?;
                std::process::exit(0);
            }
            "--list-monitors" => {
                println!("{}", serde_json::to_string(&monitors::list())?);
                std::process::exit(0);
            }
            "--cleanup" => {
                cleanup();
                std::process::exit(0);
            }
            "--play-sound" => {
                play_sound(it.by_ref())?;
                std::process::exit(0);
            }
            "--check-update" => {
                let latest = update::latest_version()?;
                let current = env!("CARGO_PKG_VERSION");
                let newer = if update::is_newer(&latest, current) { "an update" } else { "up to date" };
                println!("latest {latest}, installed {current}: {newer}");
                std::process::exit(0);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}\n{USAGE}"),
        }
    }
    Ok(a)
}

/// `--play-sound [error] [--device NAME] [--volume 0-1]`: the save (or
/// error) sound once, for the ▶ buttons in the window.
fn play_sound(mut it: impl Iterator<Item = String>) -> Result<()> {
    let (mut sound, mut device, mut volume) = (cue::Sound::Saved, String::new(), 0.6f32);
    while let Some(flag) = it.next() {
        let mut value = || it.next().with_context(|| format!("{flag} needs a value\n{USAGE}"));
        match flag.as_str() {
            "error" => sound = cue::Sound::Error,
            "--device" => device = value()?,
            "--volume" => volume = value()?.parse::<f32>()?.clamp(0.0, 1.0),
            other => bail!("unknown argument {other}\n{USAGE}"),
        }
    }
    cue::play_once(sound, &device, volume)
}

/// For the uninstaller: what Klipp set up outside its own folder. Clips and
/// settings stay.
fn cleanup() {
    let _ = autostart::set(false);
    cs2::remove_cfg();
}

fn list_audio() -> Result<()> {
    unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED).ok()? };
    for d in wasapi::list_devices()? {
        let kind = match d.kind {
            SourceKind::Loopback => "playback (loopback)",
            SourceKind::Capture => "recording",
        };
        println!("{:<20} {}{}", kind, d.name, if d.is_default { "  [default]" } else { "" });
    }
    Ok(())
}

/// With no console of our own, print to the terminal that started us, if any.
/// Redirected output (and debug builds) already have working handles.
fn attach_console() {
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|h| !h.is_invalid() && !h.0.is_null()) {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok()
            && let Ok(console) = CreateFileW(
                w!("CONOUT$"),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        {
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, console);
            let _ = SetStdHandle(STD_ERROR_HANDLE, console);
        }
    }
}

fn message_box(text: &str, style: MESSAGEBOX_STYLE) {
    unsafe { MessageBoxW(None, &HSTRING::from(text), w!("Klipp"), MB_OK | style) };
}

/// Held for the process lifetime; a second recorder sees it and exits.
struct SingleInstance(HANDLE);

impl SingleInstance {
    fn acquire() -> Result<Option<Self>> {
        let handle = unsafe { CreateMutexW(None, true, w!("Local\\KlippRecorder"))? };
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn on_console_ctrl(_event: u32) -> BOOL {
    let _ = unsafe { PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_QUIT, WPARAM(0), LPARAM(0)) };
    true.into()
}

/// Notices when the settings file changes, but only reports it once the file
/// has stopped changing for a poll interval, so half-saved files aren't read.
struct SettingsWatcher {
    path: PathBuf,
    seen: Option<SystemTime>,
    pending: bool,
}

impl SettingsWatcher {
    fn new(path: PathBuf) -> Self {
        let seen = Self::modified(&path);
        Self { path, seen, pending: false }
    }

    fn modified(path: &Path) -> Option<SystemTime> {
        fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    fn poll(&mut self) -> bool {
        let now = Self::modified(&self.path);
        if now != self.seen {
            self.seen = now;
            self.pending = true;
            return false;
        }
        std::mem::take(&mut self.pending)
    }
}

/// Remembers when the save was requested and arms a timer for when the audio
/// has caught up with that moment (see `AUDIO_CATCH_UP`).
fn schedule_save(pending: &mut Option<(usize, i64)>) {
    if pending.is_none() {
        let end = clock::now();
        let ms = (AUDIO_CATCH_UP * 1000 / clock::SECOND) as u32;
        *pending = Some((unsafe { SetTimer(None, 0, ms, None) }, end));
    }
}

/// Opens a folder or file with its default app (Notepad for files without one).
fn open_path(path: &Path) {
    let target = HSTRING::from(path.as_os_str());
    let result = unsafe { ShellExecuteW(None, w!("open"), &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if result.0 as usize <= 32 {
        let quoted = HSTRING::from(format!("\"{}\"", path.display()));
        unsafe { ShellExecuteW(None, w!("open"), w!("notepad.exe"), &quoted, PCWSTR::null(), SW_SHOWNORMAL) };
    }
}

/// Shows an indicator if the settings want this one: "Auto-clips ON" needs
/// CS2 auto-clips on; auto-clip saves follow "also for auto-clips".
fn indicate(indicators: &mut overlay::Indicators, s: &Settings, what: overlay::Indicator) {
    use overlay::{Indicator, Pill};
    let pill = match what {
        Indicator::MatchStarted if s.indicators.autoclips_on && s.auto_clip.enabled && s.auto_clip.cs2.enabled => Pill::AutoClipsOn,
        Indicator::Saved if s.indicators.saves => Pill::Saved,
        Indicator::AutoSaved if s.indicators.saves && s.cue.auto_clips => Pill::Saved,
        Indicator::Failed if s.indicators.saves => Pill::Failed,
        _ => return,
    };
    indicators.show(pill, monitors::find(&s.video.monitor).0);
}

fn open_url(url: &str) {
    unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
}

fn tooltip(s: &Settings) -> String {
    format!("Klipp\n{} saves the last {} s", s.replay.hotkey, s.replay.length_secs)
}

fn main() {
    // Real pixels on every monitor: sharp indicators and tray icon at any display scaling.
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    attach_console();
    let log_path = log::init();
    if let Err(e) = run(log_path) {
        warning!("fatal: {e:#}");
        message_box(&format!("Klipp couldn't start:\n\n{e:#}"), MB_ICONERROR);
        std::process::exit(1);
    }
}

fn run(log_path: Option<PathBuf>) -> Result<()> {
    let args = parse_args()?;
    let Some(_instance) = SingleInstance::acquire()? else {
        message_box("Klipp is already running. Look for its icon next to the clock.", MB_ICONINFORMATION);
        return Ok(());
    };
    klipp_media::init_logging();

    let settings_path = match args.settings {
        Some(p) => p,
        None => Settings::default_path()?,
    };
    let settings = Settings::load_or_create(&settings_path).unwrap_or_else(|e| {
        warning!("settings: {e:#}; using defaults until the file is fixed");
        Settings::default()
    });
    MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    overlay::set_main_thread();
    let mut indicators = overlay::Indicators::default();
    unsafe {
        // Only works when attached to a console; Ctrl+C there quits cleanly.
        let _ = SetConsoleCtrlHandler(Some(on_console_ctrl), true);
    }

    let mut recorder = Recorder::start(settings)?;
    let mut tray = Tray::create(&tooltip(recorder.settings()))?;
    update::start();
    let mut hotkey_ok = recorder.hotkey_registered();
    if !hotkey_ok {
        let hk = &recorder.settings().replay.hotkey;
        tray.notify("Hotkey unavailable", &format!("{hk} is used by another app. Close it or pick another hotkey in settings."));
    }

    let mut watcher = SettingsWatcher::new(settings_path.clone());
    let stats_timer = unsafe { SetTimer(None, 0, 60_000, None) };
    let settings_timer = unsafe { SetTimer(None, 0, 1_000, None) };
    let games_timer = unsafe { SetTimer(None, 0, 2_000, None) };
    let autoclip_timer = unsafe { SetTimer(None, 0, 250, None) };
    info!(
        "recording: {} saves the last {} s to {} | settings {} | log {}",
        recorder.settings().replay.hotkey,
        recorder.settings().replay.length_secs,
        recorder.settings().storage.clips_dir().display(),
        settings_path.display(),
        log_path.as_deref().map_or("(none)".into(), |p| p.display().to_string())
    );

    let mut prev = (Instant::now(), stats::sample());
    let mut pending_save = None;
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        if !msg.hwnd.is_invalid() {
            unsafe { DispatchMessageW(&msg) };
            continue;
        }
        match msg.message {
            WM_HOTKEY if msg.wParam.0 == HOTKEY_SAVE as usize => {
                let ok = recorder.press();
                indicate(&mut indicators, recorder.settings(), if ok { overlay::Indicator::Saved } else { overlay::Indicator::Failed });
                schedule_save(&mut pending_save);
            }
            overlay::WM_INDICATOR => {
                if let Some(what) = overlay::Indicator::from_wparam(msg.wParam.0) {
                    indicate(&mut indicators, recorder.settings(), what);
                }
            }
            WM_TIMER if indicators.owns(msg.wParam.0) => indicators.tick(),
            WM_TRAY_COMMAND => match Command::from_wparam(msg.wParam.0) {
                Some(Command::SaveClip) => {
                    let ok = recorder.press();
                    indicate(&mut indicators, recorder.settings(), if ok { overlay::Indicator::Saved } else { overlay::Indicator::Failed });
                    schedule_save(&mut pending_save);
                }
                Some(Command::OpenClips) => {
                    let dir = recorder.settings().storage.clips_dir();
                    let _ = fs::create_dir_all(&dir);
                    open_path(&dir);
                }
                Some(Command::OpenStudio) => {
                    if !open_studio(&[]) {
                        let dir = recorder.settings().storage.clips_dir();
                        let _ = fs::create_dir_all(&dir);
                        open_path(&dir);
                    }
                }
                Some(Command::OpenLastClip) => match recorder.last_clip() {
                    Some(clip) if !open_studio(&[clip.as_os_str()]) => open_path(&clip),
                    Some(_) => {}
                    None => tray.notify("No clip yet", "Nothing has been saved since Klipp started."),
                },
                Some(Command::TogglePause) => {
                    let paused = !recorder.paused();
                    recorder.set_paused(paused);
                    tray.set_paused(paused);
                    tray.set_tooltip(&if paused { "Klipp\nPaused".to_string() } else { tooltip(recorder.settings()) });
                }
                Some(Command::OpenSettings) => {
                    if !open_studio(&["--settings".as_ref()]) {
                        open_path(&settings_path);
                    }
                }
                Some(Command::ToggleAutostart) => {
                    if let Err(e) = autostart::set(!autostart::is_enabled()) {
                        warning!("start with Windows: {e:#}");
                    }
                }
                // The window shows the update and installs it in one click.
                Some(Command::Update) => {
                    if !open_studio(&["--update".as_ref()]) {
                        open_url("https://github.com/alonsy/klipp/releases/latest");
                    }
                }
                Some(Command::ReAddIcon) => tray.add(),
                Some(Command::ThemeChanged) => tray.refresh_theme(),
                Some(Command::Quit) => {
                    close_studio();
                    break;
                }
                None => {}
            },
            WM_TIMER if msg.wParam.0 == stats_timer => {
                log_stats(&recorder.shared, prev);
                prev = (Instant::now(), stats::sample());
            }
            WM_TIMER if msg.wParam.0 == settings_timer => {
                if watcher.poll() {
                    match Settings::load_or_create(&settings_path) {
                        Ok(s) if s != *recorder.settings() => {
                            recorder.apply(s);
                            tray.set_tooltip(&tooltip(recorder.settings()));
                            let ok = recorder.hotkey_registered();
                            if hotkey_ok && !ok {
                                tray.notify("Hotkey unavailable", &format!("{} is used by another app.", recorder.settings().replay.hotkey));
                            }
                            hotkey_ok = ok;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            warning!("settings: {e:#}; keeping the previous settings");
                            tray.notify("Settings not applied", "There's a mistake in settings.toml; the log has details.");
                        }
                    }
                }
            }
            WM_TIMER if msg.wParam.0 == games_timer => {
                recorder.games.poll(&recorder.settings().games.clone());
                recorder.check_health();
            }
            WM_TIMER if msg.wParam.0 == autoclip_timer => recorder.tick_autoclip(),
            WM_TIMER if pending_save.is_some_and(|(timer, _)| timer == msg.wParam.0) => {
                let _ = unsafe { KillTimer(None, msg.wParam.0) };
                if let Some((_, end)) = pending_save.take() {
                    recorder.save(end);
                }
            }
            _ => {}
        }
    }

    let shared = recorder.shared.clone();
    drop(tray);
    // Stops capture and waits for queued clips to finish writing.
    drop(recorder);
    log_stats(&shared, prev);
    info!("stopped");
    Ok(())
}

/// Opens the studio (klipp.exe next to us) with these arguments. If it is
/// already open, it comes to the front and handles them. False if it isn't installed.
fn open_studio(args: &[&std::ffi::OsStr]) -> bool {
    let Some(exe) = std::env::current_exe().ok().map(|p| p.with_file_name("klipp.exe")).filter(|p| p.exists()) else {
        return false;
    };
    std::process::Command::new(exe).args(args).spawn().is_ok()
}

/// "Quit Klipp" closes the studio window too. It waits on this event while open.
fn close_studio() {
    unsafe {
        if let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\KlippQuit")) {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
    }
}

fn log_stats(shared: &Shared, (prev_time, prev_stats): (Instant, stats::ProcessStats)) {
    let s = stats::sample();
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    let cpu = (s.cpu - prev_stats.cpu).as_secs_f64() / prev_time.elapsed().as_secs_f64() / cores * 100.0;
    let (held, allocated, seconds) = {
        let b = shared.buffer.lock().unwrap();
        (b.held_bytes(), b.allocated_bytes(), b.duration() as f64 / clock::SECOND as f64)
    };
    info!(
        "stats: CPU {cpu:.2}% | RAM {:.1} MB, commit {:.1} MB | buffer {seconds:.1} s, {:.1} MB held / {:.1} MB allocated | frames {} (new {}, late {}, skipped {}) | capture resizes {}",
        mb(s.private_working_set),
        mb(s.commit),
        held as f64 / 1048576.0,
        allocated as f64 / 1048576.0,
        shared.frames.load(Ordering::Relaxed),
        shared.new_frames.load(Ordering::Relaxed),
        shared.late_frames.load(Ordering::Relaxed),
        shared.skipped_ticks.load(Ordering::Relaxed),
        shared.capture_resizes.load(Ordering::Relaxed),
    );
}
