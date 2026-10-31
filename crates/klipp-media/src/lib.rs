//! FFmpeg wrappers for Klipp.
//!
//! Every `unsafe` call into FFmpeg lives in this crate. The rest of the project
//! only sees owned types that free their FFmpeg objects when dropped.

mod audio;
mod encoder;
mod error;
mod hw;
mod mux;
mod stream;

pub use audio::{AudioEncoder, SAMPLE_RATE};
pub use encoder::{FramePool, Packet, Vendor, VideoCodec, VideoEncoder, VideoEncoderConfig};
pub use hw::{HwDevice, HwFrame};
pub use mux::{Chapter, Mp4Writer};
pub use stream::StreamInfo;

/// Limits FFmpeg's own console logging to warnings and errors.
pub fn init_logging() {
    unsafe { ffmpeg_sys_next::av_log_set_level(ffmpeg_sys_next::AV_LOG_WARNING as i32) };
}
