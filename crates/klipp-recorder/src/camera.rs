//! Content Creator Mode (experimental): the webcam, recorded
//! with the screen. Its pictures become a "Camera" video track in every clip,
//! on the same clock as the screen and the audio, for the studio's Content
//! Creation Center to put on top of the game.
//!
//! Pictures come from Windows' frame reader as NV12, each stamped with the
//! performance counter at the moment it was captured (`SystemRelativeTime`),
//! and go to the graphics card's H.264 encoder.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use klipp_media::{CAMERA_CLOCK, CameraEncoder, Packet, Vendor};
use serde::{Deserialize, Serialize};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{BitmapBufferAccessMode, BitmapPixelFormat, BitmapSize};
use windows::Media::Capture::Frames::{
    MediaFrameArrivedEventArgs, MediaFrameFormat, MediaFrameReader, MediaFrameReaderAcquisitionMode, MediaFrameReaderStartStatus,
    MediaFrameSourceGroup, MediaFrameSourceKind,
};
use windows::Media::Capture::{
    MediaCapture, MediaCaptureInitializationSettings, MediaCaptureMemoryPreference, MediaCaptureSharingMode, MediaStreamType,
    StreamingCaptureMode,
};
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::System::WinRT::{IMemoryBufferByteAccess, RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::{HSTRING, Interface};

use crate::clock;
use crate::log::{info, warning};
use crate::replay::PacketInfo;
use crate::state::Shared;

/// Buffer stream ids for the camera: each start gets the next one, so a clip
/// never mixes pictures from two encoders.
pub const FIRST_STREAM: u8 = 64;
pub const STREAMS: u8 = 64;

/// A picture stamped further from now than this isn't on the shared clock.
const CLOCK_TOLERANCE: i64 = clock::SECOND;
/// Pictures waiting for the encoder; more are left out (the clip shows the one before longer).
const QUEUE: usize = 4;
/// A camera that sends nothing for this long (after starting) has stopped.
const QUIET: i64 = 5 * clock::SECOND;
/// Opening a camera can take this long before its first picture.
const STARTING: i64 = 15 * clock::SECOND;
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

/// What the Content Creation Center chose, in `%APPDATA%\Klipp\creator.json`.
/// A file of its own: settings.toml is shared with builds that don't have this.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CameraSettings {
    pub enabled: bool,
    /// The camera's name ("" = the first one Windows lists).
    pub camera: String,
    /// The tallest picture to ask for, and the frame rate.
    pub height: u32,
    pub fps: u32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self { enabled: false, camera: String::new(), height: 1080, fps: 30 }
    }
}

impl CameraSettings {
    pub fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("Klipp").join("creator.json"))
    }

    /// The saved choice, or off if there's none (or it can't be read).
    pub fn load() -> Self {
        let mut s: Self = Self::path().and_then(|p| fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        s.height = s.height.clamp(360, 2160);
        s.fps = s.fps.clamp(15, 60);
        s
    }

    /// The camera's bitrate ceiling, for the replay buffer's byte cap.
    pub fn max_bitrate(&self) -> u32 {
        match self.height {
            ..=720 => 6_000_000,
            ..=1080 => 10_000_000,
            _ => 16_000_000,
        }
    }
}

/// A camera Windows offers: a group of frame sources with a colour picture.
pub struct Device {
    pub name: String,
    group: MediaFrameSourceGroup,
    source: HSTRING,
}

pub fn devices() -> Result<Vec<Device>> {
    let groups = MediaFrameSourceGroup::FindAllAsync()?.join()?;
    let mut out = Vec::new();
    for i in 0..groups.Size()? {
        let group = groups.GetAt(i)?;
        let infos = group.SourceInfos()?;
        // The recording stream if there is one; some cameras only have a preview stream.
        let mut best: Option<(HSTRING, bool)> = None;
        for j in 0..infos.Size()? {
            let info = infos.GetAt(j)?;
            if info.SourceKind()? != MediaFrameSourceKind::Color {
                continue;
            }
            let record = match info.MediaStreamType()? {
                MediaStreamType::VideoRecord => true,
                MediaStreamType::VideoPreview => false,
                _ => continue,
            };
            if best.as_ref().is_none_or(|&(_, r)| record && !r) {
                best = Some((info.Id()?, record));
            }
        }
        if let Some((source, _)) = best {
            out.push(Device { name: group.DisplayName()?.to_string(), group, source });
        }
    }
    Ok(out)
}

