//! Limits for what saves clips by itself. A model that misreads a screen or
//! mishears a voice must not be able to fill a drive: whatever it thinks it
//! saw, only so much of it can become clips.

use std::collections::VecDeque;

/// At most this many within this time (shared clock).
pub type Limit = (usize, i64);

pub struct Budget {
    limits: &'static [Limit],
    /// Newest last.
    taken: VecDeque<i64>,
}

impl Budget {
    pub fn new(limits: &'static [Limit]) -> Self {
        Self { limits, taken: VecDeque::new() }
    }

    /// Takes one at `now`, or false if that would break a limit. A refused
    /// one isn't counted, so it can't push out what's allowed later.
    pub fn take(&mut self, now: i64) -> bool {
        let longest = self.limits.iter().map(|l| l.1).max().unwrap_or(0);
        while self.taken.front().is_some_and(|&t| now - t >= longest) {
            self.taken.pop_front();
        }
        if self.limits.iter().any(|&(count, within)| self.taken.iter().filter(|&&t| now - t < within).count() >= count) {
            return false;
        }
        self.taken.push_back(now);
        true
    }

    pub fn clear(&mut self) {
        self.taken.clear();
    }
}
