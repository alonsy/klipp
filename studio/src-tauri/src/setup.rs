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
    let mut report = recorder_json(&["--probe"])?;
    report["clipsDir"] = json!(clips_dir.to_string_lossy());
    report["freeGb"] = json!(free_gb(clips_dir));
    Ok(report)
}

/// The monitors Klipp can record (`klipp-recorder --list-monitors`), main display first.
pub fn monitors() -> Result<Value> {
    recorder_json(&["--list-monitors"])
}

/// Runs the recorder with `args` and reads the JSON it prints. Only the last
/// JSON line counts: graphics drivers sometimes print their own lines (AMD's
/// encoder prints "Terminating internal PA thread").
fn recorder_json(args: &[&str]) -> Result<Value> {
    let recorder = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let out = Command::new(&recorder).args(args).creation_flags(CREATE_NO_WINDOW.0).output().context("couldn't run klipp-recorder.exe")?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().rev().find(|l| l.trim_start().starts_with(['{', '['])).context("the recorder's report wasn't readable")?;
    serde_json::from_str(line).context("the recorder's report wasn't readable")
}

/// The PC's own drives (not USB sticks or network drives), for setup's "Save
/// clips on": letter, name, space, and the folder Klipp would make there
/// (Videos\Klipp on the Windows drive, `X:\Klipp` on the others).
pub fn drives() -> Vec<Value> {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    const DRIVE_FIXED: u32 = 3;
    let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
    let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_default();
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u8 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = format!("{}:", (b'A' + i) as char);
        let root = HSTRING::from(format!("{letter}\\"));
        if unsafe { GetDriveTypeW(&root) } != DRIVE_FIXED {
            continue;
        }
        let mut label = [0u16; 64];
        let _ = unsafe { GetVolumeInformationW(&root, Some(&mut label), None, None, None, None) };
        let label = String::from_utf16_lossy(&label[..label.iter().position(|&c| c == 0).unwrap_or(0)]);
        let (mut free, mut total) = (0u64, 0u64);
        if unsafe { GetDiskFreeSpaceExW(&root, Some(&mut free), Some(&mut total), None) }.is_err() {
            continue;
        }
        let is_system = letter == system;
        let folder = if is_system { home.join("Videos").join("Klipp") } else { std::path::PathBuf::from(format!("{letter}\\Klipp")) };
        let gb = |b: u64| (b as f64 / 1e9).round();
        out.push(json!({
            "letter": letter,
            "label": label,
            "freeGb": gb(free),
            "totalGb": gb(total),
            "system": is_system,
            "folder": folder.to_string_lossy(),
        }));
    }
    out
}

/// Free space on the drive holding `dir` (or the nearest folder above it that exists).
pub fn free_gb(dir: &Path) -> Option<f64> {
    let existing = dir.ancestors().find(|p| p.exists())?;
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(existing.as_os_str()), Some(&mut free), None, None) }.ok()?;
    Some((free as f64 / 1e9 * 10.0).round() / 10.0)
}
