//! The webcam's encoder (Content Creator Mode): H.264 on the
//! graphics card's own encoder, fed NV12 pictures from system memory. The
//! camera hands them over that way, and the encoder uploads them itself.

use std::ptr;

use anyhow::{Result, bail};
use ffmpeg_sys_next as ff;

use crate::encoder::{Packet, Vendor, amf_quality_level, set_opt};
use crate::error::{AVERROR_EAGAIN, check};
use crate::stream::StreamInfo;

/// Camera timestamps count 90 kHz ticks (MPEG's clock). A camera's frame rate
/// wanders (it slows down in the dark), so every picture keeps its own capture time.
pub const CAMERA_CLOCK: i64 = 90_000;

/// NVENC-scale constant quality for the camera: a face needs less than a game.
const QUALITY: u8 = 24;

pub struct CameraEncoder {
    ctx: *mut ff::AVCodecContext,
    frame: *mut ff::AVFrame,
    pkt: *mut ff::AVPacket,
    width: usize,
    height: usize,
}

// SAFETY: used from one thread at a time; FFmpeg codec contexts have no thread affinity.
unsafe impl Send for CameraEncoder {}

impl CameraEncoder {
    /// `fps` is the camera's nominal rate (for the encoder's rate control);
    /// keyframes come every half second, so a clip's camera starts within that.
    pub fn new(vendor: Vendor, width: u32, height: u32, fps: u32, max_bitrate: u32) -> Result<Self> {
        if width % 2 != 0 || height % 2 != 0 {
            bail!("the camera's picture is {width}x{height}; NV12 needs even sides");
        }
        let name = match vendor {
            Vendor::Nvidia => c"h264_nvenc",
            Vendor::Amd => c"h264_amf",
            Vendor::Intel => c"h264_qsv",
        };
        unsafe {
            let codec = ff::avcodec_find_encoder_by_name(name.as_ptr());
            if codec.is_null() {
                bail!("{name:?} is not available in this FFmpeg build");
            }
            // Built first so Drop cleans up whatever was allocated if a later step fails.
            let enc = Self {
                ctx: ff::avcodec_alloc_context3(codec),
                frame: ff::av_frame_alloc(),
                pkt: ff::av_packet_alloc(),
                width: width as usize,
                height: height as usize,
            };
            if enc.ctx.is_null() || enc.frame.is_null() || enc.pkt.is_null() {
                bail!("out of memory while creating the camera encoder");
            }
            let fps = fps.max(1) as i32;
            let c = &mut *enc.ctx;
            c.width = width as i32;
            c.height = height as i32;
            c.pix_fmt = ff::AVPixelFormat::AV_PIX_FMT_NV12;
            c.time_base = ff::AVRational { num: 1, den: CAMERA_CLOCK as i32 };
            c.framerate = ff::AVRational { num: fps, den: 1 };
            c.gop_size = (fps / 2).max(1);
            c.max_b_frames = 0;
            c.rc_max_rate = max_bitrate as i64;
            c.rc_buffer_size = max_bitrate.saturating_mul(2).min(i32::MAX as u32) as i32;
            c.bit_rate = match vendor {
                Vendor::Nvidia => 0,
                Vendor::Amd | Vendor::Intel => max_bitrate as i64 * 3 / 5,
            };
            c.color_range = ff::AVColorRange::AVCOL_RANGE_MPEG;
            c.color_primaries = ff::AVColorPrimaries::AVCOL_PRI_BT709;
            c.color_trc = ff::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
            c.colorspace = ff::AVColorSpace::AVCOL_SPC_BT709;
            c.flags |= ff::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            match vendor {
                Vendor::Nvidia => {
                    set_opt(c.priv_data, "preset", "p4")?;
                    set_opt(c.priv_data, "tune", "hq")?;
                    set_opt(c.priv_data, "rc", "vbr")?;
                    set_opt(c.priv_data, "cq", &QUALITY.to_string())?;
                }
                Vendor::Amd => {
                    set_opt(c.priv_data, "usage", "transcoding")?;
                    set_opt(c.priv_data, "quality", "balanced")?;
                    set_opt(c.priv_data, "rc", "qvbr")?;
                    set_opt(c.priv_data, "qvbr_quality_level", &amf_quality_level(QUALITY).to_string())?;
                    set_opt(c.priv_data, "bf", "0")?;
                }
                Vendor::Intel => set_opt(c.priv_data, "preset", "veryfast")?,
            }
            check(ff::avcodec_open2(enc.ctx, codec, ptr::null_mut()), "avcodec_open2(camera)")?;

            // One reusable picture, refilled for every frame.
            let f = &mut *enc.frame;
            f.format = ff::AVPixelFormat::AV_PIX_FMT_NV12 as i32;
            f.width = width as i32;
            f.height = height as i32;
            check(ff::av_frame_get_buffer(enc.frame, 0), "av_frame_get_buffer(camera)")?;
            Ok(enc)
        }
    }

    /// Encodes one NV12 picture (`y`: width × height bytes, `uv`: width ×
    /// height / 2, both without padding) as `pts` 90 kHz ticks.
    pub fn encode(&mut self, y: &[u8], uv: &[u8], pts: i64, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
        let (w, h) = (self.width, self.height);
        if y.len() < w * h || uv.len() < w * h / 2 {
            bail!("camera picture too small for {w}x{h}");
        }
        unsafe {
            check(ff::av_frame_make_writable(self.frame), "av_frame_make_writable(camera)")?;
            let f = &mut *self.frame;
            for (plane, src, rows) in [(0, y, h), (1, uv, h / 2)] {
                let stride = f.linesize[plane] as usize;
                let dst = std::slice::from_raw_parts_mut(f.data[plane], stride * rows);
                for row in 0..rows {
                    dst[row * stride..row * stride + w].copy_from_slice(&src[row * w..row * w + w]);
                }
            }
            f.pts = pts;
            check(ff::avcodec_send_frame(self.ctx, self.frame), "avcodec_send_frame(camera)")?;
        }
        self.drain(sink)
    }

    /// Flushes the encoder. Call once after the last picture.
    pub fn finish(&mut self, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
        unsafe { check(ff::avcodec_send_frame(self.ctx, ptr::null()), "avcodec_send_frame(camera flush)")? };
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
            check(ret, "avcodec_receive_packet(camera)")?;
            let result = sink(&mut Packet::wrap(self.pkt));
            unsafe { ff::av_packet_unref(self.pkt) };
            result?;
        }
    }
}

impl Drop for CameraEncoder {
    fn drop(&mut self) {
        unsafe {
            ff::avcodec_free_context(&mut self.ctx);
            ff::av_frame_free(&mut self.frame);
            ff::av_packet_free(&mut self.pkt);
        }
    }
}
