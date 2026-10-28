//! The running recorder: owns the pipelines, game integrations and auto-clip
//! engine, and applies settings changes.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use klipp_media::StreamInfo;
use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey};

use crate::arrivals::Arrivals;
use crate::audio::AudioPipeline;
use crate::autoclip::AutoClipper;
use crate::clock;
use crate::cs2::{self, Cs2Integration};
use crate::cue::{CuePlayer, Sound};
use crate::events::{EventKind, GameEvent};
use crate::games::GameTracker;
use crate::hotkey::Hotkey;
use crate::league::{self, LeagueIntegration};
use crate::log::{info, warning};
use crate::replay::{Clip, ReplayBuffer};
use crate::rocketleague::{self, RocketLeagueIntegration};
use crate::save::{ClipKind, SaveJob, Saver};
use crate::settings::Settings;
use crate::state::Shared;
use crate::video::VideoPipeline;

/// `RegisterHotKey` id of the save hotkey.
pub const HOTKEY_SAVE: i32 = 1;

/// The frame rate Klipp drops to when the graphics card can't keep up with a
/// higher one (FIX-17): half of 120, the same as the default.
const REDUCED_FPS: u32 = 60;

/// Audio is mixed ~100 ms behind real time and AAC adds a little delay. A clip
/// is only cut once this much time has passed after its end, so every audio
/// track has reached that moment.
pub const AUDIO_CATCH_UP: i64 = clock::SECOND / 4;

pub struct Recorder {
    pub shared: Arc<Shared>,
    settings: Settings,
    video: Option<VideoPipeline>,
    audio: Option<AudioPipeline>,
    saver: Option<Saver>,
    cue: CuePlayer,
    hotkey_registered: bool,
    /// The save key is lent to the Klipp window's key test until then.
    hotkey_lent_until: Option<Instant>,
    paused: bool,
    /// Manual clipping is off and no game Klipp auto-clips is running, so
    /// nothing is recorded (D51); see `tick_standby`.
    standby: bool,
    pub games: GameTracker,
    cs2: Option<Cs2Integration>,
    league: Option<LeagueIntegration>,
    rocket_league: Option<RocketLeagueIntegration>,
    /// Events from every game integration, with the game's name.
    events_tx: Sender<(String, GameEvent)>,
    events_rx: Receiver<(String, GameEvent)>,
    autoclip: AutoClipper,
    /// Hotkey presses close together, collected into one clip.
    burst: Option<Burst>,
    /// What the Klipp window shows as arriving.
    arrivals: Arc<Arrivals>,
    /// Auto-clip chains shown as arriving, by their first event time.
    auto_arrivals: BTreeMap<i64, u64>,
    watchdog: crate::watchdog::Watchdog,
    /// Recording at this frame rate instead of the set one, for the rest of
    /// the session, because the card couldn't keep up (FIX-17).
    fps_cap: Option<u32>,
    /// What earlier sessions learned about this PC (see `tuning`).
    tuning: crate::tuning::Tuning,
    /// When this session started, to count sessions that couldn't keep up.
    session: String,
    /// Performance mode's steps, and its look at the graphics card while a
    /// game is in front (see `performance`).
    perf: crate::performance::Governor,
    perf_load: Option<crate::gpuload::GpuLoad>,
    /// The video's (frames, late, skipped) at the last look.
    perf_seen: (u64, u64, u64),
}

/// A clip saved with the hotkey that more presses can still extend. Each
/// press within the merge gap appends what was recorded since, so a burst of
/// presses becomes one clip with a marker at every press.
struct Burst {
    clip: Clip,
    streams: Vec<StreamInfo>,
    tracks: Vec<String>,
    game: String,
    presses: Vec<i64>,
    requested_at: Instant,
    arrival: u64,
}

/// The game's name in log lines: "cs2", "league", "rocket league".
fn log_tag(game: &str) -> String {
    match game {
        cs2::GAME_NAME => "cs2".into(),
        league::GAME_NAME => "league".into(),
        _ => game.to_lowercase(),
    }
}

