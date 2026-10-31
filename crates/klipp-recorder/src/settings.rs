//! Settings from `%APPDATA%\Klipp\settings.toml`. Written with defaults
//! on first run; the recorder reloads it whenever the file changes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use klipp_media::VideoCodec;
use serde::Deserialize;

use crate::audio::{AudioSourceConfig, Programs};
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
    pub indicators: Indicators,
    pub auto_clip: AutoClip,
    /// Extra exe name → folder name mappings.
    pub games: BTreeMap<String, String>,
}

/// The on-screen indicators (see `overlay`). Both off unless switched on.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Indicators {
    /// "Auto-clips ON" when a match starts in a game Klipp auto-clips.
    pub autoclips_on: bool,
    /// A mark when a clip is saved, or can't be.
    pub saves: bool,
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
    pub league: League,
    pub rocket_league: RocketLeague,
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
            league: League::default(),
            rocket_league: RocketLeague::default(),
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

/// League of Legends. Multi-kills are always counted the game's way.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct League {
    pub enabled: bool,
    pub multi_kill_min: u8,
    pub any_kill: bool,
    pub first_blood: bool,
    pub death: bool,
    /// A Baron, Dragon or Herald you stole.
    pub steal: bool,
    /// Every objective you took part in (they're always marked).
    pub objective: bool,
    /// Your team killed the whole enemy team, with you in the fight.
    pub ace: bool,
}

impl Default for League {
    fn default() -> Self {
        Self {
            enabled: true,
            multi_kill_min: 2,
            any_kill: false,
            first_blood: true,
            death: false,
            steal: true,
            objective: false,
            ace: true,
        }
    }
}

/// Rocket League. Assists and saves are always markers; these make them clips too.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RocketLeague {
    pub enabled: bool,
    pub goal: bool,
    pub assist: bool,
    pub save: bool,
    pub epic_save: bool,
    pub demolition: bool,
}

