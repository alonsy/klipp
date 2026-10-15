//! AAC audio encoding.

use std::ptr;

use anyhow::{Result, bail};
use ffmpeg_sys_next as ff;

use crate::encoder::Packet;
use crate::error::{AVERROR_EAGAIN, check};
use crate::stream::StreamInfo;

/// Every audio track uses this rate; capture is converted to it.
pub const SAMPLE_RATE: u32 = 48_000;

/// Stereo AAC-LC at 48 kHz, fed planar f32 samples.
pub struct AudioEncoder {
    ctx: *mut ff::AVCodecContext,
    frame: *mut ff::AVFrame,
    pkt: *mut ff::AVPacket,
    frame_size: usize,
}

// SAFETY: used from one thread at a time; FFmpeg codec contexts have no thread affinity.
unsafe impl Send for AudioEncoder {}

impl AudioEncoder {
    pub fn new(bitrate: u32) -> Result<Self> {
        unsafe {
            let codec = ff::avcodec_find_encoder_by_name(c"aac".as_ptr());
            if codec.is_null() {
                bail!("the AAC encoder is not available in this FFmpeg build");
            }
            // Built first so Drop cleans up whatever was allocated if a later step fails.
            let mut enc = Self {
                ctx: ff::avcodec_alloc_context3(codec),
                frame: ff::av_frame_alloc(),
                pkt: ff::av_packet_alloc(),
                frame_size: 0,
            };
            if enc.ctx.is_null() || enc.frame.is_null() || enc.pkt.is_null() {
                bail!("out of memory while creating the audio encoder");
            }
            let c = &mut *enc.ctx;
            c.sample_fmt = ff::AVSampleFormat::AV_SAMPLE_FMT_FLTP;
            c.sample_rate = SAMPLE_RATE as i32;
            ff::av_channel_layout_default(&mut c.ch_layout, 2);
            c.bit_rate = bitrate as i64;
            c.time_base = ff::AVRational { num: 1, den: SAMPLE_RATE as i32 };
            c.flags |= ff::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            check(ff::avcodec_open2(enc.ctx, codec, ptr::null_mut()), "avcodec_open2(aac)")?;
            enc.frame_size = c.frame_size as usize;

            // One reusable input frame. The encoder releases it after each drain,
            // so refilling it never allocates.
            let f = &mut *enc.frame;
            f.nb_samples = c.frame_size;
            f.format = ff::AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
            f.sample_rate = c.sample_rate;
            check(ff::av_channel_layout_copy(&mut f.ch_layout, &c.ch_layout), "av_channel_layout_copy")?;
            check(ff::av_frame_get_buffer(enc.frame, 0), "av_frame_get_buffer")?;
            Ok(enc)
        }
    }

    /// Samples per channel that every `encode` call must provide (1024 for AAC).
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// Encodes one frame whose first sample is sample number `pts` on the track's timeline.
    pub fn encode(&mut self, left: &[f32], right: &[f32], pts: i64, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
        assert!(left.len() == self.frame_size && right.len() == self.frame_size, "wrong audio frame size");
        unsafe {
            check(ff::av_frame_make_writable(self.frame), "av_frame_make_writable")?;
            let f = &mut *self.frame;
            std::slice::from_raw_parts_mut(f.data[0] as *mut f32, self.frame_size).copy_from_slice(left);
            std::slice::from_raw_parts_mut(f.data[1] as *mut f32, self.frame_size).copy_from_slice(right);
            f.pts = pts;
            check(ff::avcodec_send_frame(self.ctx, self.frame), "avcodec_send_frame(aac)")?;
        }
        self.drain(sink)
    }

    /// Flushes the encoder. Call once after the last frame.
    pub fn finish(&mut self, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
        unsafe { check(ff::avcodec_send_frame(self.ctx, ptr::null()), "avcodec_send_frame(aac flush)")? };
        self.drain(sink)
    }

    /// What a muxer needs to write this encoder's output.
    pub fn stream_info(&self) -> Result<StreamInfo> {
        StreamInfo::from_codec(self.ctx)
    }

    fn drain(&mut self, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
        loop {
            let ret = unsafe { ff::avcodec_receive_packet(self.ctx, self.pkt) };
            if ret == AVERROR_EAGAIN || ret == ff::AVERROR_EOF {
                return Ok(());
            }
            check(ret, "avcodec_receive_packet(aac)")?;
            let result = sink(&mut Packet::wrap(self.pkt));
            unsafe { ff::av_packet_unref(self.pkt) };
            result?;
        }
    }
}

impl Drop for AudioEncoder {
    fn drop(&mut self) {
        unsafe {
            ff::avcodec_free_context(&mut self.ctx);
            ff::av_frame_free(&mut self.frame);
            ff::av_packet_free(&mut self.pkt);
        }
    }
}