/// Buffer window and byte cap for the current settings: the worst case at the
/// bitrate ceiling, plus room for one extra GOP.
fn buffer_limits(s: &Settings) -> (i64, u64) {
    let secs = s.replay.length_secs as u64;
    (secs as i64 * clock::SECOND, s.video.max_bitrate_mbps as u64 * 1_000_000 / 8 * (secs + 2))
}

/// The programs of the games Klipp auto-clips, with auto-clips on for them.
fn autoclipped_exes(s: &Settings) -> Vec<&'static str> {
    let a = &s.auto_clip;
    let games = [(a.cs2.enabled, "cs2.exe"), (a.league.enabled, "league of legends.exe"), (a.rocket_league.enabled, "rocketleague.exe")];
    if !a.enabled {
        return Vec::new();
    }
    games.into_iter().filter(|&(on, _)| on).map(|(_, exe)| exe).collect()
}

/// Klipp records: always with manual clipping on, otherwise only while a
/// game it auto-clips is running (`running`: lowercase program names).
fn wanted(s: &Settings, running: impl FnOnce() -> Vec<String>) -> bool {
    if s.replay.manual {
        return true;
    }
    let exes = autoclipped_exes(s);
    !exes.is_empty() && running().iter().any(|name| exes.contains(&name.as_str()))
}

fn running_programs() -> Vec<String> {
    crate::games::processes().into_iter().map(|(_, _, name)| name).collect()
}

impl Recorder {
    /// Must be called on the thread that runs the message loop (for the hotkey).
    pub fn start(settings: Settings) -> Result<Self> {
        let (window, max_bytes) = buffer_limits(&settings);
        let cue = CuePlayer::start(&settings.cue)?;
        let (events_tx, events_rx) = mpsc::channel();
        let arrivals = Arc::new(Arrivals::new());
        let mut r = Self {
            shared: Arc::new(Shared::new(ReplayBuffer::new(window, max_bytes))),
            video: None,
            audio: None,
            saver: Some(Saver::start(cue.handle(), arrivals.clone())?),
            arrivals,
            auto_arrivals: BTreeMap::new(),
            cue,
            autoclip: AutoClipper::new(settings.autoclip_rules()),
            settings,
            hotkey_registered: false,
            hotkey_lent_until: None,
            paused: false,
            standby: false,
            games: GameTracker::default(),
            cs2: None,
            league: None,
            rocket_league: None,
            events_tx,
            events_rx,
            burst: None,
            watchdog: crate::watchdog::Watchdog::default(),
            fps_cap: None,
            tuning: crate::tuning::Tuning::load(),
            session: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()).to_string(),
            perf: crate::performance::Governor::default(),
            perf_load: None,
            perf_seen: (0, 0, 0),
        };
        r.set_game_rules();
        crate::watchdog::clear_health();
        let fps = r.settings.video.fps;
        if let Some(cap) = r.tuning.fps_cap.filter(|&cap| cap < fps) {
            info!("video: games kept this graphics card too busy for {fps} fps in earlier sessions; recording at {cap} fps (Klipp tries {fps} again after its next update, or with new video settings)");
            r.fps_cap = Some(cap);
            r.watchdog.set_reduced(Some((cap, fps)), true);
        }
        if wanted(&r.settings, running_programs) {
            r.video = Some(VideoPipeline::start(r.video_settings(), r.shared.clone())?);
            r.start_audio();
        } else {
            r.standby = true;
            info!("recording: manual clipping is off, so Klipp records only while a game it auto-clips is running; none is yet");
        }
        r.register_hotkey();
        r.start_integrations();
        Ok(r)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// False if another app holds the configured hotkey (there's none to hold
    /// with manual clipping off).
    pub fn hotkey_registered(&self) -> bool {
        self.hotkey_registered || !self.settings.replay.manual
    }

