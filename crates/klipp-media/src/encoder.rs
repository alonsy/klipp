//! Hardware video encoding from D3D11 textures, on the graphics card's own
//! encoder: NVIDIA NVENC, AMD AMF or Intel Quick Sync.

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

/// Whose hardware encoder: the one on the graphics card that records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
}

impl Vendor {
    /// From a PCI vendor id (`DXGI_ADAPTER_DESC1::VendorId`); `None` for
    /// cards without an encoder Klipp can use.
    pub fn from_pci_id(id: u32) -> Option<Self> {
        match id {
            0x10DE => Some(Self::Nvidia),
            0x1002 | 0x1022 => Some(Self::Amd),
            0x8086 => Some(Self::Intel),
            _ => None,
        }
    }

    /// The encoder's name, for logs and the setup screen.
    pub fn encoder(self) -> &'static str {
        match self {
            Self::Nvidia => "NVENC",
            Self::Amd => "AMD AMF",
            Self::Intel => "Intel Quick Sync",
        }
    }
}

fn encoder_name(vendor: Vendor, codec: VideoCodec) -> &'static CStr {
    match (vendor, codec) {
        (Vendor::Nvidia, VideoCodec::Av1) => c"av1_nvenc",
        (Vendor::Nvidia, VideoCodec::H264) => c"h264_nvenc",
        (Vendor::Amd, VideoCodec::Av1) => c"av1_amf",
        (Vendor::Amd, VideoCodec::H264) => c"h264_amf",
        (Vendor::Intel, VideoCodec::Av1) => c"av1_qsv",
        (Vendor::Intel, VideoCodec::H264) => c"h264_qsv",
    }
}

#[derive(Clone, Debug)]
pub struct VideoEncoderConfig {
    pub vendor: Vendor,
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Constant-quality target on NVENC's scale (0–51, lower is better
    /// quality); AMD's scale is mapped from it.
    pub quality: u8,
    /// Hard bitrate ceiling in bits/s. Keeps the replay buffer's size predictable.
    pub max_bitrate: u32,
    /// The encoder's fastest preset: less work on the card for a little less
    /// quality per bit (Performance+).
    pub light: bool,
}

/// Quick Sync takes its own frame type: a Quick Sync device made from our
/// D3D11 device, and a view of our texture pool as Quick Sync surfaces, made
/// per frame as textures come and go. Each frame is shared, not copied.
struct Qsv {
    device: *mut ff::AVBufferRef,
    frames: *mut ff::AVBufferRef,
}

impl Qsv {
    fn derive(device: &HwDevice, d3d11_frames: *mut ff::AVBufferRef) -> Result<Self> {
        unsafe {
            let mut qsv = Self { device: ptr::null_mut(), frames: ptr::null_mut() };
            check(
                ff::av_hwdevice_ctx_create_derived(&mut qsv.device, ff::AVHWDeviceType::AV_HWDEVICE_TYPE_QSV, device.r, 0),
                "starting Quick Sync on this graphics card",
            )?;
            check(
                ff::av_hwframe_ctx_create_derived(&mut qsv.frames, ff::AVPixelFormat::AV_PIX_FMT_QSV, qsv.device, d3d11_frames, 0),
                "sharing textures with Quick Sync",
            )?;
            Ok(qsv)
        }
    }

    /// `frame` as a Quick Sync surface (it keeps the D3D11 frame alive).
    fn map(&self, frame: *const ff::AVFrame) -> Result<*mut ff::AVFrame> {
        unsafe {
            let mut mapped = ff::av_frame_alloc();
            if mapped.is_null() {
                bail!("av_frame_alloc returned null");
            }
            (*mapped).format = ff::AVPixelFormat::AV_PIX_FMT_QSV as i32;
            (*mapped).hw_frames_ctx = ff::av_buffer_ref(self.frames);
            let flags = ff::AV_HWFRAME_MAP_READ as i32 | ff::AV_HWFRAME_MAP_DIRECT as i32;
            if let Err(e) = check(ff::av_hwframe_map(mapped, frame, flags), "handing the frame to Quick Sync") {
                ff::av_frame_free(&mut mapped);
                return Err(e);
            }
            Ok(mapped)
        }
    }
}

impl Drop for Qsv {
    fn drop(&mut self) {
        unsafe {
            ff::av_buffer_unref(&mut self.frames);
            ff::av_buffer_unref(&mut self.device);
        }
    }
}

/// The size of the encoder's input textures. Quick Sync works on surfaces
/// whose sides are multiples of 16 (FFmpeg sizes its own Quick Sync textures
/// that way) and crops them to the picture, so 1080p gets 1088 rows. The
/// picture fills the top-left `width` × `height` of each texture.
fn texture_size(cfg: &VideoEncoderConfig) -> (u32, u32) {
    match cfg.vendor {
        Vendor::Intel => (cfg.width.next_multiple_of(16), cfg.height.next_multiple_of(16)),
        Vendor::Nvidia | Vendor::Amd => (cfg.width, cfg.height),
    }
}

