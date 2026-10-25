//! The recorder watching its own video: if frames stop coming (a monitor
//! unplugged, the main display changed, a graphics driver reset), the video
//! restarts; if it can't keep up (frames late), the Klipp window is told so
//! it can suggest a lower resolution; if the capture can't see the screen
//! (see `revive`), the window says that. Through `health.json` next to the
//! log, which the window reads. Costs nothing but a check every couple of seconds.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::log::{info, warning};

/// No frames for this long means the video has stopped.
const STALLED_AFTER: Duration = Duration::from_secs(6);
/// At most one restart this often, so a broken setup doesn't spin.
const RESTART_EVERY: Duration = Duration::from_secs(30);
/// "Can't keep up" is judged over this long...
const SLOW_WINDOW: Duration = Duration::from_secs(30);
/// ...when more than one frame in ten was late...
const SLOW_SHARE: u64 = 10;
/// ...in this many windows in a row, so a one-off stall (an encoder warming
/// up, a game loading) doesn't count.
const SLOW_WINDOWS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// No frames; the video is being restarted.
    Stopped,
    /// The capture shows a still picture and starting it again doesn't help.
    Blind,
    /// Frames come late: the PC can't record this resolution and frame rate.
    Slow,
}

impl Problem {
    fn key(self) -> &'static str {
        match self {
            Problem::Stopped => "stopped",
            Problem::Blind => "blind",
            Problem::Slow => "slow",
        }
    }
}

/// The video thread's counters, as they are now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    /// Frames encoded so far.
    pub frames: u64,
    /// Ticks that started late.
    pub late: u64,
    /// Ticks skipped after a stall (never encoded).
    pub skipped: u64,
    /// The capture can't see the screen.
    pub blind: bool,
}

/// What a check concluded.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Fine,
    RestartVideo,
}

pub struct Watchdog {
    last_frames: u64,
    stalled_since: Option<Instant>,
    last_restart: Option<Instant>,
    window_start: Instant,
    window: Counts,
    /// Slow windows in a row.
    slow_windows: u32,
    /// The last window had more than half of its frames late or skipped.
    severe: bool,
    problem: Option<Problem>,
    /// Recording at a lower frame rate than set: (now, set). For the window.
    reduced: Option<(u32, u32)>,
    learned: bool,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self {
            last_frames: 0,
            stalled_since: None,
            last_restart: None,
            window_start: Instant::now(),
            window: Counts::default(),
            slow_windows: 0,
            severe: false,
            problem: None,
            reduced: None,
            learned: false,
        }
    }
}

impl Watchdog {
    /// `running`: a video pipeline exists. Call every couple of seconds.
    pub fn check(&mut self, now: Instant, counts: Counts, running: bool) -> Verdict {
        let frames = counts.frames;
        let mut verdict = Verdict::Fine;
        if !running || frames == self.last_frames {
            let since = *self.stalled_since.get_or_insert(now);
            let may_restart = self.last_restart.is_none_or(|t| now.duration_since(t) >= RESTART_EVERY);
            if now.duration_since(since) >= STALLED_AFTER && may_restart {
                warning!("watchdog: no video frames for {} s; restarting the video", now.duration_since(since).as_secs());
                self.last_restart = Some(now);
                self.stalled_since = None;
                self.set(Some(Problem::Stopped));
                verdict = Verdict::RestartVideo;
                // The new video is judged on its own: a stall isn't "can't keep up".
                (self.window_start, self.window, self.slow_windows, self.severe) = (now, counts, 0, false);
            }
        } else {
            self.stalled_since = None;
            if self.problem == Some(Problem::Stopped) {
                info!("watchdog: video frames are flowing again");
                self.set(None);
            }
        }
        self.last_frames = frames;

        if now.duration_since(self.window_start) >= SLOW_WINDOW {
            let w = self.window;
            let made = frames.saturating_sub(w.frames);
            let skipped = counts.skipped.saturating_sub(w.skipped);
            let behind = counts.late.saturating_sub(w.late) + skipped;
            // Every tick that was due: the encoded ones and the skipped ones.
            let due = made + skipped;
            if due > 0 && behind * SLOW_SHARE > due {
                self.slow_windows += 1;
                if self.slow_windows == SLOW_WINDOWS {
                    warning!("watchdog: {behind} of {due} frames were late or skipped, again; this PC can't keep up with these video settings");
                }
            } else {
                self.slow_windows = 0;
            }
            self.severe = due > 0 && behind * 2 > due;
            (self.window_start, self.window) = (now, counts);
        }

        // Video restarting first, then a still picture, then slowness.
        let problem = if self.problem == Some(Problem::Stopped) {
            Some(Problem::Stopped)
        } else if counts.blind {
            Some(Problem::Blind)
        } else if self.slow_windows >= SLOW_WINDOWS {
            Some(Problem::Slow)
        } else {
            None
        };
        self.set(problem);
        verdict
    }

    /// Paused, or new video settings: start over.
    pub fn reset(&mut self, counts: Counts) {
        let now = Instant::now();
        (self.last_frames, self.stalled_since) = (counts.frames, None);
        (self.window_start, self.window, self.slow_windows, self.severe) = (now, counts, 0, false);
        self.set(None);
    }

    /// Too slow to go on like this: two slow windows in a row, or one where
    /// more than half the frames were late or skipped (clips play like a
    /// slideshow; no startup hiccup lasts that long).
    pub fn overloaded(&self) -> bool {
        self.slow_windows >= SLOW_WINDOWS || self.severe
    }

    /// Recording at `now` fps instead of the `set` rate, for the window to
    /// say; `learned` when earlier sessions made it the rate to start at (`tuning`).
    pub fn set_reduced(&mut self, reduced: Option<(u32, u32)>, learned: bool) {
        if reduced != self.reduced || learned != self.learned {
            self.reduced = reduced;
            self.learned = learned;
            write_health(self.problem, reduced, learned);
        }
    }

    fn set(&mut self, problem: Option<Problem>) {
        if problem != self.problem {
            self.problem = problem;
            write_health(problem, self.reduced, self.learned);
        }
    }
}

fn health_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Klipp").join("health.json"))
}

fn write_health(problem: Option<Problem>, reduced: Option<(u32, u32)>, learned: bool) {
    // Tests don't touch the real Klipp folder.
    if cfg!(test) {
        return;
    }
    let Some(path) = health_path() else { return };
    let since = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let reduced = reduced.map(|(to, from)| json!({ "to": to, "from": from, "learned": learned }));
    let body = json!({ "problem": problem.map(Problem::key), "since": since, "reduced": reduced });
    let part = path.with_extension("json.part");
    if std::fs::write(&part, body.to_string()).is_ok() {
        let _ = std::fs::rename(&part, &path);
    }
}

/// A recorder starting up has no problems yet.
pub fn clear_health() {
    write_health(None, None, false);
}