    /// Not recording because manual clipping is off and no game it auto-clips is running.
    pub fn standby(&self) -> bool {
        self.standby
    }

    /// With manual clipping off, starts recording when a game Klipp
    /// auto-clips starts and stops when it's closed (every couple of seconds).
    pub fn tick_standby(&mut self) {
        let wanted = wanted(&self.settings, running_programs);
        if wanted != self.standby {
            return;
        }
        self.standby = !wanted;
        if self.paused {
            return;
        }
        if wanted {
            match VideoPipeline::start(self.video_settings(), self.shared.clone()) {
                Ok(v) => self.video = Some(v),
                Err(e) => warning!("video failed to start: {e:#}"),
            }
            self.start_audio();
            info!("recording: a game Klipp auto-clips is running; recording");
        } else {
            self.flush_burst();
            self.video = None;
            self.audio = None;
            self.shared.buffer.lock().unwrap().clear();
            info!("recording: manual clipping is off and no game Klipp auto-clips is running; recording stops until one is");
        }
    }

    /// A clip is on its way (see arrivals): not the moment to restart for an update.
    pub fn saving(&self) -> bool {
        !self.arrivals.is_empty()
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn last_clip(&self) -> Option<std::path::PathBuf> {
        self.saver.as_ref().and_then(Saver::last_clip)
    }

    /// The watchdog's look at the video (every couple of seconds): restarts it
    /// if frames stopped; tells the window if it can't keep up.
    pub fn check_health(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        let counts = crate::watchdog::Counts {
            frames: self.shared.frames.load(Relaxed),
            late: self.shared.late_frames.load(Relaxed),
            skipped: self.shared.skipped_ticks.load(Relaxed),
            blind: self.shared.capture_blind.load(Relaxed),
        };
        if self.paused || self.standby {
            self.watchdog.reset(counts);
            return;
        }
        if self.watchdog.check(Instant::now(), counts, self.video.is_some()) == crate::watchdog::Verdict::RestartVideo {
            self.restart_video();
        }
        // A game keeping the card too busy for this frame rate leaves clips
        // stuttering or as a slideshow (FIX-17). Half the rate halves Klipp's
        // work on the card; it lasts until Klipp restarts or the video
        // settings change, and the window says so.
        let fps = self.settings.video.fps;
        if self.fps_cap.is_none() && fps > REDUCED_FPS && self.video.is_some() && self.watchdog.overloaded() {
            warning!("video: the graphics card can't keep up at {fps} fps; recording at {REDUCED_FPS} fps until Klipp restarts");
            self.fps_cap = Some(REDUCED_FPS);
            self.restart_video();
            self.watchdog.reset(counts);
            let learned = self.tuning.overloaded(&self.session, REDUCED_FPS);
            if learned {
                info!("video: the second session this graphics card couldn't keep up at {fps} fps; Klipp starts at {REDUCED_FPS} fps from now on");
            }
            self.watchdog.set_reduced(Some((REDUCED_FPS, fps)), learned);
        }
    }

    /// Performance mode's look at the card, every couple of seconds: while a
    /// game is in front, a step down when it stays busy (or Klipp falls
    /// behind), a step up when it has room; full again outside games.
    pub fn tick_performance(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        let counts = (self.shared.frames.load(Relaxed), self.shared.late_frames.load(Relaxed), self.shared.skipped_ticks.load(Relaxed));
        let (frames, late, skipped) = (counts.0.saturating_sub(self.perf_seen.0), counts.1.saturating_sub(self.perf_seen.1), counts.2.saturating_sub(self.perf_seen.2));
        self.perf_seen = counts;
        let on = self.settings.video.performance != crate::settings::Performance::Off && !self.paused && self.video.is_some();
        let game = self.games.current(Duration::from_secs(5));
        if !on || game == crate::games::DESKTOP {
            // Nothing to count outside games.
            self.perf_load = None;
            if self.perf.reset().is_some() {
                self.set_performance_step(0, if on { "the game is closed or in the background" } else { "performance mode is off" });
            }
            return;
        }
        // More than one frame in twenty late or left out.
        let behind = (late + skipped) * 20 > frames + skipped;
        let busy = match &mut self.perf_load {
            Some(load) => load.sample().into_iter().find(|e| e.kind == "3D").map_or(0.0, |e| e.total),
            // The first look only starts counting.
            None => {
                self.perf_load = crate::gpuload::GpuLoad::start();
                return;
            }
        };
        let before = self.perf.level();
        let sample = crate::performance::Sample { in_game: true, busy, behind };
        if let Some(level) = self.perf.update(Instant::now(), sample) {
            let why = if level > before {
                format!("{game} keeps the graphics card {busy:.0}% busy{}", if behind { " and Klipp's frames fell behind" } else { "" })
            } else {
                format!("the graphics card has room again ({busy:.0}% busy)")
            };
            self.set_performance_step(level, &why);
        }
    }

    /// Tells the running video what performance mode asks, and logs it.
    fn set_performance_step(&self, level: usize, why: &str) {
        use std::sync::atomic::Ordering::Relaxed;
        let (stride, fps, bitrate) = crate::performance::at(level, self.video_settings().fps, self.settings.video.max_bitrate_mbps * 1_000_000);
        self.shared.perf_stride.store(stride, Relaxed);
        self.shared.perf_max_bitrate.store(if level == 0 { 0 } else { bitrate }, Relaxed);
        let last = crate::performance::STEPS.len() - 1;
        let step = if level == 0 { "full".to_string() } else { format!("step {level} of {last}") };
        info!("performance: {why}; recording {} ({step})", self.step_words(fps, bitrate));
    }

    /// "30 fps, up to 20 Mbit/s"; AMF keeps its bitrate while recording.
    fn step_words(&self, fps: u32, bitrate: u32) -> String {
        if self.shared.bitrate_adjustable.load(std::sync::atomic::Ordering::Relaxed) {
            format!("{fps} fps, up to {} Mbit/s", bitrate / 1_000_000)
        } else {
            format!("{fps} fps (this encoder keeps its bitrate while recording)")
        }
    }

    /// Performance mode's step for the activity line, when it isn't full.
    pub fn performance_note(&self) -> Option<String> {
        let level = self.perf.level();
        (level > 0).then(|| {
            let (_, fps, bitrate) = crate::performance::at(level, self.video_settings().fps, self.settings.video.max_bitrate_mbps * 1_000_000);
            format!("performance step {level}: {}", self.step_words(fps, bitrate))
        })
    }

    /// The video settings in effect: the user's, with the frame rate capped
    /// if the card couldn't keep up.
    fn video_settings(&self) -> crate::video::VideoSettings {
        let mut v = self.settings.video_settings();
        if let Some(cap) = self.fps_cap {
            v.fps = v.fps.min(cap);
        }
        v
    }

    fn restart_video(&mut self) {
        // A new encoder's packets can't continue the old ones in a clip.
        self.flush_burst();
        self.video = None;
        self.shared.buffer.lock().unwrap().clear();
        match VideoPipeline::start(self.video_settings(), self.shared.clone()) {
            Ok(v) => self.video = Some(v),
            Err(e) => warning!("watchdog: the video didn't restart ({e:#}); trying again later"),
        }
    }

    /// Pausing stops capture and forgets what was buffered, for moments that
    /// shouldn't be recorded at all. Resuming starts fresh.
    pub fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        if paused {
            self.flush_burst();
            self.video = None;
            self.audio = None;
            self.shared.buffer.lock().unwrap().clear();
            info!("recording paused; buffer cleared");
        } else if self.standby {
            info!("recording resumed; it starts once a game Klipp auto-clips runs");
        } else {
            match VideoPipeline::start(self.video_settings(), self.shared.clone()) {
                Ok(v) => self.video = Some(v),
                Err(e) => warning!("video failed to resume: {e:#}"),
            }
            self.start_audio();
            info!("recording resumed");
        }
    }

