//! The update channel: normal releases, or test builds too (for testers).
//! Kept in updates.json next to settings.toml, where the recorder reads it
//! as well (its update check, and sending the log on the test channel).

use std::fs;
use std::path::Path;

use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{Manager, Runtime, Webview};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The latest release's manifest, and the test channel's (the newest test build).
const RELEASE: &str = "https://github.com/alonsy/klipp/releases/latest/download/latest.json";
const TEST: &str = "https://github.com/alonsy/klipp/releases/download/test-channel/latest.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Updates {
    /// "production" or "test".
    channel: String,
    send_logs: bool,
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
    let send_logs = file.as_ref().and_then(|v| v["sendLogs"].as_bool()).unwrap_or(true);
    Updates { channel: channel.into(), send_logs, test_build }
}

pub fn write(dir: &Path, channel: &str, send_logs: bool) -> Result<()> {
    ensure!(channel == "production" || channel == "test", "no such update channel: {channel}");
    fs::create_dir_all(dir)?;
    fs::write(dir.join("updates.json"), serde_json::to_vec_pretty(&json!({ "channel": channel, "sendLogs": send_logs }))?)?;
    Ok(())
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
pub async fn check<R: Runtime>(webview: &Webview<R>, channel: &str) -> Result<Option<Metadata>> {
    let current = webview.package_info().version.clone();
    let update = if channel == "test" {
        match (at(webview, RELEASE, false).await, at(webview, TEST, false).await) {
            (Ok(Some(r)), Ok(Some(t))) => Some(if newer(&t.version, &r.version) { t } else { r }),
            (Ok(r), Ok(t)) => r.or(t),
            (Ok(r), Err(_)) => r,
            (Err(_), Ok(t)) => t,
            (Err(e), Err(_)) => return Err(e),
        }
    } else {
        at(webview, RELEASE, !current.pre.is_empty()).await?
    };
    Ok(update.map(|u| Metadata {
        current_version: u.current_version.clone(),
        version: u.version.clone(),
        body: u.body.clone(),
        raw_json: u.raw_json.clone(),
        rid: webview.resources_table().add(u),
    }))
}

async fn at<R: Runtime>(webview: &Webview<R>, url: &str, any_version: bool) -> Result<Option<Update>> {
    let mut builder = webview.updater_builder().endpoints(vec![url.parse()?])?;
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
