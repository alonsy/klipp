//! Settings from `%APPDATA%\Klipp\settings.toml`. Written with defaults
//! on first run; the recorder reloads it whenever the file changes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use klipp_media::VideoCodec;
use serde::Deserialize;

use crate::audio::AudioSourceConfig;
use crate::autoclip::Rules;
use crate::clock;
use crate::hotkey::Hotkey;
use crate::video::VideoSettings;
use crate::wasapi::SourceKind;

/// Written on first run. Must match `Settings::default()` (checked by a test).
pub const TEMPLATE: &str = include_str!("../default-settings.toml");

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub replay: Replay,
    pub video: Video,
    pub audio: Audio,
    pub storage: Storage,
    pub cue: Cue,
    pub auto_clip: AutoClip,
    /// Extra exe name → folder name mappings.
    pub games: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AutoClip {
    pub enabled: bool,
    pub pre_roll_secs: f32,
    pub post_roll_secs: f32,
    /// Events closer together than this end up in the same clip.
    pub merge_gap_secs: f32,
    pub cs2: Cs2,
    /// Per-event overrides, keyed by event name.
    pub timing: BTreeMap<String, Timing>,
}

impl Default for AutoClip {
    fn default() -> Self {
        Self {
            enabled: true,
            pre_roll_secs: 8.0,
            post_roll_secs: 4.0,
            merge_gap_secs: 12.0,
            cs2: Cs2::default(),
            timing: BTreeMap::new(),
        }
    }
}

/// What makes kills a multi-kill.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MultiKillMode {
    /// Kills close together (within the merge gap): a highlight.
    Streak,
    /// Kills anywhere in the same round, like the scoreboard.
    Round,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Cs2 {
    pub enabled: bool,
    pub headshot: bool,
    pub multi_kill_min: u8,
    pub multi_kill_mode: MultiKillMode,
    pub any_kill: bool,
    pub knife_or_zeus_kill: bool,
    pub death: bool,
    pub mvp: bool,
    pub port: u16,
    /// Log every game-state update to cs2.log, for timing checks.
    pub debug_log: bool,
}