impl Default for RocketLeague {
    fn default() -> Self {
        Self { enabled: true, goal: true, assist: false, save: false, epic_save: true, demolition: true }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Timing {
    pub pre_roll_secs: Option<f32>,
    pub post_roll_secs: Option<f32>,
}

/// Event names allowed under `[auto_clip.timing]`.
const TIMING_EVENTS: &[&str] = &[
    "kill", "headshot", "2k", "3k", "4k", "ace", "multikill", "knife", "zeus", "death", "mvp", "penta", "firstblood", "steal", "objective",
    "teamace", "assist", "goal", "save", "epicsave", "demolition",
];

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Replay {
    pub length_secs: u32,
    pub hotkey: String,
    /// Manual clipping: the hotkey works anywhere, so Klipp records all the
    /// time. Off: no hotkey, and Klipp records only while a game it
    /// auto-clips is running (`recorder::wanted`).
    pub manual: bool,
}

impl Default for Replay {
    fn default() -> Self {
        Self { length_secs: 60, hotkey: "F8".into(), manual: true }
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
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
}

/// How the screen is captured.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMethod {
    /// Desktop Duplication on Windows 10 (no yellow border there), Windows
    /// Graphics Capture on Windows 11.
    Auto,
    /// Windows Graphics Capture.
    Wgc,
    /// DXGI Desktop Duplication.
    Duplication,
    /// The least work on the graphics card: Windows Graphics Capture's own
    /// pictures go straight to the encoder's conversion, no copy, about as
    /// many as are recorded (`capture`). Desktop Duplication on Windows 10.
    Light,
}

/// Performance mode: how much Klipp steps back while a game is in front.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Performance {
    /// The video settings, always.
    #[default]
    Off,
    /// In games, a lower frame rate and bitrate while the graphics card is
    /// busy, back up when it has room (`performance`).
    On,
    /// That, recorded the lightest way this PC has: the light capture and the
    /// encoder's fastest preset.
    Plus,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Video {
    pub codec: Codec,
    pub resolution: Resolution,
    pub fps: u32,
    pub quality: u8,
    pub max_bitrate_mbps: u32,
    /// The monitor to record ("" = the main display).
    pub monitor: String,
    pub capture: CaptureMethod,
    /// Part of a graphics card's name to encode on instead of the one that
    /// drives the screen ("" = that one). For testing.
    pub gpu: String,
    pub performance: Performance,
    /// The encoder's fastest preset (Performance+ always uses it).
    pub fast_encoder: bool,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            codec: Codec::Av1,
            resolution: Resolution::Native,
            fps: 60,
            quality: 30,
            max_bitrate_mbps: 40,
            monitor: String::new(),
            capture: CaptureMethod::Auto,
            gpu: String::new(),
            performance: Performance::Off,
            fast_encoder: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Loopback,
    Capture,
    /// Only the Discord app (experimental); the first loopback track then leaves Discord out.
    Discord,
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

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Storage {
    /// Empty means Videos\Klipp; use `clips_dir()` for the actual folder.
    pub clips_dir: PathBuf,
    /// Deletes old clips by itself (`cleanup`); favorites are never deleted.
    pub auto_delete: bool,
    /// Clips older than this many days go (0: no age limit).
    pub delete_after_days: u32,
    /// The oldest clips go while the clips take more than this many GB (0: no limit).
    pub max_gb: u32,
}

impl Default for Storage {
    fn default() -> Self {
        Self { clips_dir: PathBuf::new(), auto_delete: false, delete_after_days: 30, max_gb: 0 }
    }
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
    /// Ignored since 0.2.1, when custom sounds were dropped (Klipp plays its
    /// own). Still accepted, so settings files that have them keep loading.
    #[serde(rename = "sound")]
    _sound: toml::Value,
    #[serde(rename = "auto_sound")]
    _auto_sound: toml::Value,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_clips: true,
            volume: 0.6,
            device: String::new(),
            _sound: toml::Value::String(String::new()),
            _auto_sound: toml::Value::String(String::new()),
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
        clamp_to("storage.delete_after_days", &mut self.storage.delete_after_days, 0, 3650, &mut w);
        clamp_to("storage.max_gb", &mut self.storage.max_gb, 0, 100_000, &mut w);
        let a = &mut self.auto_clip;
        clamp_to("auto_clip.pre_roll_secs", &mut a.pre_roll_secs, 0.0, 120.0, &mut w);
        clamp_to("auto_clip.post_roll_secs", &mut a.post_roll_secs, 0.0, 60.0, &mut w);
        clamp_to("auto_clip.merge_gap_secs", &mut a.merge_gap_secs, 1.0, 60.0, &mut w);
        for (game, min) in [("cs2", &mut a.cs2.multi_kill_min), ("league", &mut a.league.multi_kill_min)] {
            if *min == 1 || *min > 5 {
                w.push(format!("auto_clip.{game}.multi_kill_min {min} should be 0 (off) or 2-5; using 2"));
                *min = 2;
            }
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
            w.push(format!("replay.hotkey \"{}\" isn't a valid hotkey; using F8", self.replay.hotkey));
            self.replay.hotkey = Replay::default().hotkey;
        }
        if ![30, 60, 120].contains(&self.video.fps) {
            w.push(format!("video.fps {} isn't 30, 60 or 120; using 60", self.video.fps));
            self.video.fps = 60;
        }
        w
    }

    /// Writes `video.performance` into settings.toml (the tray's Performance
    /// mode menu), keeping everything else; the recorder applies it as any change.
    pub fn save_performance(path: &Path, mode: Performance) -> Result<()> {
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut doc: toml_edit::DocumentMut = text.parse().context("settings.toml")?;
        if !doc.contains_table("video") {
            doc["video"] = toml_edit::table();
        }
        let value = match mode {
            Performance::Off => "off",
            Performance::On => "on",
            Performance::Plus => "plus",
        };
        doc["video"]["performance"] = toml_edit::value(value);
        fs::write(path, doc.to_string()).with_context(|| format!("writing {}", path.display()))
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
                Resolution::P1440 => Some(1440),
                Resolution::P1080 => Some(1080),
                Resolution::P720 => Some(720),
            },
            quality: v.quality,
            max_mbps: v.max_bitrate_mbps,
            monitor: v.monitor.clone(),
            // Performance+ records the lightest way there is.
            capture: if v.performance == Performance::Plus { CaptureMethod::Light } else { v.capture },
            gpu: v.gpu.clone(),
            light_encoder: v.performance == Performance::Plus || v.fast_encoder,
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
            victory: false,
            knock: false,
            first_blood: false,
            steal: false,
            objective: false,
            team_ace: false,
            goal: false,
            assist: false,
            save: false,
            epic_save: false,
            demolition: false,
            pre_roll: ticks(a.pre_roll_secs),
            post_roll: ticks(a.post_roll_secs),
            merge_gap: ticks(a.merge_gap_secs),
            max_chain: 0,
            lateness: 0,
            timing: a.timing.iter().map(|(k, t)| (k.clone(), (t.pre_roll_secs.map(ticks), t.post_roll_secs.map(ticks)))).collect(),
        }
    }

    pub fn audio_sources(&self) -> Vec<AudioSourceConfig> {
        // With a Discord track, the first track of what the PC plays leaves Discord out.
        let discord = self.audio.sources.iter().any(|s| s.kind == Kind::Discord);
        let first_loopback = self.audio.sources.iter().position(|s| s.kind == Kind::Loopback);
        self.audio
            .sources
            .iter()
            .enumerate()
            .map(|(i, s)| AudioSourceConfig {
                title: s.name.clone(),
                device: s.device.clone(),
                kind: if s.kind == Kind::Capture { SourceKind::Capture } else { SourceKind::Loopback },
                programs: match s.kind {
                    Kind::Discord => Programs::Discord,
                    Kind::Loopback if discord && Some(i) == first_loopback => Programs::AllButDiscord,
                    // The save sound plays on this device: the same name ("" is the Windows default for both).
                    Kind::Loopback if (self.cue.enabled || self.cue.auto_clips) && s.device.trim().eq_ignore_ascii_case(self.cue.device.trim()) => Programs::AllButKlipp,
                    _ => Programs::Device,
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
