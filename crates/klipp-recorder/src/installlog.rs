//! What installing and updating did, for the log. The installer and the
//! window's updater write `install.log` next to recorder.log, while Klipp is
//! closed or closing; the next recorder start copies its lines into
//! recorder.log (so testers' logs carry them to the developer) and keeps the
//! file as install.log.1. Programs an install had to move aside because they
//! were still running (klipp.exe.old, klipp-recorder.exe.old) are deleted
//! here too, once nothing runs them.

use std::fs;
use std::path::Path;

use crate::log::{info, warning};

pub fn import(log_dir: &Path) {
    let path = log_dir.join("install.log");
    if let Ok(bytes) = fs::read(&path) {
        for line in String::from_utf8_lossy(&bytes).lines().map(str::trim).filter(|l| !l.is_empty()) {
            info!("install: {line}");
        }
        let _ = fs::rename(&path, log_dir.join("install.log.1"));
    }
    let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) else { return };
    for name in ["klipp.exe.old", "klipp-recorder.exe.old"] {
        let old = dir.join(name);
        if old.exists() {
            match fs::remove_file(&old) {
                Ok(()) => info!("install: removed {name}, which an install moved aside while it was still running"),
                Err(e) => warning!("install: {name} is left from an install and can't be removed yet: {e}"),
            }
        }
    }
}