    /// Applies new settings, restarting only what changed.
    pub fn apply(&mut self, new: Settings) {
        let old = std::mem::replace(&mut self.settings, new);
        let limits_changed = old.replay.length_secs != self.settings.replay.length_secs
            || old.video.max_bitrate_mbps != self.settings.video.max_bitrate_mbps;
        let hotkey_changed = old.replay.hotkey != self.settings.replay.hotkey || old.replay.manual != self.settings.replay.manual;
        let video_changed = old.video != self.settings.video;
        // Also when moving the save sound changes whether a track leaves it out.
        let audio_changed = old.audio != self.settings.audio || old.audio_sources() != self.settings.audio_sources();
        let (was, is) = (&old.auto_clip, &self.settings.auto_clip);
        let all_changed = was.enabled != is.enabled;
        let cs2_changed = all_changed || was.cs2.enabled != is.cs2.enabled || was.cs2.port != is.cs2.port || was.cs2.debug_log != is.cs2.debug_log;
        let league_changed = all_changed || was.league.enabled != is.league.enabled;
        let rocket_league_changed = all_changed || was.rocket_league.enabled != is.rocket_league.enabled;

        if limits_changed {
            let (window, max_bytes) = buffer_limits(&self.settings);
            self.shared.buffer.lock().unwrap().set_limits(window, max_bytes);
        }
        if hotkey_changed {
            self.unregister_hotkey();
            self.hotkey_lent_until = None;
            self.register_hotkey();
        }
        self.cue.handle().update(&self.settings.cue);
        self.autoclip.set_rules(self.settings.autoclip_rules());
        self.set_game_rules();
        // Each restarts on its own, so a CS2 change doesn't interrupt a League game.
        if cs2_changed {
            self.cs2 = None;
            self.start_cs2();
        }
        if league_changed {
            self.league = None;
            self.start_league();
        }
        if rocket_league_changed {
            self.rocket_league = None;
            self.start_rocket_league();
        }
        // While paused or standing by nothing is running; starting again uses the new settings.
        if (video_changed || audio_changed) && !self.paused && !self.standby {
            // New streams can't continue a clip from the old ones.
            self.flush_burst();
            if video_changed {
                self.video = None;
            }
            if audio_changed {
                self.audio = None;
            }
            // Buffered packets don't match the new streams, so the buffer starts over.
            self.shared.buffer.lock().unwrap().clear();
            if video_changed {
                // New video settings get a fresh try at their own frame rate.
                self.fps_cap = None;
                self.tuning.forget();
                self.watchdog.set_reduced(None, false);
                if self.perf.reset().is_some() {
                    self.set_performance_step(0, "new video settings");
                }
                match VideoPipeline::start(self.video_settings(), self.shared.clone()) {
                    Ok(v) => self.video = Some(v),
                    Err(e) => warning!("video failed to restart with the new settings: {e:#}"),
                }
            }
            if audio_changed {
                self.start_audio();
            }
        }
        info!("settings applied");
    }