impl Default for Cs2 {
    fn default() -> Self {
        Self {
            enabled: true,
            headshot: true,
            multi_kill_min: 2,
            multi_kill_mode: MultiKillMode::Streak,
            any_kill: false,
            knife_or_zeus_kill: false,
            death: false,
            mvp: false,
            port: 52741,
            debug_log: false,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Timing {
    pub pre_roll_secs: Option<f32>,
    pub post_roll_secs: Option<f32>,
}

/// Event names allowed under `[auto_clip.timing]`.
const TIMING_EVENTS: &[&str] = &["kill", "headshot", "2k", "3k", "4k", "ace", "multikill", "knife", "zeus", "death", "mvp"];

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Replay {
    pub length_secs: u32,
    pub hotkey: String,
}

impl Default for Replay {
    fn default() -> Self {
        Self { length_secs: 60, hotkey: "Alt+F10".into() }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    Av1,
    H264,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum Resolution {
    #[serde(rename = "native")]
    Native,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Video {
    pub codec: Codec,
    pub resolution: Resolution,
    pub fps: u32,
    pub quality: u8,
    pub max_bitrate_mbps: u32,
}

impl Default for Video {
    fn default() -> Self {
        Self { codec: Codec::Av1, resolution: Resolution::Native, fps: 60, quality: 30, max_bitrate_mbps: 40 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Loopback,
    Capture,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub name: String,
    pub device: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Audio {
    pub sources: Vec<Source>,
}

impl Default for Audio {
    fn default() -> Self {
        let source = |name: &str, device: &str, kind| Source { name: name.into(), device: device.into(), kind };
        Self {
            sources: vec![source("PC audio", "", Kind::Loopback)],
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Storage {
    /// Empty means Videos\Klipp; use `clips_dir()` for the actual folder.
    pub clips_dir: PathBuf,
}

impl Storage {
    /// The folder clips are saved in.
    pub fn clips_dir(&self) -> PathBuf {
        if self.clips_dir.as_os_str().is_empty() {
            let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
            home.join("Videos").join("Klipp")
        } else {
            self.clips_dir.clone()
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Cue {
    pub enabled: bool,
    /// Also play for auto-clips.
    pub auto_clips: bool,
    pub volume: f32,
    pub device: String,
    /// Custom WAV; empty means the built-in chime.
    pub sound: String,
    /// Custom WAV for auto-clips; empty means the same as `sound`.
    pub auto_sound: String,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_clips: true,
            volume: 0.6,
            device: String::new(),
            sound: String::new(),
            auto_sound: String::new(),
        }
    }
}

impl Settings {
    /// `%APPDATA%\Klipp\settings.toml`.
    pub fn default_path() -> Result<PathBuf> {
        let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
        Ok(PathBuf::from(appdata).join("Klipp").join("settings.toml"))
    }

    /// Reads `path`, or creates it with the commented defaults if it doesn't exist.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if !path.exists() {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(path, TEMPLATE).with_context(|| format!("writing {}", path.display()))?;
        }
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut s: Settings = toml::from_str(text)?;
        for warning in s.clamp() {
            crate::log::warning!("settings: {warning}");
        }
        Ok(s)
    }

    /// Pulls out-of-range values back into range. Returns what was changed.
    fn clamp(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        clamp_to("replay.length_secs", &mut self.replay.length_secs, 10, 300, &mut w);
        clamp_to("video.quality", &mut self.video.quality, 1, 51, &mut w);
        clamp_to("video.max_bitrate_mbps", &mut self.video.max_bitrate_mbps, 5, 150, &mut w);
        clamp_to("cue.volume", &mut self.cue.volume, 0.0, 1.0, &mut w);
        let a = &mut self.auto_clip;
        clamp_to("auto_clip.pre_roll_secs", &mut a.pre_roll_secs, 0.0, 120.0, &mut w);
        clamp_to("auto_clip.post_roll_secs", &mut a.post_roll_secs, 0.0, 60.0, &mut w);
        clamp_to("auto_clip.merge_gap_secs", &mut a.merge_gap_secs, 1.0, 60.0, &mut w);
        if a.cs2.multi_kill_min == 1 || a.cs2.multi_kill_min > 5 {
            w.push(format!("auto_clip.cs2.multi_kill_min {} should be 0 (off) or 2-5; using 2", a.cs2.multi_kill_min));
            a.cs2.multi_kill_min = 2;
        }
        a.timing.retain(|name, _| {
            let known = TIMING_EVENTS.contains(&name.as_str());
            if !known {
                w.push(format!("auto_clip.timing.{name}: unknown event (known: {}); ignored", TIMING_EVENTS.join(", ")));
            }
            known
        });
        for (name, t) in &mut a.timing {
            if let Some(p) = &mut t.pre_roll_secs {
                clamp_to(&format!("auto_clip.timing.{name}.pre_roll_secs"), p, 0.0, 120.0, &mut w);
            }
            if let Some(p) = &mut t.post_roll_secs {
                clamp_to(&format!("auto_clip.timing.{name}.post_roll_secs"), p, 0.0, 60.0, &mut w);
            }
        }
        if Hotkey::parse(&self.replay.hotkey).is_err() {
            w.push(format!("replay.hotkey \"{}\" isn't a valid hotkey; using Alt+F10", self.replay.hotkey));
            self.replay.hotkey = Replay::default().hotkey;
        }
        if ![30, 60, 120].contains(&self.video.fps) {
            w.push(format!("video.fps {} isn't 30, 60 or 120; using 60", self.video.fps));
            self.video.fps = 60;
        }
        w
    }

    pub fn video_settings(&self) -> VideoSettings {
        let v = &self.video;
        VideoSettings {
            codec: match v.codec {
                Codec::Av1 => VideoCodec::Av1,
                Codec::H264 => VideoCodec::H264,
            },
            fps: v.fps,
            height: match v.resolution {
                Resolution::Native => None,
                Resolution::P1080 => Some(1080),
                Resolution::P720 => Some(720),
            },
            quality: v.quality,
            max_mbps: v.max_bitrate_mbps,
        }
    }

    pub fn autoclip_rules(&self) -> Rules {
        let a = &self.auto_clip;
        let ticks = |secs: f32| (secs as f64 * clock::SECOND as f64) as i64;
        Rules {
            headshot: a.cs2.headshot,
            multi_kill_min: a.cs2.multi_kill_min,
            multi_kill_mode: a.cs2.multi_kill_mode,
            any_kill: a.cs2.any_kill,
            knife_or_zeus_kill: a.cs2.knife_or_zeus_kill,
            death: a.cs2.death,
            mvp: a.cs2.mvp,
            pre_roll: ticks(a.pre_roll_secs),
            post_roll: ticks(a.post_roll_secs),
            merge_gap: ticks(a.merge_gap_secs),
            timing: a.timing.iter().map(|(k, t)| (k.clone(), (t.pre_roll_secs.map(ticks), t.post_roll_secs.map(ticks)))).collect(),
        }
    }

    pub fn audio_sources(&self) -> Vec<AudioSourceConfig> {
        self.audio
            .sources
            .iter()
            .map(|s| AudioSourceConfig {
                title: s.name.clone(),
                device: s.device.clone(),
                kind: match s.kind {
                    Kind::Loopback => SourceKind::Loopback,
                    Kind::Capture => SourceKind::Capture,
                },
            })
            .collect()
    }
}

fn clamp_to<T: PartialOrd + Copy + std::fmt::Display>(name: &str, value: &mut T, lo: T, hi: T, warnings: &mut Vec<String>) {
    if *value < lo || *value > hi {
        let fixed = if *value < lo { lo } else { hi };
        warnings.push(format!("{name} = {value} is outside {lo}-{hi}; using {fixed}"));
        *value = fixed;
    }
}
