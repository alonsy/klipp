//! What Klipp learns about this PC while it records. When games keep the
//! graphics card too busy for the set frame rate in two sessions, Klipp
//! starts at the lower rate from then on, instead of stuttering at the start
//! of every match until the watchdog steps in. Each new version
//! tries the set rate again (it may have made the work lighter), and so do
//! new video settings. Each version also checks the saved video settings once
//! before recording (perfcheck). Kept in tuning.json next to the log.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Sessions with an overload before the lower rate sticks.
const SESSIONS: usize = 2;

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Tuning {
    /// The version that learned this.
    version: String,
    /// When each session that couldn't keep up started, oldest first.
    overloaded: Vec<String>,
    /// The frame rate to start at.
    pub fps_cap: Option<u32>,
    /// This version's check of the video settings ran.
    pub checked: bool,
    /// What that check changed, for the Klipp window to tell.
    pub adjusted: Option<Adjusted>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Adjusted {
    /// "1440p at 120 fps" before, "1080p at 60 fps" after.
    pub from: String,
    pub to: String,
}

fn path() -> Option<PathBuf> {
    // Tests don't touch the real Klipp folder.
    if cfg!(test) {
        return None;
    }
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Klipp").join("tuning.json"))
}

impl Tuning {
    /// What earlier sessions of this version learned; a new version starts over.
    pub fn load() -> Self {
        let current = env!("CARGO_PKG_VERSION");
        let loaded: Option<Tuning> = path().and_then(|p| fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok());
        match loaded {
            Some(t) if t.version == current => t,
            _ => {
                let fresh = Tuning { version: current.into(), ..Default::default() };
                fresh.save();
                fresh
            }
        }
    }

    fn save(&self) {
        if let (Some(p), Ok(body)) = (path(), serde_json::to_vec_pretty(self)) {
            let _ = fs::write(p, body);
        }
    }

    /// The card couldn't keep up in the session that started at `session`.
    /// True when that makes `cap` the rate to start at from now on.
    pub fn overloaded(&mut self, session: &str, cap: u32) -> bool {
        if !self.overloaded.iter().any(|s| s == session) {
            self.overloaded.push(session.to_string());
        }
        let learned = self.fps_cap.is_none() && self.overloaded.len() >= SESSIONS;
        if learned {
            self.fps_cap = Some(cap);
        }
        self.save();
        learned
    }

    /// The check of the video settings ran, and what it changed.
    pub fn mark_checked(&mut self, adjusted: Option<Adjusted>) {
        self.checked = true;
        self.adjusted = adjusted;
        self.save();
    }

    /// New video settings: what was learned was about the old ones.
    pub fn forget(&mut self) {
        if self.fps_cap.is_some() || !self.overloaded.is_empty() {
            self.overloaded.clear();
            self.fps_cap = None;
            self.save();
        }
    }
}