/// AMD's QVBR quality level (1–51, higher is better) for an NVENC-scale
/// constant-quality value (lower is better): both are QP-like, mirrored.
pub(crate) fn amf_quality_level(nvenc_cq: u8) -> u8 {
    52u8.saturating_sub(nvenc_cq).clamp(1, 51)
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
    /// Intel only.
    qsv: Option<Qsv>,
    vendor: Vendor,
}

// SAFETY: the encoder is only used from one thread at a time; FFmpeg codec
// contexts have no thread affinity.
unsafe impl Send for VideoEncoder {}

impl VideoEncoder {
    pub fn new(device: &HwDevice, cfg: &VideoEncoderConfig) -> Result<Self> {
        unsafe {
            let name = encoder_name(cfg.vendor, cfg.codec);
            let codec = ff::avcodec_find_encoder_by_name(name.as_ptr());
            if codec.is_null() {
                bail!("{name:?} is not available in this FFmpeg build");
            }
            // Built first so Drop cleans up whatever was allocated if a later step fails.
            let mut enc = Self { ctx: ff::avcodec_alloc_context3(codec), frames: ptr::null_mut(), pkt: ff::av_packet_alloc(), qsv: None, vendor: cfg.vendor };
            if enc.ctx.is_null() || enc.pkt.is_null() {
                bail!("out of memory while creating the encoder");
            }
            let intel = cfg.vendor == Vendor::Intel;
            let (w, h) = texture_size(cfg);
            enc.frames = hw::alloc_nv12_frames(device, w as i32, h as i32)?;

            let fps = cfg.fps as i32;
            let c = &mut *enc.ctx;
            c.width = cfg.width as i32;
            c.height = cfg.height as i32;
            c.time_base = ff::AVRational { num: 1, den: fps };
            c.framerate = ff::AVRational { num: fps, den: 1 };
            if intel {
                let qsv = Qsv::derive(device, enc.frames)?;
                c.pix_fmt = ff::AVPixelFormat::AV_PIX_FMT_QSV;
                c.hw_frames_ctx = ff::av_buffer_ref(qsv.frames);
                enc.qsv = Some(qsv);
            } else {
                // NVENC and AMF read the D3D11 textures directly.
                c.pix_fmt = ff::AVPixelFormat::AV_PIX_FMT_D3D11;
                c.hw_frames_ctx = ff::av_buffer_ref(enc.frames);
            }
            // A keyframe every second, so a saved clip starts within 1 s of the requested time.
            c.gop_size = fps;
            c.max_b_frames = 0;
            set_rates(c, cfg.vendor, cfg.max_bitrate);
            c.color_range = ff::AVColorRange::AVCOL_RANGE_MPEG;
            c.color_primaries = ff::AVColorPrimaries::AVCOL_PRI_BT709;
            c.color_trc = ff::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
            c.colorspace = ff::AVColorSpace::AVCOL_SPC_BT709;
            // MP4 stores codec headers once, up front.
            c.flags |= ff::AV_CODEC_FLAG_GLOBAL_HEADER as i32;

            match cfg.vendor {
                Vendor::Nvidia => {
                    // Constant quality, capped at the ceiling.
                    set_opt(c.priv_data, "preset", if cfg.light { "p1" } else { "p4" })?;
                    set_opt(c.priv_data, "tune", "hq")?;
                    set_opt(c.priv_data, "rc", "vbr")?;
                    set_opt(c.priv_data, "cq", &cfg.quality.to_string())?;
                    // Each NVENC surface holds an output buffer in system RAM (~9.5 MB at
                    // 1440p). With 2, every frame waited for the one before it to finish
                    // encoding, so a card busy with a game set the recorder's pace, down to
                    // a few frames a second. FFmpeg's default of 4 keeps three
                    // in flight, and a slow frame makes the next ones late, not lost.
                    set_opt(c.priv_data, "surfaces", "4")?;
                }
                Vendor::Amd => {
                    // Quality-defined variable bitrate (AMD's constant quality), peaking at the ceiling.
                    set_opt(c.priv_data, "usage", "transcoding")?;
                    set_opt(c.priv_data, "quality", if cfg.light { "speed" } else { "balanced" })?;
                    set_opt(c.priv_data, "rc", "qvbr")?;
                    set_opt(c.priv_data, "qvbr_quality_level", &amf_quality_level(cfg.quality).to_string())?;
                    set_opt(c.priv_data, "bf", "0")?;
                    // Frames in flight; each holds GPU and system memory. With 2, handing
                    // over a frame waited for the one before it to finish: 12.9 ms of
                    // each 16.7 on an RX 6650 XT at 1080p60 on the desktop, and late
                    // frames in games (a tester's log, 2026-10-25). 4, as NVENC.
                    set_opt(c.priv_data, "async_depth", "4")?;
                }
                Vendor::Intel => {
                    // Plain variable bitrate: the mode every Quick Sync generation and
                    // codec supports (AV1 has no quality-defined mode on all of them).
                    // Already the fastest preset, light or not.
                    set_opt(c.priv_data, "preset", "veryfast")?;
                    set_opt(c.priv_data, "async_depth", "2")?;
                }
            }

            check(ff::avcodec_open2(enc.ctx, codec, ptr::null_mut()), "avcodec_open2")?;
            Ok(enc)
        }
    }

