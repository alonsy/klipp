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
/// so "Copy details" has something to send even when setup gave up here,
/// and its measurements as `pc-test.json` (see `save_pc_test`).
pub fn system_check(clips_dir: &Path, log_dir: &Path, version: &str) -> Result<Value> {
    let result = probe(clips_dir);
    let saved = match &result {
        Ok(report) => report.clone(),
        Err(e) => json!({ "error": format!("{e:#}") }),
    };
    let _ = fs::create_dir_all(log_dir);
    let _ = fs::write(log_dir.join(CHECK_FILE), serde_json::to_vec_pretty(&saved).unwrap_or_default());
    if let Ok(report) = &result {
        save_pc_test(log_dir, version, report);
    }
    result
}

pub const CHECK_FILE: &str = "setup-check.json";

/// The PC test: what recording cost this PC at each setting the probe tried
/// (Klipp's share of the graphics card, CPU and memory, next to how busy the
/// PC was), kept for Settings → Video and setup to show what other settings
/// would cost. `testVersion` changes when the measurement does.
pub const PC_TEST_FILE: &str = "pc-test.json";
const PC_TEST_VERSION: u32 = 1;

/// Keeps the probe's report as the PC test, when it measured anything (an
/// older recorder, or one that couldn't record, leaves the last test alone).
fn save_pc_test(log_dir: &Path, version: &str, report: &Value) {
    let measured = report["performance"].as_array().is_some_and(|runs| runs.iter().any(|r| r["usage"].is_object()));
    if !measured {
        return;
    }
    let mut test = json!({ "testVersion": PC_TEST_VERSION, "klippVersion": version, "at": iso_now() });
    if let (Some(test), Some(report)) = (test.as_object_mut(), report.as_object()) {
        for (k, v) in report {
            test.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    let _ = fs::write(log_dir.join(PC_TEST_FILE), serde_json::to_vec_pretty(&test).unwrap_or_default());
}

/// The last PC test, if there is one this version of Klipp can read.
pub fn last_pc_test(log_dir: &Path) -> Option<Value> {
    let test: Value = serde_json::from_slice(&fs::read(log_dir.join(PC_TEST_FILE)).ok()?).ok()?;
    (test["testVersion"].as_u64() == Some(PC_TEST_VERSION as u64)).then_some(test)
}

/// Runs the PC test again (the whole system check) and returns it, or why it couldn't measure.
pub fn run_pc_test(clips_dir: &Path, log_dir: &Path, version: &str) -> Result<Value> {
    let report = system_check(clips_dir, log_dir, version)?;
    if let Some(e) = report["error"].as_str() {
        anyhow::bail!("{e}");
    }
    last_pc_test(log_dir).context("the recorder couldn't record on this PC to measure it")
}

/// Now in UTC, as "2026-10-28T02:50:00Z".
fn iso_now() -> String {
    iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64)
}

/// Seconds since 1970 in UTC, as "2026-10-28T02:50:00Z".
fn iso(secs: i64) -> String {
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

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
