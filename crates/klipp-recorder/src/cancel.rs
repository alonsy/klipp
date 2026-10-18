//! Takes Klipp's own sounds back out of a track that records everything but
//! Discord. Windows leaves only one program (and what it starts) out of a
//! capture, so with Discord on its own track, the save sound would end up in
//! the Game track. A third capture hears only Klipp, and subtracting it
//! removes the sound exactly (55 to 60 dB down in tests), once the two line
//! up: Windows delivers them a little apart (70 to 90 samples in tests,
//! different for every pair of captures, steady while they run).
//!
//! The save sound is a chime that nearly repeats itself every 8.5 ms, so a
//! wrong offset can look almost right over a short stretch, more so under a
//! loud game. So the offset is measured twice: quickly when a sound starts,
//! to use right away, and again over the whole sound once it's over, which is
//! far more certain. That one is kept for every later sound.

use crate::timeline::Timeline;

/// How far apart the two captures may be, either way (10 ms).
const MAX_OFFSET: i64 = 480;
/// Audio used for the quick measurement when a sound starts (85 ms; the
/// mixer runs 100 ms behind, so this much of the track is already there).
const QUICK_FRAMES: usize = 4096;
/// Audio used for the careful measurement once a sound is over: the whole
/// sound, up to 1.5 s (the save sound is 1.25 s).
const CAREFUL_FRAMES: usize = 72_000;
/// The track and Klipp's sound kept for the careful measurement (2 s).
const HISTORY_FRAMES: usize = 96_000;
/// After this much silence from Klipp, a sound counts as over.
const OVER_AFTER: i64 = 4_800;
/// Quieter than this counts as silence.
const QUIET: f32 = 1e-5;
/// Quick measurements per sound, one per frame, before leaving it to the careful one.
const QUICK_TRIES: u8 = 4;
/// Subtracting at an offset must take out at least this share of Klipp's
/// sound, however loud the game is; otherwise nothing is subtracted, so a bad
/// measurement can't add a sound.
const MATCH_SHARE: f32 = 0.8;
/// The careful measurement is kept only if the best offset beats every other
/// one (more than a few samples away) by this share of Klipp's sound.
const CLEAR_MARGIN: f32 = 0.05;

/// Frames of two stereo signals, kept by their place on the shared timeline.
struct History {
    game: [Vec<f32>; 2],
    own: [Vec<f32>; 2],
}

impl History {
    fn index(pos: i64) -> usize {
        pos.rem_euclid(HISTORY_FRAMES as i64) as usize
    }
}

