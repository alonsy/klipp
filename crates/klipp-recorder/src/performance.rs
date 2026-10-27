//! Performance mode (IDEA-42): while a game is in front and keeps the
//! graphics card busy, Klipp steps back, first its bitrate, then its frame
//! rate (half, then a third), and comes back a step at a time once the card
//! has room. The video keeps running through every step: the video loop takes
//! a new picture only every few frames, and the encoder lowers its ceiling
//! (see `video`), so nothing in the replay buffer is lost.

use std::time::{Duration, Instant};

/// A step: a new picture every `stride` frames, and this share of the bitrate ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub stride: u32,
    pub bitrate_percent: u32,
}

/// Full, then less bitrate, then half the frame rate, then a third.
pub const STEPS: [Step; 4] = [
    Step { stride: 1, bitrate_percent: 100 },
    Step { stride: 1, bitrate_percent: 70 },
    Step { stride: 2, bitrate_percent: 50 },
    Step { stride: 3, bitrate_percent: 35 },
];

/// The 3D engine at least this busy (percent), or Klipp's own frames falling
/// behind, counts as a busy card...
const BUSY: f64 = 90.0;
/// ...below this, with Klipp keeping up, as one with room.
const CALM: f64 = 75.0;
/// Samples (every 2 s) in a row before a step down...
const BUSY_SAMPLES: u32 = 2;
/// ...and before a step back up.
const CALM_SAMPLES: u32 = 5;
/// After any step, the next one up waits at least this long, so a card that
/// only has room because Klipp stepped back doesn't make it seesaw.
const HOLD: Duration = Duration::from_secs(20);
/// Clips never drop below this many pictures a second.
const MIN_FPS: u32 = 20;

/// One look at the PC, every couple of seconds.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// A game is in front.
    pub in_game: bool,
    /// The busiest 3D engine, percent.
    pub busy: f64,
    /// Klipp's own frames were late or left out since the last sample.
    pub behind: bool,
}

#[derive(Debug, Default)]
pub struct Governor {
    level: usize,
    busy: u32,
    calm: u32,
    changed: Option<Instant>,
}

impl Governor {
    pub fn level(&self) -> usize {
        self.level
    }

    /// Takes a sample; the new level when it changed.
    pub fn update(&mut self, now: Instant, s: Sample) -> Option<usize> {
        if !s.in_game {
            (self.busy, self.calm) = (0, 0);
            return self.set(now, 0);
        }
        if s.behind || s.busy >= BUSY {
            (self.busy, self.calm) = (self.busy + 1, 0);
        } else if s.busy < CALM {
            (self.busy, self.calm) = (0, self.calm + 1);
        } else {
            (self.busy, self.calm) = (0, 0);
        }
        if self.busy >= BUSY_SAMPLES && self.level + 1 < STEPS.len() {
            self.busy = 0;
            return self.set(now, self.level + 1);
        }
        let held = self.changed.is_some_and(|t| now.duration_since(t) < HOLD);
        if self.calm >= CALM_SAMPLES && self.level > 0 && !held {
            self.calm = 0;
            return self.set(now, self.level - 1);
        }
        None
    }

    /// Back to full at once (performance mode off, paused, new settings).
    pub fn reset(&mut self) -> Option<usize> {
        (self.busy, self.calm, self.changed) = (0, 0, None);
        std::mem::take(&mut self.level).ne(&0).then_some(0)
    }

    fn set(&mut self, now: Instant, level: usize) -> Option<usize> {
        if level == self.level {
            return None;
        }
        self.level = level;
        self.changed = Some(now);
        Some(level)
    }
}

/// What a level means for these settings: (new picture every N frames,
/// pictures a second, bitrate ceiling in bits/s). The frame rate stays at
/// `MIN_FPS` or more.
pub fn at(level: usize, fps: u32, max_bitrate: u32) -> (u32, u32, u32) {
    let step = STEPS[level.min(STEPS.len() - 1)];
    let stride = step.stride.min(fps / MIN_FPS).max(1);
    (stride, fps / stride, (max_bitrate as u64 * step.bitrate_percent as u64 / 100) as u32)
}