/// `--list-cameras`: their names as JSON, for the Content Creation Center.
pub fn list_json() -> Result<String> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let names: Vec<String> = devices()?.into_iter().map(|d| d.name).collect();
    Ok(serde_json::to_string(&names)?)
}

/// One of the formats a camera offers.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mode {
    width: u32,
    height: u32,
    fps: f64,
    /// MJPEG or another compressed format (decoded on the way to NV12).
    compressed: bool,
}

/// The format to ask for: the tallest picture up to `height` (the smallest
/// there is, if every one is taller), then the frame rate closest to `fps`,
/// then an uncompressed one over MJPEG.
fn pick(modes: &[Mode], height: u32, fps: u32) -> Option<usize> {
    let fits = modes.iter().any(|m| m.height <= height);
    let ok = |m: &Mode| !fits || m.height <= height;
    let best_height = modes.iter().filter(|m| ok(m)).map(|m| m.height).reduce(|a, b| if fits { a.max(b) } else { a.min(b) })?;
    modes
        .iter()
        .enumerate()
        .filter(|(_, m)| ok(m) && m.height == best_height)
        .min_by(|(_, a), (_, b)| {
            let off = |m: &Mode| (m.fps - fps as f64).abs();
            off(a).total_cmp(&off(b)).then(a.compressed.cmp(&b.compressed)).then(b.width.cmp(&a.width))
        })
        .map(|(i, _)| i)
}

fn mode_of(f: &MediaFrameFormat) -> Result<Mode> {
    let video = f.VideoFormat()?;
    let rate = f.FrameRate()?;
    let subtype = f.Subtype()?.to_string().to_uppercase();
    Ok(Mode {
        width: video.Width()?,
        height: video.Height()?,
        fps: rate.Numerator()? as f64 / rate.Denominator()?.max(1) as f64,
        compressed: !["NV12", "YUY2", "YV12", "I420", "RGB24", "RGB32", "ARGB32"].contains(&subtype.as_str()),
    })
}

/// What the camera is doing, for the Content Creation Center
/// (`%LOCALAPPDATA%\Klipp\camera.json`; only read while the recorder runs).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status<'a> {
    /// "starting", "recording", "error" or "off".
    state: &'a str,
    camera: &'a str,
    width: u32,
    height: u32,
    fps: f64,
    /// Another app had the camera, so Klipp shares it (and can't pick its format).
    shared: bool,
    error: &'a str,
}

fn status_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Klipp").join("camera.json"))
}

fn write_status(s: &Status) {
    let Some(path) = status_path() else { return };
    let part = path.with_extension("json.part");
    if serde_json::to_vec(s).is_ok_and(|b| fs::write(&part, b).is_ok()) {
        let _ = fs::rename(&part, &path);
    }
}

/// The camera is off (Content Creator Mode off, recording paused, no game).
pub fn status_off() {
    write_status(&Status { state: "off", camera: "", width: 0, height: 0, fps: 0.0, shared: false, error: "" });
}

/// A picture on its way from the frame reader to the encoder.
struct Picture {
    /// When the camera captured it (shared clock), if it said.
    stamp: Option<i64>,
    /// When it reached Klipp.
    arrived: i64,
    width: usize,
    height: usize,
    /// NV12 without padding: the Y plane, then the UV plane.
    nv12: Vec<u8>,
}

pub struct CameraPipeline {
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    /// When the last picture arrived (shared clock; 0 = none yet).
    last_picture: Arc<AtomicI64>,
    started: i64,
    thread: Option<JoinHandle<()>>,
}