    /// Takes a frame from the encoder's GPU texture pool.
    pub fn alloc_frame(&self) -> Result<HwFrame> {
        HwFrame::alloc(self.frames)
    }

    /// The texture pool, for a thread other than the encoder's to take frames from.
    pub fn frame_pool(&self) -> Result<FramePool> {
        let r = unsafe { ff::av_buffer_ref(self.frames) };
        if r.is_null() {
            bail!("out of memory while sharing the texture pool");
        }
        Ok(FramePool(r))
    }

    /// Encodes `frame` as frame number `pts` and passes every finished packet to `sink`.
    /// The same frame may be sent again with a later `pts` to repeat it.
    pub fn encode(&mut self, frame: &mut HwFrame, pts: i64, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
        self.send(frame, pts)?;
        self.drain(sink)
    }

    /// A new bitrate ceiling for the frames sent from now on (Performance
    /// mode lowers it while a game keeps the card busy). NVENC takes it with
    /// its next frame (20 to 6 Mbit/s within a second, on noise); FFmpeg
    /// passes it to Quick Sync the same way (not measured yet); AMF keeps the
    /// one it opened with.
    pub fn set_max_bitrate(&mut self, max_bitrate: u32) {
        unsafe { set_rates(&mut *self.ctx, self.vendor, max_bitrate) };
    }

    /// The first half of `encode`: hands the frame to the encoder. With a GPU
    /// encoder this is where the wait for the card to finish drawing it shows.
    pub fn send(&mut self, frame: &mut HwFrame, pts: i64) -> Result<()> {
        unsafe {
            let f = frame.as_ptr();
            (*f).pts = pts;
            match &self.qsv {
                Some(qsv) => {
                    let mut mapped = qsv.map(f)?;
                    (*mapped).pts = pts;
                    let sent = check(ff::avcodec_send_frame(self.ctx, mapped), "avcodec_send_frame");
                    ff::av_frame_free(&mut mapped);
                    sent?;
                }
                None => {
                    check(ff::avcodec_send_frame(self.ctx, f), "avcodec_send_frame")?;
                }
            }
        }
        Ok(())
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

    /// The second half of `encode`: passes every finished packet to `sink`.
    pub fn drain(&mut self, sink: &mut dyn FnMut(&mut Packet) -> Result<()>) -> Result<()> {
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

/// The encoder's texture pool, held by another thread: frames are taken here
/// and handed to the encoder (FFmpeg's pools are thread-safe).
pub struct FramePool(*mut ff::AVBufferRef);

// SAFETY: a reference to an FFmpeg frames context; taking buffers from it is locked inside FFmpeg.
unsafe impl Send for FramePool {}

impl FramePool {
    pub fn alloc(&self) -> Result<HwFrame> {
        HwFrame::alloc(self.0)
    }
}

impl Drop for FramePool {
    fn drop(&mut self) {
        unsafe { ff::av_buffer_unref(&mut self.0) };
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        unsafe {
            ff::avcodec_free_context(&mut self.ctx);
            // The Quick Sync view of the pool goes before the pool itself.
            self.qsv = None;
            ff::av_buffer_unref(&mut self.frames);
            ff::av_packet_free(&mut self.pkt);
        }
    }
}

/// The ceiling, the rate-control buffer (two seconds of it) and the target:
/// none on NVENC (constant quality up to the ceiling), three fifths of the
/// ceiling on AMF and Quick Sync.
fn set_rates(c: &mut ff::AVCodecContext, vendor: Vendor, max_bitrate: u32) {
    c.rc_max_rate = max_bitrate as i64;
    c.rc_buffer_size = max_bitrate.saturating_mul(2).min(i32::MAX as u32) as i32;
    c.bit_rate = match vendor {
        Vendor::Nvidia => 0,
        Vendor::Amd | Vendor::Intel => max_bitrate as i64 * 3 / 5,
    };
}

pub(crate) fn set_opt(obj: *mut std::ffi::c_void, name: &str, value: &str) -> Result<()> {
    let (n, v) = (CString::new(name)?, CString::new(value)?);
    check(unsafe { ff::av_opt_set(obj, n.as_ptr(), v.as_ptr(), 0) }, &format!("setting encoder option {name}={value}"))?;
    Ok(())
}
