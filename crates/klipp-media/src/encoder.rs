//! Hardware video encoding (NVENC) from D3D11 textures.

use std::ffi::{CStr, CString};
use std::ptr;

use anyhow::{Result, bail};
use ffmpeg_sys_next as ff;

use crate::error::{AVERROR_EAGAIN, check};
use crate::hw::{self, HwDevice, HwFrame};
use crate::stream::StreamInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec {
    Av1,
    H264,
}

impl VideoCodec {
    fn encoder_name(self) -> &'static CStr {
        match self {
            VideoCodec::Av1 => c"av1_nvenc",
            VideoCodec::H264 => c"h264_nvenc",
        }
    }
}

#[derive(Clone, Debug)]
pub struct VideoEncoderConfig {
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// NVENC constant-quality target; lower is better quality.
    pub quality: u8,
    /// Hard bitrate ceiling in bits/s. Keeps the replay buffer's size predictable.
    pub max_bitrate: u32,
}

/// An encoded packet. Only valid inside the sink callback it was passed to.
pub struct Packet {
    p: *mut ff::AVPacket,
}

impl Packet {
    pub(crate) fn wrap(p: *mut ff::AVPacket) -> Self {
        Self { p }
    }

    pub fn size(&self) -> usize {
        unsafe { (*self.p).size as usize }
    }

    pub fn is_key(&self) -> bool {
        unsafe { (*self.p).flags & ff::AV_PKT_FLAG_KEY as i32 != 0 }
    }

    pub fn data(&self) -> &[u8] {
        unsafe {
            if (*self.p).data.is_null() {
                return &[];
            }
            std::slice::from_raw_parts((*self.p).data, (*self.p).size as usize)
        }
    }

    /// Presentation timestamp, in the encoder's time base.
    pub fn pts(&self) -> i64 {
        unsafe { (*self.p).pts }
    }

    /// Decoding timestamp, in the encoder's time base.
    pub fn dts(&self) -> i64 {
        unsafe { (*self.p).dts }
    }
}

pub struct VideoEncoder {
    ctx: *mut ff::AVCodecContext,
    frames: *mut ff::AVBufferRef,
    pkt: *mut ff::AVPacket,
}

// SAFETY: the encoder is only used from one thread at a time; FFmpeg codec
// contexts have no thread affinity.
unsafe impl Send for VideoEncoder {}

impl VideoEncoder {
    pub fn new(device: &HwDevice, cfg: &VideoEncoderConfig) -> Result<Self> {
        unsafe {
            let codec = ff::avcodec_find_encoder_by_name(cfg.codec.encoder_name().as_ptr());
            if codec.is_null() {
                bail!("{:?} is not available in this FFmpeg build", cfg.codec.encoder_name());
            }
            // Built first so Drop cleans up whatever was allocated if a later step fails.
            let mut enc = Self { ctx: ff::avcodec_alloc_context3(codec), frames: ptr::null_mut(), pkt: ff::av_packet_alloc() };
            if enc.ctx.is_null() || enc.pkt.is_null() {
                bail!("out of memory while creating the encoder");
            }
            enc.frames = hw::alloc_nv12_frames(device, cfg.width as i32, cfg.height as i32)?;

            let fps = cfg.fps as i32;
            let c = &mut *enc.ctx;
            c.width = cfg.width as i32;
            c.height = cfg.height as i32;
            c.time_base = ff::AVRational { num: 1, den: fps };
            c.framerate = ff::AVRational { num: fps, den: 1 };
            c.pix_fmt = ff::AVPixelFormat::AV_PIX_FMT_D3D11;
            c.hw_frames_ctx = ff::av_buffer_ref(enc.frames);
            // A keyframe every second, so a saved clip starts within 1 s of the requested time.
            c.gop_size = fps;
            c.max_b_frames = 0;
            c.bit_rate = 0;
            c.rc_max_rate = cfg.max_bitrate as i64;
            c.rc_buffer_size = cfg.max_bitrate.saturating_mul(2).min(i32::MAX as u32) as i32;
            c.color_range = ff::AVColorRange::AVCOL_RANGE_MPEG;
            c.color_primaries = ff::AVColorPrimaries::AVCOL_PRI_BT709;
            c.color_trc = ff::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
            c.colorspace = ff::AVColorSpace::AVCOL_SPC_BT709;
            // MP4 stores codec headers once, up front.
            c.flags |= ff::AV_CODEC_FLAG_GLOBAL_HEADER as i32;

            set_opt(c.priv_data, "preset", "p4")?;
            set_opt(c.priv_data, "tune", "hq")?;
            set_opt(c.priv_data, "rc", "vbr")?;
            set_opt(c.priv_data, "cq", &cfg.quality.to_string())?;
            // Each NVENC surface holds an output buffer in system RAM (~9.5 MB at
            // 1440p). FFmpeg defaults to 4; we feed one frame at a time and drain
            // immediately, so 2 keeps up at 60 fps and saves ~19 MB.
            set_opt(c.priv_data, "surfaces", "2")?;

            check(ff::avcodec_open2(enc.ctx, codec, ptr::null_mut()), "avcodec_open2")?;
            Ok(enc)
        }
    }

    /// Takes a frame from the encoder's GPU texture pool.
    pub fn alloc_frame(&self) -> Result<HwFrame> {
        HwFrame::alloc(self.frames)
    }

    /// Encodes `frame` as frame number `pts` and passes every finished packet to `sink`.
    /// The same frame may be sent again with a later `pts` to repeat it.
    pub fn encode(&mut self, frame: &mut HwFrame, pts: i64, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
        unsafe {
            let f = frame.as_ptr();
            (*f).pts = pts;
            check(ff::avcodec_send_frame(self.ctx, f), "avcodec_send_frame")?;
        }
        self.drain(sink)
    }

    /// Flushes the encoder; call once after the last frame.
    pub fn finish(&mut self, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
        unsafe { check(ff::avcodec_send_frame(self.ctx, ptr::null()), "avcodec_send_frame(flush)")? };
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
            check(ret, "avcodec_receive_packet")?;
            let result = sink(&mut Packet::wrap(self.pkt));
            unsafe { ff::av_packet_unref(self.pkt) };
            result?;
        }
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        unsafe {
            ff::avcodec_free_context(&mut self.ctx);
            ff::av_buffer_unref(&mut self.frames);
            ff::av_packet_free(&mut self.pkt);
        }
    }
}

fn set_opt(obj: *mut std::ffi::c_void, name: &str, value: &str) -> Result<()> {
    let (n, v) = (CString::new(name)?, CString::new(value)?);
    check(unsafe { ff::av_opt_set(obj, n.as_ptr(), v.as_ptr(), 0) }, &format!("setting encoder option {name}={value}"))?;
    Ok(())
}