pub struct Canceller {
    /// Klipp's own sound, on the shared timeline, kept `MAX_OFFSET` frames
    /// behind the mixer so any offset can still reach it.
    own: Timeline,
    /// The offset for this pair of captures, measured over a whole sound:
    /// the track holds Klipp's frame `t + offset` at frame `t`.
    known: Option<i64>,
    /// The offset used for the sound playing now.
    current: Option<i64>,
    /// Where the sound playing now started, and where Klipp last made one.
    sound_start: Option<i64>,
    last_sound: i64,
    /// Quick measurements tried for the sound playing now.
    tries: u8,
    history: History,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Canceller {
    pub fn new(capacity_frames: usize) -> Self {
        let ring = || [vec![0.0; HISTORY_FRAMES], vec![0.0; HISTORY_FRAMES]];
        Self {
            own: Timeline::new(capacity_frames),
            known: None,
            current: None,
            sound_start: None,
            last_sound: i64::MIN / 2,
            tries: 0,
            history: History { game: ring(), own: ring() },
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    /// One packet of Klipp's own sound, as `Timeline::write` takes it.
    pub fn write(&mut self, pos: i64, samples: Option<&[f32]>, frames: usize, mixed: i64) {
        self.own.write(pos, samples, frames, mixed - MAX_OFFSET);
    }

    /// Subtracts Klipp's sound from the track's frames `[start, start + n)`,
    /// already taken out of `track` into `left` and `right`; `track` still
    /// holds what follows, for measuring.
    pub fn apply(&mut self, start: i64, left: &mut [f32], right: &mut [f32], track: &Timeline) {
        let n = left.len();
        let span = n + 2 * MAX_OFFSET as usize;
        self.left.resize(span, 0.0);
        self.right.resize(span, 0.0);
        self.own.peek(start - MAX_OFFSET, &mut self.left, &mut self.right);
        let sounding = self.left.iter().chain(&self.right).any(|s| s.abs() > QUIET);

        // Kept as they arrive, before subtracting, for the careful measurement.
        for i in 0..n {
            let k = History::index(start + i as i64);
            self.history.game[0][k] = left[i];
            self.history.game[1][k] = right[i];
            self.history.own[0][k] = self.left[MAX_OFFSET as usize + i];
            self.history.own[1][k] = self.right[MAX_OFFSET as usize + i];
        }

        if sounding {
            self.sound_start.get_or_insert(start);
            self.last_sound = start + n as i64;
            if self.current.is_none() {
                self.current = self.known;
            }
            if self.current.is_none() && self.tries < QUICK_TRIES {
                self.tries += 1;
                self.current = self.quick(start, left, right, track);
            }
            if let Some(d) = self.current {
                let base = (d + MAX_OFFSET) as usize;
                for i in 0..n {
                    left[i] -= self.left[base + i];
                    right[i] -= self.right[base + i];
                }
            }
        } else if start - self.last_sound > OVER_AFTER {
            if let Some(from) = self.sound_start.take() {
                if let Some(d) = self.careful(from, self.last_sound) {
                    self.known = Some(d);
                }
            }
            (self.current, self.tries) = (None, 0);
        }
        // Frames this far back can't be reached by any offset any more.
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        self.own.take(start - MAX_OFFSET, &mut l, &mut r);
    }

    /// When a sound starts: the offset over the next 85 ms of the track.
    fn quick(&self, start: i64, left: &[f32], right: &[f32], track: &Timeline) -> Option<i64> {
        let n = left.len();
        let m = QUICK_FRAMES.max(n);
        let (mut gl, mut gr) = (vec![0.0; m], vec![0.0; m]);
        gl[..n].copy_from_slice(left);
        gr[..n].copy_from_slice(right);
        track.peek(start + n as i64, &mut gl[n..], &mut gr[n..]);
        let w = m + 2 * MAX_OFFSET as usize;
        let (mut ol, mut or) = (vec![0.0; w], vec![0.0; w]);
        self.own.peek(start - MAX_OFFSET, &mut ol, &mut or);
        best_offset(&gl, &gr, &ol, &or, None)
    }

    /// Once a sound is over: the offset over (the start of) the whole sound,
    /// if it's clearly the one.
    fn careful(&self, from: i64, to: i64) -> Option<i64> {
        // A long run of sounds (a burst of saves): its end, which the history still holds.
        let from = from.max(to - CAREFUL_FRAMES as i64);
        let m = ((to - from) as usize).max(1);
        let w = m + 2 * MAX_OFFSET as usize;
        let read = |ring: &[Vec<f32>; 2], at: i64, len: usize| -> [Vec<f32>; 2] {
            let ch = |c: usize| (0..len).map(|i| ring[c][History::index(at + i as i64)]).collect();
            [ch(0), ch(1)]
        };
        let [gl, gr] = read(&self.history.game, from, m);
        let [ol, or] = read(&self.history.own, from - MAX_OFFSET, w);
        best_offset(&gl, &gr, &ol, &or, Some(QUICK_FRAMES))
    }
}

/// How far apart two offsets must be to count as different matches: the
/// save sound matches itself over about 20 samples either way.
const SAME_MATCH: i64 = 32;
/// The careful measurement checks this many of the best short-stretch offsets over the whole sound.
const CANDIDATES: usize = 6;
/// How many times the game's noise the winning offset must win by.
const NOISE_MARGIN: f32 = 4.0;

/// The offset at which `own` cancels out of `game` (`own` is
/// `2 * MAX_OFFSET` frames longer, starting `MAX_OFFSET` earlier), but only if
/// it's clearly the one. A wrong offset would add a second copy of the sound,
/// so when the game drowns the measurement, nothing is subtracted.
/// `short`: first find candidates over this many frames, then check only
/// those over all of it (much cheaper over a whole sound).
fn best_offset(gl: &[f32], gr: &[f32], ol: &[f32], or: &[f32], short: Option<usize>) -> Option<i64> {
    let m = gl.len();
    let all = || -MAX_OFFSET..=MAX_OFFSET;
    let gains = match short.filter(|&s| s < m) {
        None => gains(gl, gr, ol, or, all()),
        Some(s) => {
            let first = gains(&gl[..s], &gr[..s], &ol[..s + 2 * MAX_OFFSET as usize], &or[..s + 2 * MAX_OFFSET as usize], all());
            gains(gl, gr, ol, or, peaks(&first, CANDIDATES).into_iter())
        }
    };
    let energy = |x: &[f32]| x.iter().map(|s| s * s).sum::<f32>();
    let (game, own_total) = (energy(gl) + energy(gr), energy(ol) + energy(or));
    let &(best, removed, own) = gains.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    // Most of Klipp's sound must be in the stretch, and most of it must come out.
    if own < own_total * 0.5 || removed < own * MATCH_SHARE {
        return None;
    }
    // The best other match must lose by a share of the sound and by far more
    // than the game's noise could account for.
    let runner_up = gains.iter().filter(|g| (g.0 - best).abs() > SAME_MATCH).map(|g| g.1).fold(f32::MIN, f32::max);
    let noise = 2.0 * (((game - removed).max(0.0) / (2 * m) as f32) * own).sqrt();
    (removed - runner_up > (own * CLEAR_MARGIN).max(NOISE_MARGIN * noise)).then_some(best)
}

/// (offset, energy taken out by subtracting Klipp's sound there, Klipp's energy there)
/// for each offset in `lags`. Subtracting `o` from `g` takes out 2<g,o> - |o|².
fn gains(gl: &[f32], gr: &[f32], ol: &[f32], or: &[f32], lags: impl Iterator<Item = i64>) -> Vec<(i64, f32, f32)> {
    lags.map(|d| {
        let k = (d + MAX_OFFSET) as usize;
        let (mut dot, mut own) = (0.0f32, 0.0f32);
        for i in 0..gl.len() {
            dot += gl[i] * ol[k + i] + gr[i] * or[k + i];
            own += ol[k + i] * ol[k + i] + or[k + i] * or[k + i];
        }
        (d, 2.0 * dot - own, own)
    })
    .collect()
}

/// The `n` offsets with the most taken out, at least `SAME_MATCH` apart.
fn peaks(gains: &[(i64, f32, f32)], n: usize) -> Vec<i64> {
    let mut sorted: Vec<&(i64, f32, f32)> = gains.iter().collect();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut picked: Vec<i64> = Vec::new();
    for g in sorted {
        if picked.len() == n {
            break;
        }
        if picked.iter().all(|&p| (p - g.0).abs() > SAME_MATCH) {
            picked.push(g.0);
        }
    }
    picked
}
