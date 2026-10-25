//! Log lines go to `%LOCALAPPDATA%\Klipp\recorder.log` and, if there
//! is one, the console.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::System::SystemInformation::GetLocalTime;

static LOG: Mutex<Option<File>> = Mutex::new(None);
/// Checked at startup; a bigger log is moved to recorder.old.log.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Opens the log file. Returns its path, or `None` if it can't be opened
/// (logging then goes to the console only).
pub fn init() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Klipp");
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join("recorder.log");
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = fs::rename(&path, dir.join("recorder.old.log"));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    *LOG.lock().unwrap() = Some(file);
    Some(path)
}

/// Stops writing to the log file, for helpers that run beside the recorder
/// (the window's level bars) and would only clutter its log.
pub fn to_console_only() {
    *LOG.lock().unwrap() = None;
}

static QUIET: AtomicBool = AtomicBool::new(false);

/// Log lines go to the file only: for modes whose output is read as JSON (`--probe`).
pub fn to_file_only() {
    QUIET.store(true, Ordering::Relaxed);
}

pub fn write(level: &str, args: fmt::Arguments) {
    let t = unsafe { GetLocalTime() };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {level} {args}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    if QUIET.load(Ordering::Relaxed) {
    } else if level == "WARN" {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
    if let Some(f) = LOG.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{line}");
    }
}

macro_rules! info {
    ($($t:tt)*) => { $crate::log::write("INFO", format_args!($($t)*)) };
}
macro_rules! warning {
    ($($t:tt)*) => { $crate::log::write("WARN", format_args!($($t)*)) };
}
pub(crate) use {info, warning};
