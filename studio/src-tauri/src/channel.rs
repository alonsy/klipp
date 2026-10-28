//! The update channel: normal releases, or test builds too (for testers).
//! Kept in updates.json next to settings.toml, where the recorder reads it
//! as well (its update check, and sending the log on the test channel).
//!
//! Checks ask klipp.dev first: it answers with the same manifests as GitHub
//! and counts active installs from the random install number it's sent
//! (D43). GitHub directly if klipp.dev can't be reached.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::path::Path;

use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{Manager, Runtime, Webview};
use tauri_plugin_updater::{Update, UpdaterExt};
use toml_edit::DocumentMut;

/// The latest release's manifest, and the test channel's (the newest test build).
const RELEASE: &str = "https://github.com/alonsy/klipp/releases/latest/download/latest.json";
const TEST: &str = "https://github.com/alonsy/klipp/releases/download/test-channel/latest.json";
const KLIPP_DEV: &str = "https://klipp.dev/api/update";

/// Settings tables test builds add that releases before them don't know.
/// Their recorders refuse unknown settings (and fall back to defaults), so
/// going back to a normal release takes these out first.
const TEST_ONLY_SETTINGS: &[(&str, &str)] = &[("auto_clip", "league"), ("auto_clip", "rocket_league")];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Updates {
    /// "production" or "test".
    channel: String,
    send_logs: bool,
    /// Look for updates by itself.
    checks: bool,
    /// A newer version installs itself between games or when Klipp starts (the recorder's autoinstall).
    auto_install: bool,
    /// This copy is a test build ("0.3.5-test.1").
    test_build: bool,
}

/// What updates.json says; a test build starts on the test channel.
pub fn read(dir: &Path, version: &str) -> Updates {
    let file = fs::read(dir.join("updates.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let test_build = version.contains("-test");
    let channel = match file.as_ref().and_then(|v| v["channel"].as_str()) {
        Some("test") => "test",
        Some("production") => "production",
        _ if test_build => "test",
        _ => "production",
    };
    let flag = |key: &str, default: bool| file.as_ref().and_then(|v| v[key].as_bool()).unwrap_or(default);
    // Logs go by default on the test channel only (D52).
    Updates {
        channel: channel.into(),
        send_logs: flag("sendLogs", channel == "test"),
        checks: flag("checks", true),
        auto_install: flag("autoInstall", true),
        test_build,
    }
}

pub fn write(dir: &Path, version: &str, channel: &str, send_logs: bool, checks: bool, auto_install: bool) -> Result<()> {
    ensure!(channel == "production" || channel == "test", "no such update channel: {channel}");
    fs::create_dir_all(dir)?;
    fs::write(dir.join("updates.json"), serde_json::to_vec_pretty(&json!({ "channel": channel, "sendLogs": send_logs, "checks": checks, "autoInstall": auto_install }))?)?;
    if channel == "production" && version.contains("-test") {
        forget_test_settings(&dir.join("settings.toml"))?;
    }
    Ok(())
}

/// Takes the test builds' own settings tables out of settings.toml, keeping everything else as it is.
fn forget_test_settings(path: &Path) -> Result<()> {
    let Ok(text) = fs::read_to_string(path) else { return Ok(()) };
    let mut doc: DocumentMut = text.trim_start_matches('\u{feff}').parse()?;
    let mut changed = false;
    for (section, table) in TEST_ONLY_SETTINGS {
        if let Some(t) = doc.get_mut(section).and_then(|s| s.as_table_like_mut()) {
            changed |= t.remove(table).is_some();
        }
    }
    if changed {
        fs::write(path, doc.to_string())?;
    }
    Ok(())
}

/// The recorder's random install number (made here if the recorder hasn't yet).
fn install_id(dir: &Path) -> Option<String> {
    let path = dir.join("install-id");
    if let Ok(id) = fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(id);
        }
    }
    // RandomState is seeded from the operating system's random source.
    let half = || {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        h.finish()
    };
    let id = format!("{:016x}{:016x}", half(), half());
    fs::create_dir_all(dir).ok()?;
    fs::write(&path, &id).ok()?;
    Some(id)
}

/// What the window's `Update` object is made from (the updater plugin's own shape).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    rid: tauri::ResourceId,
    current_version: String,
    version: String,
    body: Option<String>,
    raw_json: Value,
}

/// The update the channel offers, if any. On the test channel that's the
/// newer of the newest test build and the latest release; leaving a test
/// build for normal releases offers the latest release even though its
/// number is lower.
pub async fn check<R: Runtime>(webview: &Webview<R>, dir: &Path, channel: &str) -> Result<Option<Metadata>> {
    let current = webview.package_info().version.clone();
    let any_version = channel != "test" && !current.pre.is_empty();
    let asked = format!("{KLIPP_DEV}?channel={channel}&version={current}&from=window");
    let update = match at(webview, &asked, any_version, install_id(dir)).await {
        Ok(found) => found,
        Err(_) if channel == "test" => match (at(webview, RELEASE, false, None).await, at(webview, TEST, false, None).await) {
            (Ok(Some(r)), Ok(Some(t))) => Some(if newer(&t.version, &r.version) { t } else { r }),
            (Ok(r), Ok(t)) => r.or(t),
            (Ok(r), Err(_)) => r,
            (Err(_), Ok(t)) => t,
            (Err(e), Err(_)) => return Err(e),
        },
        Err(_) => at(webview, RELEASE, any_version, None).await?,
    };
    Ok(update.map(|u| Metadata {
        current_version: u.current_version.clone(),
        version: u.version.clone(),
        body: u.body.clone(),
        raw_json: u.raw_json.clone(),
        rid: webview.resources_table().add(u),
    }))
}

async fn at<R: Runtime>(webview: &Webview<R>, url: &str, any_version: bool, install: Option<String>) -> Result<Option<Update>> {
    let mut builder = webview.updater_builder().endpoints(vec![url.parse()?])?;
    if let Some(id) = install {
        builder = builder.header("Klipp-Install", id)?;
    }
    if any_version {
        builder = builder.version_comparator(|current, remote| remote.version != current);
    }
    Ok(builder.build()?.check().await?)
}

fn newer(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}
