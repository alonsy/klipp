//! "Copy details": what someone reporting a problem can paste into an issue
//! or a message. The PC, setup's last check, the settings that matter and the
//! end of the recorder's log, with the user's own folder name left out.

use std::fmt::Write as _;
use std::fs;

use serde_json::Value;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::{HSTRING, w};

use crate::config::Config;
use crate::{lifecycle, settings, setup};

const LOG_LINES: usize = 40;

pub fn details(version: &str, config: &Config) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Klipp {version} on {}", windows_version());
    let _ = writeln!(out, "Recorder: {}", if lifecycle::recorder_running() { "running" } else { "not running" });

    match fs::read(config.log_dir.join(setup::CHECK_FILE)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        Some(check) => describe_check(&mut out, &check),
        None => {
            let _ = writeln!(out, "Setup check: not run yet");
        }
    }
    match settings::read(&config.settings_path) {
        Ok(s) => describe_settings(&mut out, &s),
        Err(e) => {
            let _ = writeln!(out, "Settings: can't be read ({e:#})");
        }
    }
    // What the recorder found when it tried the save key.
    if let Some(key) = fs::read(config.log_dir.join("hotkey.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        let state = if key["works"] == true { "works" } else { "taken by another app" };
        let _ = writeln!(out, "Save key: {} ({state})", key["hotkey"].as_str().unwrap_or("?"));
    }

    let log = fs::read_to_string(config.log_dir.join("recorder.log")).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    let tail = &lines[lines.len().saturating_sub(LOG_LINES)..];
    if tail.is_empty() {
        let _ = writeln!(out, "\nRecorder log: empty (the recorder hasn't run yet)");
    } else {
        let _ = writeln!(out, "\nRecorder log (last {} lines):", tail.len());
    }
    for line in tail {
        let _ = writeln!(out, "{line}");
    }
    anonymize(&out)
}

fn describe_check(out: &mut String, check: &Value) {
    if let Some(error) = check["error"].as_str() {
        let _ = writeln!(out, "Setup check failed: {error}");
    }
    let gpus: Vec<String> = check["gpus"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| match g["vramMb"].as_u64() {
            Some(mb) if mb >= 1024 => format!("{} ({} GB)", g["name"].as_str().unwrap_or("?"), (mb + 512) / 1024),
            _ => g["name"].as_str().unwrap_or("?").to_string(),
        })
        .collect();
    if !gpus.is_empty() {
        let _ = writeln!(out, "Graphics: {}", gpus.join(", "));
    }
    let screen = match (check["monitor"]["width"].as_u64(), check["monitor"]["height"].as_u64(), check["monitor"]["refreshHz"].as_u64()) {
        (Some(w), Some(h), Some(hz)) => format!("{w} × {h} at {hz} Hz"),
        _ => "unknown".into(),
    };
    let encoder = check["encoder"].as_str().map(|e| format!(" ({e})")).unwrap_or_default();
    let _ = writeln!(out, "Records on: {}{encoder} · screen {screen}", check["recordingGpu"].as_str().unwrap_or("nothing found"));
    for codec in ["av1", "h264"] {
        let tries: Vec<&Value> = check["encoders"].as_array().into_iter().flatten().filter(|e| e["codec"] == codec).collect();
        if tries.is_empty() {
            continue;
        }
        let name = if codec == "av1" { "AV1" } else { "H.264" };
        let ok: Vec<String> = tries.iter().filter(|e| e["ok"] == true).map(|e| format!("{}p", e["height"])).collect();
        let failed = tries.iter().find(|e| e["ok"] != true);
        let mut line = if ok.is_empty() { format!("{name}: fails") } else { format!("{name}: works at {}", ok.join(", ")) };
        if let Some(f) = failed {
            let _ = write!(line, " · fails at {}p: {}", f["height"], f["error"].as_str().unwrap_or("?"));
        }
        let _ = writeln!(out, "{line}");
    }
    let ram = check["ramMb"].as_u64().unwrap_or(0);
    let found = |key: &str| if check[key] == true { "found" } else { "not found" };
    let _ = writeln!(
        out,
        "Memory: {} GB · CS2: {} · League: {} · Rocket League: {}",
        (ram + 512) / 1024,
        found("cs2"),
        found("league"),
        found("rocketLeague")
    );
}

fn describe_settings(out: &mut String, s: &Value) {
    let text = |v: &Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
    let video = &s["video"];
    let _ = writeln!(
        out,
        "Video: {}, {}, {} fps · buffer {} s",
        text(&video["codec"]).to_uppercase(),
        text(&video["resolution"]),
        text(&video["fps"]),
        text(&s["replay"]["length_secs"])
    );
    let sources: Vec<String> = s["audio"]["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|src| {
            if src["kind"] == "discord" {
                return format!("{} (the Discord app, experimental)", text(&src["name"]));
            }
            let device = src["device"].as_str().filter(|d| !d.is_empty()).unwrap_or("Windows default");
            format!("{} ({}: {device})", text(&src["name"]), text(&src["kind"]))
        })
        .collect();
    let _ = writeln!(out, "Audio: {}", if sources.is_empty() { "none".into() } else { sources.join(", ") });
    let auto = |v: &Value| if v == &Value::Bool(true) { "on" } else { "off" };
    let a = &s["auto_clip"];
    let _ = writeln!(
        out,
        "Auto-clips: {} (CS2 {}, League {}, Rocket League {})",
        auto(&a["enabled"]),
        auto(&a["cs2"]["enabled"]),
        auto(&a["league"]["enabled"]),
        auto(&a["rocket_league"]["enabled"])
    );
}

/// "Windows 11 24H2 (build 26200)".
fn windows_version() -> String {
    let read = |name: &str| -> Option<String> {
        let mut buf = [0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
                &HSTRING::from(name),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut len),
            )
        }
        .ok()
        .ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    };
    let build = read("CurrentBuildNumber").unwrap_or_default();
    // Windows 11 still calls itself "Windows 10" in ProductName; the build tells them apart.
    let name = if build.parse::<u32>().is_ok_and(|b| b >= 22000) { "Windows 11" } else { "Windows 10" };
    match read("DisplayVersion") {
        Some(v) => format!("{name} {v} (build {build})"),
        None => format!("{name} (build {build})"),
    }
}

/// Paths under the user's own folder name the person; show them as %USERPROFILE%.
fn anonymize(text: &str) -> String {
    let Some(home) = std::env::var_os("USERPROFILE").map(|h| h.to_string_lossy().into_owned()).filter(|h| h.len() > 3) else {
        return text.to_string();
    };
    replace_ignore_case(text, &home, "%USERPROFILE%")
}

fn replace_ignore_case(text: &str, needle: &str, with: &str) -> String {
    let (lower, needle_lower) = (text.to_lowercase(), needle.to_lowercase());
    // Lowercasing can change byte lengths for some letters; then fall back to an exact match.
    if lower.len() != text.len() {
        return text.replace(needle, with);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    for (i, _) in lower.match_indices(&needle_lower) {
        out.push_str(&text[rest..i]);
        out.push_str(with);
        rest = i + needle.len();
    }
    out.push_str(&text[rest..]);
    out
}
