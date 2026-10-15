//! Places one audio source's samples on the shared sample timeline by timestamp.
//!
//! Loopback devices send nothing while nothing plays, and device clocks drift a
//! little against each other. Positioning every packet by its own timestamp
//! turns gaps into silence and keeps all tracks in sync with the video.

/// A packet that starts within this many frames of where the previous one
/// ended is treated as continuous, so timestamp jitter doesn't cause clicks.
const JITTER_FRAMES: i64 = 96; // 2 ms at 48 kHz

pub struct Timeline {
    left: Vec<f32>,
    right: Vec<f32>,
    /// Where the previous packet ended.
    next: Option<i64>,
}

impl Timeline {
    pub fn new(capacity_frames: usize) -> Self {
        Self { left: vec![0.0; capacity_frames], right: vec![0.0; capacity_frames], next: None }
    }

    /// Writes interleaved stereo `samples` (or silence if `None`) starting at
    /// frame `pos`. Frames before `mixed` (already taken by the mixer) or
    /// beyond the capacity are dropped.
    pub fn write(&mut self, pos: i64, samples: Option<&[f32]>, frames: usize, mixed: i64) {
        let pos = match self.next {
            Some(next) if (pos - next).abs() <= JITTER_FRAMES => next,
            _ => pos,
        };
        let cap = self.left.len() as i64;
        for i in 0..frames {
            let p = pos + i as i64;
            if p < mixed || p >= mixed + cap {
                continue;
            }
            let idx = p.rem_euclid(cap) as usize;
            let (l, r) = samples.map_or((0.0, 0.0), |s| (s[2 * i], s[2 * i + 1]));
            self.left[idx] = l;
            self.right[idx] = r;
        }
        self.next = Some(pos + frames as i64);
    }

    /// Copies frames `[start, start + left.len())` out and clears them, so a
    /// source that goes quiet reads as silence rather than stale audio.
    pub fn take(&mut self, start: i64, left: &mut [f32], right: &mut [f32]) {
        let cap = self.left.len() as i64;
        for i in 0..left.len() {
            let idx = (start + i as i64).rem_euclid(cap) as usize;
            left[i] = std::mem::take(&mut self.left[idx]);
            right[i] = std::mem::take(&mut self.right[idx]);
        }
    }
}
