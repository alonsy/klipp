//! Clips the recorder says are on their way (`%LOCALAPPDATA%\Klipp\arriving.json`),
//! passed on to the page whenever the list changes, so it can show them as
//! arriving until they land in the library.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde_json::Value;
use tauri::{AppHandle, Emitter};

/// How often the file is checked. Reading its time is all it costs.
const POLL: Duration = Duration::from_millis(300);

fn path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default().join("Klipp").join("arriving.json")
}

/// The current list (empty if the recorder isn't running or nothing is on its way).
pub fn read() -> Value {
    fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_array).unwrap_or(Value::Array(Vec::new()))
}

/// Emits `arrivals` with the list each time the file changes, for as long as the window is open.
pub fn watch(app: AppHandle) {
    let _ = std::thread::Builder::new().name("arrivals".into()).spawn(move || {
        let mut seen: Option<SystemTime> = None;
        loop {
            let modified = fs::metadata(path()).and_then(|m| m.modified()).ok();
            if modified != seen {
                seen = modified;
                let _ = app.emit("arrivals", read());
            }
            std::thread::sleep(POLL);
        }
    });
}
