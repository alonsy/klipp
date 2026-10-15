//! Writing encoded packets into an MP4 file.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use anyhow::{Context, Result, bail};
use ffmpeg_sys_next as ff;

use crate::error::check;
use crate::stream::StreamInfo;

/// A named point in the file that players can jump to.
pub struct Chapter {
    pub start_ms: i64,
    pub end_ms: i64,
    pub title: String,
}

pub struct Mp4Writer {
    fmt: *mut ff::AVFormatContext,
    pkt: *mut ff::AVPacket,
    /// Per stream: (time base of incoming timestamps, time base the muxer chose).
    time_bases: Vec<(ff::AVRational, ff::AVRational)>,
}

impl Mp4Writer {
    /// Creates `path` (always MP4, whatever the extension) with one track per
    /// stream, in order, and the given chapters.
    pub fn create(path: &Path, streams: &[StreamInfo], chapters: &[Chapter]) -> Result<Self> {
        let cpath = CString::new(path.to_str().context("output path is not valid UTF-8")?)?;
        unsafe {
            let mut fmt = ptr::null_mut();
            check(
                ff::avformat_alloc_output_context2(&mut fmt, ptr::null(), c"mp4".as_ptr(), cpath.as_ptr()),
                "avformat_alloc_output_context2",
            )?;
            // Built first so Drop cleans up if a later step fails.
            let mut w = Self { fmt, pkt: ff::av_packet_alloc(), time_bases: Vec::with_capacity(streams.len()) };
            if w.pkt.is_null() {
                bail!("av_packet_alloc returned null");
            }
            for info in streams {
                let st = ff::avformat_new_stream(fmt, ptr::null());
                if st.is_null() {
                    bail!("avformat_new_stream returned null");
                }
                check(ff::avcodec_parameters_copy((*st).codecpar, info.params()), "avcodec_parameters_copy")?;
                (*st).time_base = info.time_base();
                if let Some(title) = info.title() {
                    // MP4 players read the handler name; other containers use the title.
                    ff::av_dict_set(&mut (*st).metadata, c"title".as_ptr(), title.as_ptr(), 0);
                    ff::av_dict_set(&mut (*st).metadata, c"handler_name".as_ptr(), title.as_ptr(), 0);
                }
                (*st).disposition = if info.is_default() { ff::AV_DISPOSITION_DEFAULT as i32 } else { 0 };
            }
            // The muxer sets up its chapter track in write_header, so chapters go in first.
            // avformat_free_context frees them.
            for (i, c) in chapters.iter().enumerate() {
                let ch = ff::av_mallocz(std::mem::size_of::<ff::AVChapter>()) as *mut ff::AVChapter;
                if ch.is_null() {
                    bail!("av_mallocz returned null");
                }
                (*ch).id = i as i64;
                (*ch).time_base = ff::AVRational { num: 1, den: 1000 };
                (*ch).start = c.start_ms;
                (*ch).end = c.end_ms;
                let title = CString::new(c.title.replace('\0', ""))?;
                ff::av_dict_set(&mut (*ch).metadata, c"title".as_ptr(), title.as_ptr(), 0);
                ff::av_dynarray_add(
                    &mut (*fmt).chapters as *mut _ as *mut std::ffi::c_void,
                    &mut (*fmt).nb_chapters as *mut u32 as *mut i32,
                    ch as *mut std::ffi::c_void,
                );
                if (*fmt).nb_chapters as usize != i + 1 {
                    bail!("av_dynarray_add failed");
                }
            }
            check(ff::avio_open(&mut (*fmt).pb, cpath.as_ptr(), ff::AVIO_FLAG_WRITE as i32), "avio_open")?;
            check(ff::avformat_write_header(fmt, ptr::null_mut()), "avformat_write_header")?;
            // The muxer may pick its own time base per stream.
            for (i, info) in streams.iter().enumerate() {
                let st = *(*fmt).streams.add(i);
                w.time_bases.push((info.time_base(), (*st).time_base));
            }
            Ok(w)
        }
    }

    /// Writes one packet. Timestamps are in the stream's own time base.
    pub fn write(&mut self, stream: usize, data: &[u8], pts: i64, dts: i64, duration: i64, key: bool) -> Result<()> {
        let &(src, dst) = self.time_bases.get(stream).context("no such stream")?;
        unsafe {
            let p = self.pkt;
            // Not reference-counted, so the muxer copies the bytes it needs to keep.
            (*p).buf = ptr::null_mut();
            (*p).data = data.as_ptr() as *mut u8;
            (*p).size = data.len().try_into().context("packet too large")?;
            (*p).pts = pts;
            (*p).dts = dts;
            (*p).duration = duration;
            (*p).flags = if key { ff::AV_PKT_FLAG_KEY as i32 } else { 0 };
            (*p).stream_index = stream as i32;
            ff::av_packet_rescale_ts(p, src, dst);
            check(ff::av_interleaved_write_frame(self.fmt, p), "av_interleaved_write_frame")?;
        }
        Ok(())
    }

    /// Writes the MP4 index. Without it the file is unplayable.
    pub fn finish(self) -> Result<()> {
        check(unsafe { ff::av_write_trailer(self.fmt) }, "av_write_trailer")?;
        Ok(())
    }
}

impl Drop for Mp4Writer {
    fn drop(&mut self) {
        unsafe {
            if !(*self.fmt).pb.is_null() {
                ff::avio_closep(&mut (*self.fmt).pb);
            }
            ff::avformat_free_context(self.fmt);
            ff::av_packet_free(&mut self.pkt);
        }
    }
}
