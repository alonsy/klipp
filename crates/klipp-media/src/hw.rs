//! FFmpeg's D3D11 hardware device and frame pool, wrapped around our own D3D11 device.

use std::ffi::c_void;

use anyhow::{Result, bail};
use ffmpeg_sys_next as ff;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::core::Interface;

use crate::error::check;

// Mirrors of the public structs in libavutil/hwcontext_d3d11va.h.
// Their layout has not changed since FFmpeg 4.

#[repr(C)]
struct AVD3D11VADeviceContext {
    device: *mut c_void,
    device_context: *mut c_void,
    video_device: *mut c_void,
    video_context: *mut c_void,
    lock: Option<unsafe extern "C" fn(*mut c_void)>,
    unlock: Option<unsafe extern "C" fn(*mut c_void)>,
    lock_ctx: *mut c_void,
}

#[repr(C)]
struct AVD3D11VAFramesContext {
    texture: *mut c_void,
    bind_flags: u32,
    misc_flags: u32,
    texture_infos: *mut c_void,
}

/// An FFmpeg hardware device backed by our own `ID3D11Device`, so captured
/// textures and encoder input live on the same GPU device.
pub struct HwDevice {
    pub(crate) r: *mut ff::AVBufferRef,
}

// SAFETY: D3D11 devices are free-threaded, and FFmpeg guards its use of the
// immediate context with its own lock.
unsafe impl Send for HwDevice {}

impl HwDevice {
    pub fn from_d3d11(device: &ID3D11Device, context: &ID3D11DeviceContext) -> Result<Self> {
        unsafe {
            let r = ff::av_hwdevice_ctx_alloc(ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA);
            if r.is_null() {
                bail!("av_hwdevice_ctx_alloc returned null");
            }
            let this = Self { r };
            let hw = (*((*r).data as *mut ff::AVHWDeviceContext)).hwctx as *mut AVD3D11VADeviceContext;
            // FFmpeg releases these when the device is freed, so it gets references of its own.
            (*hw).device = device.clone().into_raw();
            (*hw).device_context = context.clone().into_raw();
            check(ff::av_hwdevice_ctx_init(r), "av_hwdevice_ctx_init")?;
            Ok(this)
        }
    }
}

impl Drop for HwDevice {
    fn drop(&mut self) {
        unsafe { ff::av_buffer_unref(&mut self.r) };
    }
}

/// Creates a pool of NV12 GPU textures that the encoder reads from and our
/// video processor renders into. They're made on demand, one texture each,
/// then reused. (A pool sized up front would be one texture array: Intel's
/// driver refuses that for render targets, and Quick Sync could only find
/// its first slice.)
pub(crate) fn alloc_nv12_frames(device: &HwDevice, width: i32, height: i32) -> Result<*mut ff::AVBufferRef> {
    unsafe {
        let mut r = ff::av_hwframe_ctx_alloc(device.r);
        if r.is_null() {
            bail!("av_hwframe_ctx_alloc returned null");
        }
        let frames = (*r).data as *mut ff::AVHWFramesContext;
        (*frames).format = ff::AVPixelFormat::AV_PIX_FMT_D3D11;
        (*frames).sw_format = ff::AVPixelFormat::AV_PIX_FMT_NV12;
        (*frames).width = width;
        (*frames).height = height;
        let hw = (*frames).hwctx as *mut AVD3D11VAFramesContext;
        (*hw).bind_flags = D3D11_BIND_RENDER_TARGET.0 as u32;
        if let Err(e) = check(ff::av_hwframe_ctx_init(r), "av_hwframe_ctx_init") {
            ff::av_buffer_unref(&mut r);
            return Err(e);
        }
        Ok(r)
    }
}

/// One GPU frame from the encoder's texture pool. Returns to the pool when dropped.
pub struct HwFrame {
    f: *mut ff::AVFrame,
    texture: ID3D11Texture2D,
    slice: u32,
}

impl HwFrame {
    pub(crate) fn alloc(frames: *mut ff::AVBufferRef) -> Result<Self> {
        unsafe {
            let mut f = ff::av_frame_alloc();
            if f.is_null() {
                bail!("av_frame_alloc returned null");
            }
            if let Err(e) = check(ff::av_hwframe_get_buffer(frames, f, 0), "av_hwframe_get_buffer") {
                ff::av_frame_free(&mut f);
                return Err(e);
            }
            // For D3D11 frames, data[0] is the texture and data[1] its array slice.
            let raw = (*f).data[0] as *mut c_void;
            let texture = ID3D11Texture2D::from_raw_borrowed(&raw)
                .expect("D3D11 frame without a texture")
                .clone();
            let slice = (*f).data[1] as usize as u32;
            Ok(Self { f, texture, slice })
        }
    }

    /// The texture and array slice that hold this frame's pixels.
    pub fn texture(&self) -> (&ID3D11Texture2D, u32) {
        (&self.texture, self.slice)
    }

    pub(crate) fn as_ptr(&mut self) -> *mut ff::AVFrame {
        self.f
    }
}

impl Drop for HwFrame {
    fn drop(&mut self) {
        unsafe { ff::av_frame_free(&mut self.f) };
    }
}