    /// Feedback for a hotkey press, right away: the save sound (restarting if
    /// it's still playing), or the error sound if there's nothing to save.
    /// False in that second case (for the on-screen indicator).
    pub fn press(&self) -> bool {
        let empty = self.shared.buffer.lock().unwrap().duration() == 0;
        let ok = !self.paused && !empty;
        self.cue.handle().play(if ok { Sound::Saved } else { Sound::Error });
        ok
    }

    /// Saves the last `replay.length_secs` seconds, ending at `end` (shared
    /// clock). A press within the merge gap of the previous one extends that
    /// clip instead; it's written once the presses stop (see `tick_autoclip`).
    pub fn save(&mut self, end: i64) {
        if self.paused {
            info!("recording is paused; nothing to save");
            return;
        }
        let window = self.settings.replay.length_secs as i64 * clock::SECOND;
        let gap = self.settings.autoclip_rules().merge_gap;
        if let Some(b) = &mut self.burst {
            let last = b.presses.last().copied().unwrap_or(end);
            // Capped so a held-down habit can't grow one clip without end.
            let fits = end - last <= gap && end - b.clip.start_time <= 2 * window;
            if fits && self.shared.buffer.lock().unwrap().extend(&mut b.clip, end) {
                b.presses.push(end);
                let n = b.presses.len() as u32;
                self.arrivals.update(b.arrival, |a| a.presses = n);
                info!("hotkey again within {} s: extending the clip ({} presses)", gap / clock::SECOND, b.presses.len());
                return;
            }
        }
        self.flush_burst();
        let Some((streams, tracks)) = self.streams() else { return };
        let Some(clip) = self.shared.buffer.lock().unwrap().snapshot(end, window) else {
            warning!("nothing to save yet");
            return;
        };
        let game = self.games.current(Duration::from_secs(self.settings.replay.length_secs as u64));
        let arrival = self.arrivals.add("manual", &game, None);
        self.burst = Some(Burst { clip, streams, tracks, game, presses: vec![end], requested_at: Instant::now(), arrival });
    }

