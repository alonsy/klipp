use std::ffi::{CStr, c_char, c_int};

use anyhow::{Result, bail};
use ffmpeg_sys_next as ff;

/// `AVERROR(EAGAIN)`: the encoder wants more input before it can output a packet.
/// EAGAIN is 11 in both the MSVC and MinGW C runtimes.
pub(crate) const AVERROR_EAGAIN: c_int = -11;

/// Turns a negative FFmpeg return code into an error that names the failed call.
pub(crate) fn check(ret: c_int, what: &str) -> Result<c_int> {
    if ret < 0 {
        bail!("{what} failed: {}", describe(ret));
    }
    Ok(ret)
}

fn describe(code: c_int) -> String {
    let mut buf = [0 as c_char; 256];
    unsafe {
        ff::av_strerror(code, buf.as_mut_ptr(), buf.len());
        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}