impl CameraPipeline {
    /// Starts the camera on its own thread and returns right away (opening a
    /// camera can take seconds). Its packets go into the buffer as `stream`.
    /// `last_error` keeps a camera that keeps failing the same way from
    /// repeating itself in the log.
    pub fn start(settings: CameraSettings, vendor: Vendor, stream: u8, shared: Arc<Shared>, last_error: Arc<Mutex<String>>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let last_picture = Arc::new(AtomicI64::new(0));
        let thread = thread::Builder::new()
            .name("camera".into())
            .spawn({
                let (stop, failed, last_picture) = (stop.clone(), failed.clone(), last_picture.clone());
                move || {
                    let result = run(&settings, vendor, stream, &shared, &stop, &last_picture);
                    // Clips saved from now on have no camera (unless a new one starts).
                    let mut camera = shared.camera.lock().unwrap();
                    if camera.as_ref().is_some_and(|(s, _)| *s == stream) {
                        *camera = None;
                    }
                    drop(camera);
                    if let Err(e) = result {
                        failed.store(true, Ordering::Relaxed);
                        let text = format!("{e:#}");
                        let mut last = last_error.lock().unwrap();
                        if *last != text {
                            warning!("camera: {text}");
                            *last = text.clone();
                        }
                        write_status(&Status { state: "error", camera: &settings.camera, width: 0, height: 0, fps: 0.0, shared: false, error: &text });
                    } else if !stop.load(Ordering::Relaxed) {
                        failed.store(true, Ordering::Relaxed);
                    }
                }
            })
            .ok();
        if thread.is_none() {
            failed.store(true, Ordering::Relaxed);
        }
        Self { stop, failed, last_picture, started: clock::now(), thread }
    }

    /// It failed, or sent no picture for a while: time to start it again.
    pub fn dead(&self) -> bool {
        let now = clock::now();
        let last = self.last_picture.load(Ordering::Relaxed);
        self.failed.load(Ordering::Relaxed) || (now - self.started > STARTING && now - last.max(self.started) > QUIET)
    }

    /// It has sent pictures for a while (its errors can be told again).
    pub fn working(&self) -> bool {
        let last = self.last_picture.load(Ordering::Relaxed);
        last != 0 && clock::now() - last < QUIET
    }
}

impl Drop for CameraPipeline {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let Some(t) = self.thread.take() else { return };
        // A camera driver that hangs must not take the main thread with it.
        let deadline = Instant::now() + STOP_TIMEOUT;
        while !t.is_finished() {
            if Instant::now() >= deadline {
                warning!("camera: the camera thread is stuck (in the camera's driver?); leaving it behind");
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = t.join();
    }
}

/// Places pictures on the shared clock: by their own capture time when the
/// camera's clock is the shared one (Windows stamps them with the performance
/// counter), otherwise by when they arrived, keeping the camera's own spacing.
#[derive(Default)]
struct Timing {
    offset: Option<i64>,
}

impl Timing {
    fn place(&mut self, stamp: Option<i64>, arrived: i64) -> i64 {
        let Some(stamp) = stamp else { return arrived };
        if self.offset.is_none() && (arrived - stamp).abs() <= CLOCK_TOLERANCE {
            return stamp;
        }
        let offset = *self.offset.get_or_insert(arrived - stamp);
        let time = stamp + offset;
        if (arrived - time).abs() > CLOCK_TOLERANCE {
            self.offset = Some(arrived - stamp);
            return arrived;
        }
        time
    }
}

fn find(name: &str) -> Result<Device> {
    let mut all = devices()?;
    if all.is_empty() {
        bail!("no camera found (is it plugged in, or turned off in Device Manager?)");
    }
    let want = name.trim().to_lowercase();
    let i = if want.is_empty() { 0 } else { all.iter().position(|d| d.name.to_lowercase().contains(&want)).with_context(|| format!("{name} isn't connected"))? };
    Ok(all.swap_remove(i))
}

/// Camera errors Windows reports, in words.
fn explain(e: windows::core::Error, device: &str) -> anyhow::Error {
    if e.code() == E_ACCESSDENIED {
        return anyhow!("Windows doesn't let apps use the camera: turn on Settings → Privacy & security → Camera → Let desktop apps access your camera");
    }
    anyhow!("{device} didn't start: {e}")
}

/// Opens the camera, with control over its format or shared with whichever
/// app has it (that app's format, which Klipp can only scale).
fn open(device: &Device, exclusive: bool) -> Result<MediaCapture> {
    let settings = MediaCaptureInitializationSettings::new()?;
    settings.SetSourceGroup(&device.group)?;
    settings.SetSharingMode(if exclusive { MediaCaptureSharingMode::ExclusiveControl } else { MediaCaptureSharingMode::SharedReadOnly })?;
    settings.SetMemoryPreference(MediaCaptureMemoryPreference::Cpu)?;
    settings.SetStreamingCaptureMode(StreamingCaptureMode::Video)?;
    let capture = MediaCapture::new()?;
    capture.InitializeWithSettingsAsync(&settings)?.join().map_err(|e| explain(e, &device.name))?;
    Ok(capture)
}

/// A started camera: its reader hands each picture to `rx`.
struct Running {
    capture: MediaCapture,
    reader: MediaFrameReader,
    token: i64,
    mode: Mode,
    exclusive: bool,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.reader.RemoveFrameArrived(self.token);
        let _ = self.reader.StopAsync().and_then(|op| op.join());
        let _ = self.capture.Close();
    }
}