    /// Writes the hotkey clip being collected, with a marker at each press.
    fn flush_burst(&mut self) {
        let Some(b) = self.burst.take() else { return };
        let (start, end) = (b.clip.start_time, b.presses.last().copied().unwrap_or(b.clip.start_time));
        let mut events = self.autoclip.markers(&b.game, start, end, None);
        events.extend(b.presses.iter().map(|&t| GameEvent::new(EventKind::Capture, t, "")));
        events.sort_by_key(|e| e.time);
        // Just before the first press, which is usually what it was pressed for.
        let thumb_at = b.presses[0] - 3 * clock::SECOND;
        self.submit(b.clip, b.streams, b.tracks, b.game, ClipKind::Manual, None, events, thumb_at, b.requested_at, Some(b.arrival));
    }

    /// Takes in new game events and saves auto-clips whose chains have finished,
    /// and hotkey clips once the presses have stopped.
    /// Call regularly (a few times a second) from the message loop.
    pub fn tick_autoclip(&mut self) {
        while let Ok((game, e)) = self.events_rx.try_recv() {
            let detail = if e.detail.is_empty() { String::new() } else { format!(" ({})", e.detail) };
            info!("{}: {}{detail}", log_tag(&game), e.kind.label_in(&game));
            // Always recorded (for markers in manual clips); clips only when enabled.
            self.autoclip.on_event(&game, e);
        }
        let now = clock::now() - AUDIO_CATCH_UP;
        let gap = self.settings.autoclip_rules().merge_gap;
        if self.burst.as_ref().is_some_and(|b| b.presses.last().is_some_and(|&last| now - last > gap)) {
            self.flush_burst();
        }
        self.sync_auto_arrivals();
        for clip in self.autoclip.ready(now) {
            let arrival = self.auto_arrivals.remove(&clip.key);
            if self.paused || !self.settings.auto_clip.enabled {
                if let Some(id) = arrival {
                    self.arrivals.remove(id);
                }
                continue;
            }
            // The buffer only holds `length_secs`; a longer streak keeps its end.
            let window = self.settings.replay.length_secs as i64 * clock::SECOND;
            let start = clip.start.max(clip.end - window);
            let events = self.autoclip.markers(&clip.game, start, clip.end, Some((clip.reason, clip.reason_time)));
            let snapshot = self.streams().zip(self.shared.buffer.lock().unwrap().snapshot(clip.end, clip.end - start));
            let Some(((streams, tracks), snapshot)) = snapshot else {
                if let Some(id) = arrival {
                    self.arrivals.remove(id);
                }
                continue;
            };
            self.submit(snapshot, streams, tracks, clip.game, ClipKind::Auto, Some(clip.reason), events, clip.reason_time, Instant::now(), arrival);
        }
    }

