//! Clips on their way, for the Klipp window to show as "a clip is arriving"
//! until the file lands: a hotkey clip still collecting presses, an auto-clip
//! whose events already make it a clip, and any clip being written.
//!
//! Kept in `%LOCALAPPDATA%\Klipp\arriving.json`, which the window watches.
//! Small and rewritten whole on every change (a few times per clip).

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Arrival {
    pub id: u64,
    /// "manual" or "auto".
    pub kind: &'static str,
    pub game: String,
    /// What an auto-clip will be named after ("2k"), when known.
    pub label: Option<String>,
    /// Hotkey presses collected into it so far.
    pub presses: u32,
    /// Milliseconds since 1970.
    pub since: i64,
}

pub struct Arrivals {
    path: Option<PathBuf>,
    state: Mutex<(u64, Vec<Arrival>)>,
}

impl Arrivals {
    pub fn new() -> Self {
        let path = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Klipp").join("arriving.json"));
        let a = Self { path, state: Mutex::new((1, Vec::new())) };
        // Nothing is on its way yet, whatever a previous run left behind.
        a.write(&[]);
        a
    }

    pub fn add(&self, kind: &'static str, game: &str, label: Option<String>) -> u64 {
        let since = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let mut s = self.state.lock().unwrap();
        let id = s.0;
        s.0 += 1;
        s.1.push(Arrival { id, kind, game: game.to_string(), label, presses: u32::from(kind == "manual"), since });
        self.write(&s.1);
        id
    }

    /// Changes an arrival (more presses, a better label); writes only if something changed.
    pub fn update(&self, id: u64, change: impl FnOnce(&mut Arrival)) {
        let mut s = self.state.lock().unwrap();
        let Some(a) = s.1.iter_mut().find(|a| a.id == id) else { return };
        let before = a.clone();
        change(a);
        if *a != before {
            self.write(&s.1);
        }
    }

    pub fn remove(&self, id: u64) {
        let mut s = self.state.lock().unwrap();
        let n = s.1.len();
        s.1.retain(|a| a.id != id);
        if s.1.len() != n {
            self.write(&s.1);
        }
    }

    fn write(&self, list: &[Arrival]) {
        let Some(path) = &self.path else { return };
        let Ok(json) = serde_json::to_string(list) else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let part = path.with_extension("json.part");
        if fs::write(&part, json).is_ok() {
            let _ = fs::rename(&part, path);
        }
    }
}

impl Drop for Arrivals {
    fn drop(&mut self) {
        self.write(&[]);
    }
}