/// Starts the camera with control over its format; if another app has it,
/// shares it instead.
fn start(device: &Device, s: &CameraSettings, tx: SyncSender<Picture>) -> Result<Running> {
    let first = open(device, true).and_then(|capture| start_reader(capture, device, s, true, tx.clone()));
    match first {
        Ok(r) => Ok(r),
        Err(e) if format!("{e:#}").contains("Settings → Privacy") => Err(e),
        Err(e) => {
            info!("camera: {} is busy ({e:#}); sharing it with the app that has it", device.name);
            start_reader(open(device, false)?, device, s, false, tx)
        }
    }
}

/// Sends every picture the reader gets to `tx` (pictures the encoder has no
/// room for are left out).
fn on_picture(tx: SyncSender<Picture>) -> TypedEventHandler<MediaFrameReader, MediaFrameArrivedEventArgs> {
    TypedEventHandler::new(move |reader: windows::core::Ref<MediaFrameReader>, _| {
        if let Some(reader) = reader.as_ref()
            && let Ok(p) = take_picture(reader)
        {
            let _ = tx.try_send(p);
        }
        Ok(())
    })
}

/// Picks the format (with control), makes the NV12 frame reader and starts it.
fn start_reader(capture: MediaCapture, device: &Device, s: &CameraSettings, exclusive: bool, tx: SyncSender<Picture>) -> Result<Running> {
    let source = capture.FrameSources()?.Lookup(&device.source)?;
    let mut mode = mode_of(&source.CurrentFormat()?)?;
    if exclusive {
        let formats = source.SupportedFormats()?;
        let mut all = Vec::new();
        for i in 0..formats.Size()? {
            let f = formats.GetAt(i)?;
            if let Ok(m) = mode_of(&f) {
                all.push((f, m));
            }
        }
        let modes: Vec<Mode> = all.iter().map(|(_, m)| *m).collect();
        if let Some(i) = pick(&modes, s.height, s.fps) {
            source.SetFormatAsync(&all[i].0)?.join().map_err(|e| explain(e, &device.name))?;
            mode = all[i].1;
        }
    }
    // Taller than asked (a shared camera, or one with only big formats): scaled on the way.
    let nv12 = HSTRING::from("NV12");
    let reader = if mode.height > s.height {
        let height = s.height & !1;
        let width = ((mode.width as u64 * height as u64 / mode.height.max(1) as u64) as u32) & !1;
        mode = Mode { width, height, ..mode };
        capture.CreateFrameReaderWithSubtypeAndSizeAsync(&source, &nv12, BitmapSize { Width: width, Height: height })?.join()?
    } else {
        capture.CreateFrameReaderWithSubtypeAsync(&source, &nv12)?.join()?
    };
    reader.SetAcquisitionMode(MediaFrameReaderAcquisitionMode::Realtime)?;
    let token = reader.FrameArrived(&on_picture(tx))?;
    let running = Running { capture, reader, token, mode, exclusive };
    let status = running.reader.StartAsync()?.join()?;
    if status != MediaFrameReaderStartStatus::Success {
        let why = match status {
            MediaFrameReaderStartStatus::DeviceNotAvailable => "it isn't available (unplugged, or turned off)",
            MediaFrameReaderStartStatus::ExclusiveControlNotAvailable => "another app has it",
            MediaFrameReaderStartStatus::OutputFormatNotSupported => "it can't give pictures Klipp can use",
            _ => "it didn't start",
        };
        bail!("{}: {why}", device.name);
    }
    Ok(running)
}

