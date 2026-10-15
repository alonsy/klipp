//! State shared between the video thread, the main thread and the clip saver.

use std::sync::Mutex;
use std::sync::atomic::AtomicU64;

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
        }
    }
}
