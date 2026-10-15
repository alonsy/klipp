//! The recorder watching its own video: if frames stop coming (a monitor
//! unplugged, the main display changed, a graphics driver reset), the video
//! restarts; if it can't keep up (frames late), the Klipp window is told so
//! it can suggest a lower resolution. Through `health.json` next to the log,
//! which the window reads. Costs nothing but a check every couple of seconds.

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
/// ...when more than one frame in ten was late.
const SLOW_SHARE: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// No frames; the video is being restarted.
    Stopped,
    /// Frames come late: the PC can't record this resolution and frame rate.
    Slow,
}

impl Problem {
    fn key(self) -> &'static str {
        match self {
            Problem::Stopped => "stopped",
            Problem::Slow => "slow",
        }
    }
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
    window_frames: u64,
    window_late: u64,
    problem: Option<Problem>,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self {
            last_frames: 0,
            stalled_since: None,
            last_restart: None,
            window_start: Instant::now(),
            window_frames: 0,
            window_late: 0,
            problem: None,
        }
    }
}

impl Watchdog {
    /// `frames`: frames encoded so far; `late`: late plus skipped so far;
    /// `running`: a video pipeline exists. Call every couple of seconds.
    pub fn check(&mut self, now: Instant, frames: u64, late: u64, running: bool) -> Verdict {
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
            let (made, late_now) = (frames.saturating_sub(self.window_frames), late.saturating_sub(self.window_late));
            if made > 0 && late_now * SLOW_SHARE > made {
                if self.problem != Some(Problem::Slow) {
                    warning!("watchdog: {late_now} of {made} frames were late; this PC can't keep up with these video settings");
                }
                self.set(Some(Problem::Slow));
            } else if self.problem == Some(Problem::Slow) {
                self.set(None);
            }
            (self.window_start, self.window_frames, self.window_late) = (now, frames, late);
        }
        verdict
    }

    /// Paused, or new video settings: start over.
    pub fn reset(&mut self, frames: u64, late: u64) {
        let now = Instant::now();
        (self.last_frames, self.stalled_since) = (frames, None);
        (self.window_start, self.window_frames, self.window_late) = (now, frames, late);
        self.set(None);
    }

    fn set(&mut self, problem: Option<Problem>) {
        if problem != self.problem {
            self.problem = problem;
            write_health(problem);
        }
    }
}

fn health_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Klipp").join("health.json"))
}

fn write_health(problem: Option<Problem>) {
    // Tests don't touch the real Klipp folder.
    if cfg!(test) {
        return;
    }
    let Some(path) = health_path() else { return };
    let since = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let body = json!({ "problem": problem.map(Problem::key), "since": since });
    let part = path.with_extension("json.part");
    if std::fs::write(&part, body.to_string()).is_ok() {
        let _ = std::fs::rename(&part, &path);
    }
}

/// A recorder starting up has no problems yet.
pub fn clear_health() {
    write_health(None);
}