fn run(s: &CameraSettings, vendor: Vendor, stream: u8, shared: &Shared, stop: &AtomicBool, last_picture: &AtomicI64) -> Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let device = find(&s.camera)?;
    write_status(&Status { state: "starting", camera: &device.name, width: 0, height: 0, fps: 0.0, shared: false, error: "" });
    let (tx, rx) = mpsc::sync_channel::<Picture>(QUEUE);
    let running = start(&device, s, tx)?;
    encode(&running, rx, s, vendor, stream, shared, stop, last_picture, &device.name)
}

#[allow(clippy::too_many_arguments)]
fn encode(
    running: &Running,
    rx: Receiver<Picture>,
    s: &CameraSettings,
    vendor: Vendor,
    stream: u8,
    shared: &Shared,
    stop: &AtomicBool,
    last_picture: &AtomicI64,
    name: &str,
) -> Result<()> {
    let (mode, exclusive) = (running.mode, running.exclusive);
    info!(
        "camera: {name} at {}x{} {:.0} fps{}{}",
        mode.width,
        mode.height,
        mode.fps,
        if mode.compressed { " (MJPEG)" } else { "" },
        if exclusive { "" } else { ", shared with another app" }
    );

    let fps = (mode.fps.round() as u32).clamp(1, 120);
    let t0 = clock::now();
    let mut encoder: Option<(CameraEncoder, usize, usize)> = None;
    let mut timing = Timing::default();
    let mut last_pts = -1;
    let mut odd_size_told = false;
    let mut sink = |p: &mut Packet| -> Result<()> {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let info = PacketInfo {
            stream,
            time: t0 + p.pts() * clock::SECOND / CAMERA_CLOCK,
            pts: p.pts(),
            dts: p.dts(),
            duration: CAMERA_CLOCK / fps as i64,
            key: p.is_key(),
        };
        shared.buffer.lock().unwrap().push(info, p.data());
        Ok(())
    };
    let result = (|| -> Result<()> {
        while !stop.load(Ordering::Relaxed) {
            let p = match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(p) => p,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => bail!("{name} stopped sending pictures"),
            };
            last_picture.store(p.arrived, Ordering::Relaxed);
            if encoder.is_none() {
                let enc = CameraEncoder::new(vendor, p.width as u32, p.height as u32, fps, s.max_bitrate())?;
                *shared.camera.lock().unwrap() = Some((stream, enc.stream_info()?.with_title("Camera")));
                write_status(&Status {
                    state: "recording",
                    camera: name,
                    width: p.width as u32,
                    height: p.height as u32,
                    fps: mode.fps,
                    shared: !exclusive,
                    error: "",
                });
                encoder = Some((enc, p.width, p.height));
            }
            let Some((enc, w, h)) = encoder.as_mut() else { continue };
            // The picture size can't change within one encoder; it restarts instead.
            if (p.width, p.height) != (*w, *h) {
                if !odd_size_told {
                    warning!("camera: {name}'s pictures changed size to {}x{}; starting it again", p.width, p.height);
                    odd_size_told = true;
                }
                bail!("{name}'s picture size changed");
            }
            let time = timing.place(p.stamp, p.arrived);
            let pts = ((time - t0) * CAMERA_CLOCK / clock::SECOND).max(last_pts + 1);
            last_pts = pts;
            let (y, uv) = p.nv12.split_at(p.width * p.height);
            enc.encode(y, uv, pts, &mut sink)?;
        }
        Ok(())
    })();
    if let Some((mut enc, ..)) = encoder {
        let _ = enc.finish(&mut sink);
    }
    result
}