    /// Shows auto-clips as arriving as soon as their events make them a clip
    /// (it's written once the merge gap passes), named after the best reason so far.
    fn sync_auto_arrivals(&mut self) {
        let active = !self.paused && self.settings.auto_clip.enabled;
        let pending = if active { self.autoclip.pending() } else { Vec::new() };
        for (key, game, reason) in &pending {
            let label = Some(reason.label_in(game));
            match self.auto_arrivals.get(key) {
                Some(&id) => self.arrivals.update(id, |a| a.label = label),
                None => {
                    let id = self.arrivals.add("auto", game, label);
                    self.auto_arrivals.insert(*key, id);
                }
            }
        }
        // This runs before finished chains are taken out (`ready`), so a chain
        // only drops out here when a settings change or a pause means it won't be a clip.
        let gone: Vec<i64> = self.auto_arrivals.keys().filter(|k| !pending.iter().any(|(p, ..)| p == *k)).copied().collect();
        for key in gone {
            if let Some(id) = self.auto_arrivals.remove(&key) {
                self.arrivals.remove(id);
            }
        }
    }

    /// The streams a clip is written with now, and their track names.
    fn streams(&self) -> Option<(Vec<StreamInfo>, Vec<String>)> {
        let video = self.shared.video_stream.lock().unwrap().clone()?;
        let mut streams = vec![video.as_default()];
        streams.extend(self.shared.audio_streams.lock().unwrap().iter().cloned());
        let tracks = if streams.len() > 1 {
            std::iter::once("Mix".to_string()).chain(self.settings.audio.sources.iter().map(|s| s.name.clone())).collect()
        } else {
            Vec::new()
        };
        Some((streams, tracks))
    }

    #[allow(clippy::too_many_arguments)]
    fn submit(
        &self,
        clip: Clip,
        streams: Vec<StreamInfo>,
        tracks: Vec<String>,
        game: String,
        kind: ClipKind,
        reason: Option<EventKind>,
        events: Vec<GameEvent>,
        thumb_at: i64,
        requested_at: Instant,
        arrival: Option<u64>,
    ) {
        info!(
            "saving {game} clip{}... ({:.1} MB copied out of the buffer)",
            reason.map_or(String::new(), |r| format!(" ({})", r.label_in(&game))),
            clip.data.len() as f64 / 1e6,
        );
        // The map, if the game reported one during this clip (or just before it).
        let start = clip.start_time;
        let context = match game.as_str() {
            cs2::GAME_NAME => self.cs2.as_ref().map(|c| c.context.lock().unwrap().clone()),
            league::GAME_NAME => self.league.as_ref().map(|l| l.context.lock().unwrap().clone()),
            _ => None,
        }
        .filter(|c| c.seen >= start - 30 * clock::SECOND)
        .unwrap_or_default();
        if let Some(saver) = &self.saver {
            saver.submit(SaveJob {
                clip,
                streams,
                tracks,
                clips_dir: self.settings.storage.clips_dir(),
                game,
                kind,
                reason,
                events,
                map: context.map,
                mode: context.mode,
                thumb_at,
                arrival,
                requested_at,
            });
        }
    }
    fn start_audio(&mut self) {
        let sources = self.settings.audio_sources();
        if sources.is_empty() {
            self.shared.audio_streams.lock().unwrap().clear();
            return;
        }
        match AudioPipeline::start(sources, self.shared.clone()) {
            Ok(a) => self.audio = Some(a),
            Err(e) => {
                warning!("audio failed to start: {e:#}");
                self.shared.audio_streams.lock().unwrap().clear();
            }
        }
    }

    fn start_integrations(&mut self) {
        self.start_cs2();
        self.start_league();
        self.start_rocket_league();
    }

