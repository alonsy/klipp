//! Which updates this copy follows, and whether it sends its log to the
//! developer. Testers follow the test channel: test builds as they come out,
//! and normal releases once they're newer. Kept in updates.json next to
//! settings.toml rather than in the settings, so a version that doesn't know
//! about it (going back to a normal release) still reads its settings.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Production,
    Test,
}

pub struct Updates {
    pub channel: Channel,
    /// Test channel only: the log goes to the developer every 15 minutes.
    pub send_logs: bool,
    /// Look for updates by itself (klipp.dev first, with the version and the install number).
    pub checks: bool,
    /// Test channel only: a newer test build installs itself between games (autoinstall).
    pub auto_install: bool,
}

impl Channel {
    pub fn key(self) -> &'static str {
        match self {
            Channel::Production => "production",
            Channel::Test => "test",
        }
    }
}

/// A test build ("0.3.5-test.1") starts on the test channel.
pub fn is_test_build() -> bool {
    env!("CARGO_PKG_VERSION").contains("-test")
}

fn dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Klipp"))
}

/// What updates.json says (the window writes it), with defaults for anything missing.
pub fn read() -> Updates {
    let file = dir().and_then(|d| fs::read(d.join("updates.json")).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let channel = match file.as_ref().and_then(|v| v["channel"].as_str()) {
        Some("test") => Channel::Test,
        Some("production") => Channel::Production,
        _ if is_test_build() => Channel::Test,
        _ => Channel::Production,
    };
    let send_logs = file.as_ref().and_then(|v| v["sendLogs"].as_bool()).unwrap_or(true);
    let checks = file.as_ref().and_then(|v| v["checks"].as_bool()).unwrap_or(true);
    let auto_install = file.as_ref().and_then(|v| v["autoInstall"].as_bool()).unwrap_or(true);
    Updates { channel, send_logs, checks, auto_install }
}

/// A random number made once per install, so the developer can tell one
/// tester's logs from another's. It says nothing about the PC or the person.
pub fn install_id() -> Option<String> {
    let path = dir()?.join("install-id");
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
    fs::create_dir_all(path.parent()?).ok()?;
    fs::write(&path, &id).ok()?;
    Some(id)
}
