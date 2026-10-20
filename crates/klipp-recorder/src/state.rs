//! State shared between the video thread, the main thread and the clip saver.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use klipp_media::StreamInfo;

use crate::replay::ReplayBuffer;

pub struct Shared {
    pub buffer: Mutex<ReplayBuffer>,
    /// Set once the encoder is running. Needed to write clips.
    pub video_stream: Mutex<Option<StreamInfo>>,
    /// Audio tracks in buffer-stream order: the premix first, then one per source.
    pub audio_streams: Mutex<Vec<StreamInfo>>,
    pub frames: AtomicU64,
    /// Frames with new screen content (the rest repeat the previous frame).
    pub new_frames: AtomicU64,
    /// Ticks that started more than one frame late.
    pub late_frames: AtomicU64,
    /// Ticks skipped after a long stall (e.g. the PC was asleep).
    pub skipped_ticks: AtomicU64,
    /// How often the capture size changed (display mode switches).
    pub capture_resizes: AtomicU64,
    /// How often the capture was started again (see `revive`).
    pub capture_restarts: AtomicU64,
    /// Restarting the capture keeps not bringing the picture back.
    pub capture_blind: AtomicBool,
    /// How long the video loop's steps take, for the stats line (FIX-17).
    pub steps: Steps,
    /// Graphics memory Klipp uses, and what Windows allows it, in MB.
    pub vram_used_mb: AtomicU64,
    pub vram_budget_mb: AtomicU64,
}

/// Time spent in the video loop's steps since the last stats line: waiting
/// for the captured frame, converting it, encoding. Averages show a busy
/// GPU; maxima show stalls.
#[derive(Default)]
pub struct Steps {
    conversions: AtomicU64,
    wait_us: AtomicU64,
    wait_max_us: AtomicU64,
    convert_us: AtomicU64,
    convert_max_us: AtomicU64,
    encodes: AtomicU64,
    encode_us: AtomicU64,
    encode_max_us: AtomicU64,
}

/// Averages and maxima in milliseconds: waiting for the frame, converting, encoding.
pub struct StepTimes {
    pub wait: (f64, f64),
    pub convert: (f64, f64),
    pub encode: (f64, f64),
}

impl Steps {
    pub fn converted(&self, wait: Duration, convert: Duration) {
        self.conversions.fetch_add(1, Ordering::Relaxed);
        add(&self.wait_us, &self.wait_max_us, wait);
        add(&self.convert_us, &self.convert_max_us, convert);
    }

    pub fn encoded(&self, encode: Duration) {
        self.encodes.fetch_add(1, Ordering::Relaxed);
        add(&self.encode_us, &self.encode_max_us, encode);
    }

    /// What was measured since the last call; starts over.
    pub fn take(&self) -> StepTimes {
        let avg_max = |n: u64, sum: &AtomicU64, max: &AtomicU64| {
            let (sum, max) = (sum.swap(0, Ordering::Relaxed), max.swap(0, Ordering::Relaxed));
            (if n == 0 { 0.0 } else { sum as f64 / n as f64 / 1000.0 }, max as f64 / 1000.0)
        };
        let conversions = self.conversions.swap(0, Ordering::Relaxed);
        let encodes = self.encodes.swap(0, Ordering::Relaxed);
        StepTimes {
            wait: avg_max(conversions, &self.wait_us, &self.wait_max_us),
            convert: avg_max(conversions, &self.convert_us, &self.convert_max_us),
            encode: avg_max(encodes, &self.encode_us, &self.encode_max_us),
        }
    }
}

fn add(sum: &AtomicU64, max: &AtomicU64, d: Duration) {
    let us = d.as_micros() as u64;
    sum.fetch_add(us, Ordering::Relaxed);
    max.fetch_max(us, Ordering::Relaxed);
}

impl Shared {
    pub fn new(buffer: ReplayBuffer) -> Self {
        Self {
            buffer: Mutex::new(buffer),
            video_stream: Mutex::new(None),
            audio_streams: Mutex::new(Vec::new()),
            frames: AtomicU64::new(0),
            new_frames: AtomicU64::new(0),
            late_frames: AtomicU64::new(0),
            skipped_ticks: AtomicU64::new(0),
            capture_resizes: AtomicU64::new(0),
            capture_restarts: AtomicU64::new(0),
            capture_blind: AtomicBool::new(false),
            steps: Steps::default(),
            vram_used_mb: AtomicU64::new(0),
            vram_budget_mb: AtomicU64::new(0),
        }
    }
}