    fn start_cs2(&mut self) {
        let a = &self.settings.auto_clip;
        if !(a.enabled && a.cs2.enabled) {
            return;
        }
        match Cs2Integration::start(a.cs2.port, a.cs2.debug_log, self.events_tx.clone()) {
            Ok(i) => self.cs2 = i,
            Err(e) => warning!("cs2: integration failed to start: {e:#}"),
        }
    }

    fn start_league(&mut self) {
        let a = &self.settings.auto_clip;
        if !(a.enabled && a.league.enabled) {
            return;
        }
        match LeagueIntegration::start(self.events_tx.clone()) {
            Ok(i) => self.league = Some(i),
            Err(e) => warning!("league: integration failed to start: {e:#}"),
        }
    }

    fn start_rocket_league(&mut self) {
        let a = &self.settings.auto_clip;
        if !(a.enabled && a.rocket_league.enabled) {
            return;
        }
        match RocketLeagueIntegration::start(self.events_tx.clone()) {
            Ok(i) => self.rocket_league = i,
            Err(e) => warning!("rocket league: integration failed to start: {e:#}"),
        }
    }

    /// League's and Rocket League's own rules, with the shared timing.
    fn set_game_rules(&mut self) {
        let base = self.settings.autoclip_rules();
        let a = &self.settings.auto_clip;
        self.autoclip.set_game_rules(league::GAME_NAME, league::rules(&base, &a.league));
        self.autoclip.set_game_rules(rocketleague::GAME_NAME, rocketleague::rules(&base, &a.rocket_league));
    }

    fn register_hotkey(&mut self) {
        let text = self.settings.replay.hotkey.clone();
        if !self.settings.replay.manual {
            // Nothing to warn about in the window: the key is off on purpose.
            crate::hotkey::write_state(&text, true);
            info!("hotkey: manual clipping is off; {text} does nothing");
            return;
        }
        if self.try_hotkey() {
            info!("hotkey: {text} saves clips");
        } else {
            warning!("hotkey: {text} is already taken by another app (SteelSeries Moments or NVIDIA Instant Replay?); trying again every few seconds");
        }
    }

    /// Registers the save key; false if another app holds it. Tells the
    /// Klipp window either way.
    fn try_hotkey(&mut self) -> bool {
        let text = &self.settings.replay.hotkey;
        let Ok(h) = Hotkey::parse(text) else { return false };
        self.hotkey_registered = unsafe { RegisterHotKey(None, HOTKEY_SAVE, h.modifiers | MOD_NOREPEAT, h.vk) }.is_ok();
        crate::hotkey::write_state(text, self.hotkey_registered);
        self.hotkey_registered
    }

    /// A key another app held may be free now (it closed, or let go of it),
    /// and a lent key comes back. Call every few seconds.
    pub fn retry_hotkey(&mut self) {
        if !self.settings.replay.manual || self.hotkey_registered || self.hotkey_lent_until.is_some_and(|t| Instant::now() < t) {
            return;
        }
        let lent = self.hotkey_lent_until.take().is_some();
        if self.try_hotkey() && !lent {
            info!("hotkey: {} is free now and saves clips", self.settings.replay.hotkey);
        }
    }

    /// The Klipp window tests the save key (setup's Test button): let go of
    /// it for a moment; `retry_hotkey` takes it back afterwards.
    pub fn lend_hotkey(&mut self, time: Duration) {
        self.unregister_hotkey();
        self.hotkey_lent_until = Some(Instant::now() + time);
        info!("hotkey: {} lent to the Klipp window for a key test", self.settings.replay.hotkey);
    }

    fn unregister_hotkey(&mut self) {
        if std::mem::take(&mut self.hotkey_registered) {
            let _ = unsafe { UnregisterHotKey(None, HOTKEY_SAVE) };
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.unregister_hotkey();
        // A hotkey clip still collecting presses is written before quitting.
        self.flush_burst();
        self.cs2 = None;
        self.league = None;
        self.rocket_league = None;
        self.video = None;
        self.audio = None;
        // Finishes any queued clips (and their sounds) before the cue player stops.
        self.saver = None;
    }
}
