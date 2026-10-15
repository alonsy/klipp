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
use crate::log::{info, warning};
use crate::replay::{Clip, ReplayBuffer};
use crate::save::{ClipKind, SaveJob, Saver};
use crate::settings::Settings;
use crate::state::Shared;
use crate::video::VideoPipeline;

/// `RegisterHotKey` id of the save hotkey.
pub const HOTKEY_SAVE: i32 = 1;

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
    paused: bool,
    pub games: GameTracker,
    cs2: Option<Cs2Integration>,
    events_tx: Sender<GameEvent>,
    events_rx: Receiver<GameEvent>,
    autoclip: AutoClipper,
    /// Hotkey presses close together, collected into one clip.
    burst: Option<Burst>,
    /// What the Klipp window shows as arriving.
    arrivals: Arc<Arrivals>,
    /// Auto-clip chains shown as arriving, by their first event time.
    auto_arrivals: BTreeMap<i64, u64>,
    watchdog: crate::watchdog::Watchdog,
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

/// Buffer window and byte cap for the current settings: the worst case at the
/// bitrate ceiling, plus room for one extra GOP.
fn buffer_limits(s: &Settings) -> (i64, u64) {
    let secs = s.replay.length_secs as u64;
    (secs as i64 * clock::SECOND, s.video.max_bitrate_mbps as u64 * 1_000_000 / 8 * (secs + 2))
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
            paused: false,
            games: GameTracker::default(),
            cs2: None,
            events_tx,
            events_rx,
            burst: None,
            watchdog: crate::watchdog::Watchdog::default(),
        };
        crate::watchdog::clear_health();
        r.video = Some(VideoPipeline::start(r.settings.video_settings(), r.shared.clone())?);
        r.start_audio();
        r.register_hotkey();
        r.start_integrations();
        Ok(r)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// False if another app holds the configured hotkey.
    pub fn hotkey_registered(&self) -> bool {
        self.hotkey_registered
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
        let frames = self.shared.frames.load(Relaxed);
        let late = self.shared.late_frames.load(Relaxed) + self.shared.skipped_ticks.load(Relaxed);
        if self.paused {
            self.watchdog.reset(frames, late);
            return;
        }
        if self.watchdog.check(Instant::now(), frames, late, self.video.is_some()) == crate::watchdog::Verdict::RestartVideo {
            // A new encoder's packets can't continue the old ones in a clip.
            self.flush_burst();
            self.video = None;
            self.shared.buffer.lock().unwrap().clear();
            match VideoPipeline::start(self.settings.video_settings(), self.shared.clone()) {
                Ok(v) => self.video = Some(v),
                Err(e) => warning!("watchdog: the video didn't restart ({e:#}); trying again later"),
            }
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
        } else {
            match VideoPipeline::start(self.settings.video_settings(), self.shared.clone()) {
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
        let hotkey_changed = old.replay.hotkey != self.settings.replay.hotkey;
        let video_changed = old.video != self.settings.video;
        // Also when moving the save sound changes whether a track leaves it out.
        let audio_changed = old.audio != self.settings.audio || old.audio_sources() != self.settings.audio_sources();
        let integrations_changed = old.auto_clip.enabled != self.settings.auto_clip.enabled
            || old.auto_clip.cs2.enabled != self.settings.auto_clip.cs2.enabled
            || old.auto_clip.cs2.port != self.settings.auto_clip.cs2.port
            || old.auto_clip.cs2.debug_log != self.settings.auto_clip.cs2.debug_log;

        if limits_changed {
            let (window, max_bytes) = buffer_limits(&self.settings);
            self.shared.buffer.lock().unwrap().set_limits(window, max_bytes);
        }
        if hotkey_changed {
            self.unregister_hotkey();
            self.register_hotkey();
        }
        self.cue.handle().update(&self.settings.cue);
        self.autoclip.set_rules(self.settings.autoclip_rules());
        if integrations_changed {
            self.cs2 = None;
            self.start_integrations();
        }
        // While paused nothing is running; resuming uses the new settings.
        if (video_changed || audio_changed) && !self.paused {
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
                match VideoPipeline::start(self.settings.video_settings(), self.shared.clone()) {
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
        while let Ok(e) = self.events_rx.try_recv() {
            let detail = if e.detail.is_empty() { String::new() } else { format!(" ({})", e.detail) };
            info!("cs2: {}{detail}", e.kind.label());
            // Always recorded (for markers in manual clips); clips only when enabled.
            self.autoclip.on_event(cs2::GAME_NAME, e);
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
            let label = Some(reason.label());
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
            reason.map_or(String::new(), |r| format!(" ({})", r.label())),
            clip.data.len() as f64 / 1e6,
        );
        // The map, if CS2 reported one during this clip (or just before it).
        let start = clip.start_time;
        let context = (game == cs2::GAME_NAME)
            .then(|| self.cs2.as_ref().map(|c| c.context.lock().unwrap().clone()))
            .flatten()
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
        let a = &self.settings.auto_clip;
        if !(a.enabled && a.cs2.enabled) {
            return;
        }
        match Cs2Integration::start(a.cs2.port, a.cs2.debug_log, self.events_tx.clone()) {
            Ok(i) => self.cs2 = i,
            Err(e) => warning!("cs2: integration failed to start: {e:#}"),
        }
    }

    fn register_hotkey(&mut self) {
        let text = &self.settings.replay.hotkey;
        let Ok(h) = Hotkey::parse(text) else { return };
        if unsafe { RegisterHotKey(None, HOTKEY_SAVE, h.modifiers | MOD_NOREPEAT, h.vk) }.is_ok() {
            self.hotkey_registered = true;
        } else {
            warning!("{text} is already taken by another app (SteelSeries Moments or NVIDIA Instant Replay?)");
        }
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
        self.video = None;
        self.audio = None;
        // Finishes any queued clips (and their sounds) before the cue player stops.
        self.saver = None;
    }
}
