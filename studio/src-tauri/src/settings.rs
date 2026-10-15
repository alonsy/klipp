//! Reading and editing the recorder's settings.toml. Edits go through
//! toml_edit so comments and layout survive; the recorder notices the saved
//! file and applies it within a second.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, value};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

/// The recorder's own template: the defaults for anything the file doesn't set.
const TEMPLATE: &str = include_str!("../../../crates/klipp-recorder/default-settings.toml");

/// The settings the recorder will use: the template's defaults with the
/// user's file laid over them.
pub fn read(path: &Path) -> Result<Json> {
    let mut merged: toml::Table = TEMPLATE.parse()?;
    if let Ok(text) = fs::read_to_string(path) {
        let user: toml::Table = text.trim_start_matches('\u{feff}').parse().with_context(|| format!("{} has a mistake", path.display()))?;
        overlay(&mut merged, user);
    }
    Ok(serde_json::to_value(merged)?)
}

fn overlay(base: &mut toml::Table, top: toml::Table) {
    for (k, v) in top {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(t)) => overlay(b, t),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

#[derive(Deserialize)]
pub struct Change {
    /// e.g. ["auto_clip", "cs2", "headshot"]
    pub key: Vec<String>,
    pub value: Json,
}

/// Applies changes to the file in place (creating it from the template if missing).
pub fn write(path: &Path, changes: &[Change]) -> Result<()> {
    let text = fs::read_to_string(path).unwrap_or_else(|_| TEMPLATE.to_string());
    let mut doc: DocumentMut = text.trim_start_matches('\u{feff}').parse()?;
    for change in changes {
        let (last, parents) = change.key.split_last().context("empty settings key")?;
        let mut table = doc.as_table_mut();
        for k in parents {
            let item = table.entry(k).or_insert(Item::Table(Table::new()));
            table = item.as_table_mut().with_context(|| format!("{k} isn't a section"))?;
        }
        let new = to_toml(&change.value)?;
        // Keep any trailing comment on the line being changed.
        let decor = table.get(last).and_then(|i| i.as_value()).map(|v| v.decor().clone());
        let mut item = value(new);
        if let (Some(decor), Some(v)) = (decor, item.as_value_mut()) {
            *v.decor_mut() = decor;
        }
        table[last.as_str()] = item;
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let part = path.with_extension("toml.part");
    fs::write(&part, doc.to_string())?;
    fs::rename(&part, path)?;
    Ok(())
}

fn to_toml(v: &Json) -> Result<toml_edit::Value> {
    Ok(match v {
        Json::Bool(b) => (*b).into(),
        Json::Number(n) if n.is_i64() => n.as_i64().unwrap().into(),
        Json::Number(n) => n.as_f64().unwrap_or(0.0).into(),
        Json::String(s) => s.as_str().into(),
        Json::Array(items) => {
            let mut array = Array::new();
            for item in items {
                array.push(to_toml(item)?);
            }
            // One element per line, like the template's audio sources.
            if items.iter().any(Json::is_object) {
                for v in array.iter_mut() {
                    v.decor_mut().set_prefix("\n  ");
                }
                array.set_trailing("\n");
                array.set_trailing_comma(true);
            }
            array.into()
        }
        Json::Object(map) => {
            let mut table = InlineTable::new();
            for (k, v) in map {
                table.insert(k, to_toml(v)?);
            }
            table.into()
        }
        Json::Null => bail!("settings can't be empty"),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub name: String,
    /// "playback" or "recording".
    pub kind: String,
    pub is_default: bool,
}

/// Audio devices, as listed by the recorder (it has the WASAPI code).
pub fn audio_devices() -> Result<Vec<Device>> {
    let recorder = std::env::current_exe()?.with_file_name("klipp-recorder.exe");
    let out = Command::new(&recorder).arg("--list-audio").creation_flags(CREATE_NO_WINDOW.0).output().context("couldn't run klipp-recorder.exe")?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (kind, rest) = if let Some(r) = line.strip_prefix("playback (loopback)") {
                ("playback", r)
            } else {
                ("recording", line.strip_prefix("recording")?)
            };
            let rest = rest.trim();
            let (name, is_default) = match rest.strip_suffix("[default]") {
                Some(n) => (n.trim(), true),
                None => (rest, false),
            };
            Some(Device { name: name.to_string(), kind: kind.into(), is_default })
        })
        .collect())
}