/// The newest picture from the reader, copied out as NV12 without padding.
fn take_picture(reader: &MediaFrameReader) -> Result<Picture> {
    let arrived = clock::now();
    let frame = reader.TryAcquireLatestFrame()?;
    let stamp = frame.SystemRelativeTime().and_then(|r| r.Value()).map(|t| t.Duration).ok();
    let bitmap = frame.VideoMediaFrame()?.SoftwareBitmap()?;
    if bitmap.BitmapPixelFormat()? != BitmapPixelFormat::Nv12 {
        bail!("the picture isn't NV12");
    }
    let (w, h) = (bitmap.PixelWidth()? as usize, bitmap.PixelHeight()? as usize);
    let buffer = bitmap.LockBuffer(BitmapBufferAccessMode::Read)?;
    let (y, uv) = (buffer.GetPlaneDescription(0)?, buffer.GetPlaneDescription(1)?);
    let reference = buffer.CreateReference()?;
    let access: IMemoryBufferByteAccess = reference.cast()?;
    let (mut data, mut capacity) = (std::ptr::null_mut(), 0u32);
    unsafe { access.GetBuffer(&mut data, &mut capacity)? };
    if data.is_null() {
        bail!("the picture has no pixels");
    }
    // SAFETY: the buffer stays locked (and `reference` alive) until after the copy.
    let bytes = unsafe { std::slice::from_raw_parts(data, capacity as usize) };
    let mut nv12 = Vec::with_capacity(w * h * 3 / 2);
    copy_plane(bytes, y.StartIndex as usize, y.Stride as usize, w, h, &mut nv12)?;
    copy_plane(bytes, uv.StartIndex as usize, uv.Stride as usize, w, h / 2, &mut nv12)?;
    drop(access);
    let _ = reference.Close();
    let _ = buffer.Close();
    let _ = frame.Close();
    Ok(Picture { stamp, arrived, width: w, height: h, nv12 })
}

/// Appends `rows` rows of `width` bytes, `stride` apart from `start`.
fn copy_plane(bytes: &[u8], start: usize, stride: usize, width: usize, rows: usize, out: &mut Vec<u8>) -> Result<()> {
    if rows > 0 && start + (rows - 1) * stride + width > bytes.len() {
        bail!("the picture's buffer is smaller than its size says");
    }
    for row in 0..rows {
        let at = start + row * stride;
        out.extend_from_slice(&bytes[at..at + width]);
    }
    Ok(())
}

/// `--probe-camera SECONDS [NAME]`: opens the camera like recording would,
/// without encoding, and reports its format and how its pictures are timed.
pub fn probe(seconds: u32, name: &str) -> Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    for d in devices()? {
        println!("camera: {}", d.name);
    }
    let device = find(name)?;
    let (tx, rx) = mpsc::sync_channel::<Picture>(64);
    let running = start(&device, &CameraSettings::default(), tx)?;
    let mode = running.mode;
    println!("{}: {}x{} {:.2} fps, compressed {}, exclusive {}", device.name, mode.width, mode.height, mode.fps, mode.compressed, running.exclusive);
    let end = Instant::now() + Duration::from_secs(seconds as u64);
    let (mut n, mut stamped, mut lags) = (0u32, 0u32, Vec::new());
    let mut size = (0, 0);
    while Instant::now() < end {
        let Ok(p) = rx.recv_timeout(Duration::from_millis(500)) else { continue };
        n += 1;
        size = (p.width, p.height);
        if let Some(t) = p.stamp {
            stamped += 1;
            lags.push((p.arrived - t) as f64 / 10_000.0);
        }
    }
    drop(running);
    lags.sort_by(f64::total_cmp);
    let pct = |q: f64| lags.get(((lags.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(f64::NAN);
    println!(
        "pictures {n} ({:.1} fps), {}x{}, stamped {stamped} | arrival minus capture time, ms: min {:.1}, median {:.1}, max {:.1}",
        n as f64 / seconds as f64,
        size.0,
        size.1,
        pct(0.0),
        pct(0.5),
        pct(1.0)
    );
    Ok(())
}
