//! First-run setup: what the setup screens need to know about this PC.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::core::HSTRING;

/// A fresh install: there are no settings yet.
pub fn needed(settings_path: &Path) -> bool {
    !settings_path.exists()
}

/// The recorder's hardware report (`klipp-recorder --probe`: graphics card,
/// screen, which encoders open at which sizes, RAM, CS2), plus free space
/// where clips will go. Also kept as `setup-check.json` in the log folder,
/// so "Copy details" has something to send even when setup gave up here.
pub fn system_check(clips_dir: &Path, log_dir: &Path) -> Result<Value> {
    let result = probe(clips_dir);
    let saved = match &result {
        Ok(report) => report.clone(),
        Err(e) => json!({ "error": format!("{e:#}") }),
    };
    let _ = fs::create_dir_all(log_dir);
    let _ = fs::write(log_dir.join(CHECK_FILE), serde_json::to_vec_pretty(&saved).unwrap_or_default());
    result
}

pub const CHECK_FILE: &str = "setup-check.json";

fn probe(clips_dir: &Path) -> Result<Value> {
    let recorder = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let out = Command::new(&recorder).arg("--probe").creation_flags(CREATE_NO_WINDOW.0).output().context("couldn't run klipp-recorder.exe")?;
    let mut report: Value = serde_json::from_slice(&out.stdout).context("the recorder's report wasn't readable")?;
    report["clipsDir"] = json!(clips_dir.to_string_lossy());
    report["freeGb"] = json!(free_gb(clips_dir));
    Ok(report)
}

/// Free space on the drive holding `dir` (or the nearest folder above it that exists).
pub fn free_gb(dir: &Path) -> Option<f64> {
    let existing = dir.ancestors().find(|p| p.exists())?;
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(existing.as_os_str()), Some(&mut free), None, None) }.ok()?;
    Some((free as f64 / 1e9 * 10.0).round() / 10.0)
}
