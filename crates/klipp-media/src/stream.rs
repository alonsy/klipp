//! What a muxer needs to know about an encoded stream.

use std::ffi::CString;
use std::ptr;

use anyhow::{Result, bail};
use ffmpeg_sys_next as ff;

use crate::error::check;

/// Codec parameters and time base of one encoded stream, plus how the track
/// should be labeled. Owned and `Send`, so a clip can be written on a worker
/// thread while the encoder keeps running.
pub struct StreamInfo {
    params: *mut ff::AVCodecParameters,
    time_base: ff::AVRational,
    title: Option<CString>,
    is_default: bool,
}

// SAFETY: AVCodecParameters is plain data with no thread affinity; we own it exclusively.
unsafe impl Send for StreamInfo {}

impl StreamInfo {
    pub(crate) fn from_codec(ctx: *const ff::AVCodecContext) -> Result<Self> {
        unsafe {
            let params = ff::avcodec_parameters_alloc();
            if params.is_null() {
                bail!("avcodec_parameters_alloc returned null");
            }
            let info = Self { params, time_base: (*ctx).time_base, title: None, is_default: false };
            check(ff::avcodec_parameters_from_context(params, ctx), "avcodec_parameters_from_context")?;
            Ok(info)
        }
    }

    /// Track name shown by players and editors, e.g. "Game" or "Mic".
    pub fn with_title(mut self, title: &str) -> Self {
        self.title = CString::new(title).ok();
        self
    }

    /// Marks this as the track players pick by default.
    pub fn as_default(mut self) -> Self {
        self.is_default = true;
        self
    }

    pub(crate) fn params(&self) -> *const ff::AVCodecParameters {
        self.params
    }

    pub(crate) fn time_base(&self) -> ff::AVRational {
        self.time_base
    }

    pub(crate) fn title(&self) -> Option<&CString> {
        self.title.as_ref()
    }

    pub(crate) fn is_default(&self) -> bool {
        self.is_default
    }
}

impl Clone for StreamInfo {
    fn clone(&self) -> Self {
        unsafe {
            let params = ff::avcodec_parameters_alloc();
            assert!(!params.is_null(), "out of memory copying codec parameters");
            assert!(ff::avcodec_parameters_copy(params, self.params) >= 0, "copying codec parameters failed");
            Self { params, time_base: self.time_base, title: self.title.clone(), is_default: self.is_default }
        }
    }
}

impl Drop for StreamInfo {
    fn drop(&mut self) {
        unsafe { ff::avcodec_parameters_free(&mut self.params) };
        self.params = ptr::null_mut();
    }
}
