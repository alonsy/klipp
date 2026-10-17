//! Decides when the screen capture has stopped for good and needs starting
//! again. Windows ends a capture when its monitor goes away (asleep, switched
//! off, unplugged), and sometimes a capture just stops sending pictures after
//! the screen comes back, without saying so. Either way the video carries on
//! repeating the last picture, so clips would show a still screen.
//!
//! Two signs: Windows says the capture closed, or the picture hasn't changed
//! for a while although someone is using the PC (mouse or keyboard input since
//! the last picture; moving the mouse alone changes the picture, since the
//! cursor is captured). Only the capture restarts; the encoder and the replay
//! buffer carry on, so a needless restart costs nothing.

use std::time::{Duration, Instant};

/// How long the picture may stand still while the PC is in use.
const STILL_AFTER: Duration = Duration::from_secs(5);
/// Input this soon after the last picture may simply not have changed it yet.
const INPUT_MARGIN: Duration = Duration::from_secs(1);
/// Waits between restarts: 2 s, doubling while restarts don't bring the picture back.
const FIRST_WAIT: Duration = Duration::from_secs(2);
const LONGEST_WAIT: Duration = Duration::from_secs(60);
/// Restarts in a row that didn't help, after which Klipp says it can't see the screen.
const BLIND_AFTER: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Windows closed the capture (its monitor went away).
    Closed,
    /// The picture stood still while someone used the PC.
    Still,
}

pub struct Reviver {
    last_picture: Instant,
    /// The last restart, and how many pictures came after it.
    restart: Option<(Instant, u32)>,
    /// Restarts in a row after which the picture didn't come back.
    useless: u32,
}

impl Reviver {
    pub fn new(now: Instant) -> Self {
        Self { last_picture: now, restart: None, useless: 0 }
    }

    /// A new picture arrived. True when it shows a restart worked: a second
    /// picture after it (any capture sends one picture right when it starts).
    pub fn picture(&mut self, now: Instant) -> bool {
        self.last_picture = now;
        let Some((_, count)) = &mut self.restart else { return false };
        *count += 1;
        if *count == 2 {
            self.useless = 0;
            return true;
        }
        false
    }

    /// Whether to restart the capture now. `closed`: Windows closed it.
    /// `input_age`: how long ago the last mouse or keyboard input was.
    pub fn check(&mut self, now: Instant, closed: bool, input_age: Option<Duration>) -> Option<Why> {
        if self.restart.is_some_and(|(at, _)| now.duration_since(at) < self.wait()) {
            return None;
        }
        let still = now.duration_since(self.last_picture);
        let used = input_age.is_some_and(|age| age + INPUT_MARGIN < still);
        let why = if closed {
            Why::Closed
        } else if still >= STILL_AFTER && used {
            Why::Still
        } else {
            return None;
        };
        if self.restart.is_some_and(|(_, count)| count < 2) {
            self.useless += 1;
        }
        self.restart = Some((now, 0));
        Some(why)
    }

    /// The first restart of a run (later ones only matter to the log once it's blind).
    pub fn first_try(&self) -> bool {
        self.useless == 0
    }

    /// Restarts keep not helping: the screen can't be seen.
    pub fn blind(&self) -> bool {
        self.useless >= BLIND_AFTER
    }

    fn wait(&self) -> Duration {
        (FIRST_WAIT * 2u32.pow(self.useless.min(5))).min(LONGEST_WAIT)
    }
}
