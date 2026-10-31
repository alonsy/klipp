//! State shared between the video thread, the main thread and the clip saver.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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
    /// Pictures the capture delivered (each new picture of the screen counts once).
    pub captured: AtomicU64,
    /// How long the video loop's steps take, for the stats line.
    pub steps: Steps,
    /// Graphics memory Klipp uses, and what Windows allows it, in MB.
    pub vram_used_mb: AtomicU64,
    pub vram_budget_mb: AtomicU64,
    /// Performance mode's asks of the running video (see `performance`): a
    /// new picture every this many frames (1 = every frame)...
    pub perf_stride: AtomicU32,
    /// ...and this bitrate ceiling in bits/s (0 = the setting's).
    pub perf_max_bitrate: AtomicU32,
    /// The running encoder takes a new ceiling while recording (not AMF).
    pub bitrate_adjustable: AtomicBool,
}

/// Time spent in the video steps since the last stats line: waiting for the
/// captured frame, converting it, encoding (handing the frame over, then
/// collecting what's finished), and how far the encoder fell behind. Averages
/// show a busy GPU; maxima show stalls.
#[derive(Default)]
pub struct Steps {
    conversions: AtomicU64,
    wait_us: AtomicU64,
    wait_max_us: AtomicU64,
    convert_us: AtomicU64,
    convert_max_us: AtomicU64,
    encodes: AtomicU64,
    send_us: AtomicU64,
    send_max_us: AtomicU64,
    receive_us: AtomicU64,
    receive_max_us: AtomicU64,
    encode_us: AtomicU64,
    encode_max_us: AtomicU64,
    /// Frames waiting for the encoder now, and the most since the last stats line.
    queue: AtomicU64,
    queue_max: AtomicU64,
    /// Frames left out because the encoder's queue was full.
    dropped: AtomicU64,
}

/// Averages and maxima in milliseconds: waiting for the frame, converting,
/// encoding (and its two halves); the encoder's longest queue; frames dropped.
pub struct StepTimes {
    pub wait: (f64, f64),
    pub convert: (f64, f64),
    pub encode: (f64, f64),
    pub send: (f64, f64),
    pub receive: (f64, f64),
    pub queue_max: u64,
    pub dropped: u64,
}

impl Steps {
    pub fn converted(&self, wait: Duration, convert: Duration) {
        self.conversions.fetch_add(1, Ordering::Relaxed);
        add(&self.wait_us, &self.wait_max_us, wait);
        add(&self.convert_us, &self.convert_max_us, convert);
    }

    pub fn encoded(&self, send: Duration, receive: Duration) {
        self.encodes.fetch_add(1, Ordering::Relaxed);
        add(&self.send_us, &self.send_max_us, send);
        add(&self.receive_us, &self.receive_max_us, receive);
        add(&self.encode_us, &self.encode_max_us, send + receive);
    }

    /// A frame joined the encoder's queue.
    pub fn queued(&self) {
        let now = self.queue.fetch_add(1, Ordering::Relaxed) + 1;
        self.queue_max.fetch_max(now, Ordering::Relaxed);
    }

    /// The encoder took a frame from its queue.
    pub fn dequeued(&self) {
        self.queue.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn dropped(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
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
            send: avg_max(encodes, &self.send_us, &self.send_max_us),
            receive: avg_max(encodes, &self.receive_us, &self.receive_max_us),
            queue_max: self.queue_max.swap(self.queue.load(Ordering::Relaxed), Ordering::Relaxed),
            dropped: self.dropped.swap(0, Ordering::Relaxed),
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
            captured: AtomicU64::new(0),
            steps: Steps::default(),
            vram_used_mb: AtomicU64::new(0),
            vram_budget_mb: AtomicU64::new(0),
            perf_stride: AtomicU32::new(1),
            perf_max_bitrate: AtomicU32::new(0),
            bitrate_adjustable: AtomicBool::new(true),
        }
    }
}
