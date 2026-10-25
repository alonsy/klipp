//! Test channel: sends the recorder's log to the developer (klipp.dev), so a
//! tester doesn't have to be asked for it. Only on the test channel with
//! "Send logs" on (Settings → About); every 15 minutes, only what's new since
//! the last send, with the user's own folder name left out. Nothing else
//! leaves the PC: no clips, no pictures, no settings file.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::{HSTRING, w};

use crate::channel::{self, Channel};
use crate::log::{info, warning};

const FIRST: Duration = Duration::from_secs(120);
const EVERY: Duration = Duration::from_secs(15 * 60);
/// One piece is at most this big; a longer backlog goes in several, a few per round.
const PIECE: u64 = 512 * 1024;
const PIECES_PER_ROUND: usize = 4;

pub fn start(log: Option<PathBuf>) {
    let Some(log) = log else { return };
    let spawned = thread::Builder::new().name("log-send".into()).spawn(move || {
        thread::sleep(FIRST);
        let mut said = false;
        let mut last_error = String::new();
        loop {
            let updates = channel::read();
            if updates.channel == Channel::Test && updates.send_logs {
                match send_new(&log) {
                    Ok(bytes) => {
                        if bytes > 0 && !said {
                            info!("log: sending the log to the developer (test channel; turn it off in Settings → About)");
                            said = true;
                        }
                        last_error.clear();
                    }
                    Err(e) => {
                        let text = format!("{e:#}");
                        if text != last_error {
                            warning!("log: couldn't send the log ({text}); trying again later");
                            last_error = text;
                        }
                    }
                }
            }
            thread::sleep(EVERY);
        }
    });
    if let Err(e) = spawned {
        warning!("log: {e}");
    }
}

/// Sends what the log gained since the last send; how many bytes went.
fn send_new(log: &Path) -> Result<u64> {
    let state = log.with_file_name("log-sent.json");
    let mut offset = fs::read(&state).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["offset"].as_u64()).unwrap_or(0);
    let len = fs::metadata(log)?.len();
    if len < offset {
        // A new log: the old one was moved aside at startup.
        offset = 0;
    }
    let install = channel::install_id().context("no install id")?;
    let windows = windows_version();
    let mut sent = 0;
    for _ in 0..PIECES_PER_ROUND {
        if offset >= len {
            break;
        }
        let mut f = File::open(log)?;
        f.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        f.take(PIECE.min(len - offset)).read_to_end(&mut bytes)?;
        // Whole lines only; the rest goes next time.
        let end = match bytes.iter().rposition(|&b| b == b'\n') {
            Some(i) => i + 1,
            None if (bytes.len() as u64) < PIECE => break,
            None => bytes.len(),
        };
        let text = anonymize(&String::from_utf8_lossy(&bytes[..end]));
        let body = json!({
            "install": install,
            "version": env!("CARGO_PKG_VERSION"),
            "channel": "test",
            "windows": windows,
            "file": "recorder.log",
            "from": offset,
            "text": text,
        });
        let (status, _) = crate::update::request(w!("klipp.dev"), w!("/api/logs"), Some(&serde_json::to_vec(&body)?))?;
        match status {
            204 => {}
            429 => break,
            other => bail!("klipp.dev answered {other}"),
        }
        offset += end as u64;
        sent += end as u64;
        fs::write(&state, json!({ "offset": offset }).to_string())?;
    }
    Ok(sent)
}

/// Paths under the user's own folder name the person; they're sent as %USERPROFILE%.
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
