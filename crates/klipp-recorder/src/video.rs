//! The video thread: capture → GPU convert → encode → replay buffer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use klipp_media::{HwDevice, HwFrame, Packet, VideoCodec, VideoEncoder, VideoEncoderConfig};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

use crate::capture::{Capture, MonitorCapture};
use crate::clock;
use crate::duplication::{self, DuplicationCapture};
use crate::gpu::{Converter, Gpu};
use crate::log::{info, warning};
use crate::replay::{PacketInfo, VIDEO};
use crate::revive::{Reviver, Why};
use crate::settings::CaptureMethod;
use crate::state::Shared;

#[derive(Clone, Debug)]
pub struct VideoSettings {
    pub codec: VideoCodec,
    pub fps: u32,
    /// Output height; `None` keeps the monitor's resolution.
    pub height: Option<u32>,
    pub quality: u8,
    pub max_mbps: u32,
    /// The monitor to record ("" = the main display; see `monitors`).
    pub monitor: String,
    pub capture: CaptureMethod,
    /// Encode on the card whose name contains this instead ("" = the screen's).
    pub gpu: String,
    /// The encoder's fastest preset (Performance+).
    pub light_encoder: bool,
}

/// After a stall longer than this (sleep, hibernate, debugger), skip ahead
/// instead of encoding a burst of catch-up frames.
const MAX_CATCH_UP: Duration = Duration::from_secs(1);
/// How often the video thread checks that the capture is still alive.
const REVIVE_CHECK_EVERY: Duration = Duration::from_millis(250);
/// A still picture this soon after skipping ticks means the GPU is too busy
/// to deliver frames, not that the capture stopped (FIX-17).
const BEHIND_GRACE: Duration = Duration::from_secs(10);
/// Longest wait for the video thread to start, or to stop. A thread stuck in
/// the graphics driver is left behind instead of freezing the caller (FIX-20).
const START_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
/// How often the graphics memory use and allowance are read, for the stats line.
const MEMORY_CHECK_EVERY: Duration = Duration::from_secs(5);

pub struct VideoPipeline {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl VideoPipeline {
    /// Starts capturing the primary monitor. Returns once frames are flowing
    /// into the buffer, or with the setup error.
    pub fn start(settings: VideoSettings, shared: Arc<Shared>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new().name("video".into()).spawn({
            let stop = stop.clone();
            move || {
                if let Err(e) = run(&settings, shared, stop, ready_tx) {
                    crate::log::warning!("video pipeline stopped: {e:#}");
                }
            }
        })?;
        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(Self { stop, thread: Some(thread) }),
            Ok(Err(e)) => Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Stuck in the driver: it stops on its own if it ever gets going.
                stop.store(true, Ordering::Relaxed);
                Err(anyhow!("the video didn't start within {} s", START_TIMEOUT.as_secs()))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!("video thread exited during setup")),
        }
    }
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let Some(t) = self.thread.take() else { return };
        // The caller is the main thread (tray, save key, watchdog): a video
        // thread stuck in the graphics driver must not take it down too. Left
        // behind, it writes nothing more and ends if it ever wakes up.
        let deadline = Instant::now() + STOP_TIMEOUT;
        while !t.is_finished() {
            if Instant::now() >= deadline {
                warning!("video: the old video thread is stuck (in the graphics driver?); leaving it behind");
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = t.join();
    }
}

/// Output size for an optional target height, keeping the aspect ratio and even dimensions.
fn output_size(src_w: u32, src_h: u32, height: Option<u32>) -> (u32, u32) {
    match height {
        Some(h) if h < src_h => {
            let w = (src_w as u64 * h as u64 / src_h as u64) as u32;
            (w & !1, h & !1)
        }
        _ => (src_w & !1, src_h & !1),
    }
}

/// Starts the capture method the settings ask for. On Windows 10, "auto" and
/// "light" mean Desktop Duplication (no yellow border; it already takes only
/// as many pictures as are recorded); if that can't start, it falls back to
/// Windows Graphics Capture.
fn start_capture(gpu: &Gpu, monitor: HMONITOR, s: &VideoSettings) -> Result<Capture> {
    let duplicate = match s.capture {
        CaptureMethod::Duplication => true,
        CaptureMethod::Wgc => false,
        CaptureMethod::Auto | CaptureMethod::Light => duplication::border_cannot_be_hidden(),
    };
    if duplicate {
        match DuplicationCapture::start(gpu, monitor, s.fps) {
            Ok(c) => return Ok(Capture::Duplication(c)),
            Err(e) => warning!("video: desktop duplication unavailable ({e:#}); using Windows Graphics Capture"),
        }
    }
    let light = s.capture == CaptureMethod::Light;
    let capture = MonitorCapture::start(gpu, monitor, s.fps, light)?;
    Ok(if light { Capture::Light(capture) } else { Capture::Wgc(capture) })
}

/// A new capture of the screen the settings name, looked up again: a monitor
/// that went away and came back is a new monitor to Windows.
fn restart_capture(gpu: &Gpu, s: &VideoSettings) -> Result<Capture> {
    let (monitor, _) = crate::monitors::find(&s.monitor);
    start_capture(gpu, monitor, s)
}

/// How long ago the last mouse or keyboard input was, anywhere in this session.
fn input_age() -> Option<Duration> {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe { GetLastInputInfo(&mut info) }
        .as_bool()
        .then(|| Duration::from_millis(unsafe { GetTickCount() }.wrapping_sub(info.dwTime) as u64))
}

fn run(s: &VideoSettings, shared: Arc<Shared>, stop: Arc<AtomicBool>, ready: SyncSender<Result<()>>) -> Result<()> {
    let _priority = crate::mmcss::CaptureThread::register();
    let setup = || -> Result<_> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        let (monitor, chosen) = crate::monitors::find(&s.monitor);
        let gpu = if s.gpu.is_empty() { Gpu::for_monitor(monitor)? } else { Gpu::by_name(&s.gpu)? };
        let vendor = gpu.vendor.with_context(|| format!("{} has no video encoder Klipp can use", gpu.adapter_name))?;
        let capture = start_capture(&gpu, monitor, s)?;
        // The output size is fixed for the life of the encoder. If the display
        // mode changes later, the converter scales the new size to fit it.
        let (converter, (w, h), size_generation) = {
            let latest = capture.latest().lock();
            let out = output_size(latest.width, latest.height, s.height);
            let converter = Converter::new(&gpu, (latest.width, latest.height), out, s.fps)?;
            (converter, out, latest.size_generation)
        };
        let device = HwDevice::from_d3d11(&gpu.device, &gpu.context)?;
        let config = |codec| VideoEncoderConfig {
            vendor,
            codec,
            width: w,
            height: h,
            fps: s.fps,
            quality: s.quality,
            max_bitrate: s.max_mbps * 1_000_000,
            light: s.light_encoder,
        };
        // Older cards can't encode AV1 (it needs a GeForce RTX 40, Radeon RX 7000,
        // Intel Arc or newer); they still record, in H.264.
        let (encoder, codec) = match VideoEncoder::new(&device, &config(s.codec)) {
            Ok(e) => (e, s.codec),
            Err(e) if s.codec == VideoCodec::Av1 => {
                warning!("video: {} can't encode AV1 ({e:#}); recording H.264 instead", gpu.adapter_name);
                (VideoEncoder::new(&device, &config(VideoCodec::H264))?, VideoCodec::H264)
            }
            Err(e) => return Err(e),
        };
        *shared.video_stream.lock().unwrap() = Some(encoder.stream_info()?);
        shared.bitrate_adjustable.store(vendor != klipp_media::Vendor::Amd, Ordering::Relaxed);

        // Windows sends a first frame right after capture starts.
        let waited = Instant::now();
        while capture.latest().generation() == 0 {
            if waited.elapsed() > Duration::from_secs(3) {
                bail!("no frame captured within 3 s");
            }
            thread::sleep(Duration::from_millis(5));
        }
        let method = match capture {
            Capture::Wgc(_) => "Windows Graphics Capture",
            Capture::Light(_) => "Windows Graphics Capture, light (no copy)",
            Capture::Duplication(_) => "Desktop Duplication",
        };
        let screen = chosen.map_or("the main display".to_string(), |m| m.name);
        let scheduling = match gpu.hardware_scheduling() {
            Some((_, true)) => "hardware GPU scheduling on",
            Some((true, false)) => "hardware GPU scheduling off",
            Some((false, _)) => "no hardware GPU scheduling",
            None => "GPU scheduling unknown",
        };
        info!(
            "video: {} ({}{}, {scheduling}) | {screen} via {method} | encoding {}x{} {:?} @ {} fps",
            gpu.adapter_name,
            vendor.encoder(),
            if s.light_encoder { ", fastest preset" } else { "" },
            w,
            h,
            codec,
            s.fps
        );
        Ok((gpu, capture, converter, size_generation, (w, h), device, encoder))
    };
    // The encoder and its device go to the encoder's thread.
    let (gpu, mut capture, mut converter, mut converter_generation, out_size, device, encoder) = match setup() {
        Ok(parts) => {
            let _ = ready.send(Ok(()));
            parts
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return Ok(());
        }
    };

    let fps = s.fps as i64;
    let max_bitrate = s.max_mbps * 1_000_000;
    let t0 = clock::now();
    let start = Instant::now();
    let pool = encoder.frame_pool()?;
    let (jobs, queue) = mpsc::sync_channel::<Job>(QUEUE);
    let encoding = thread::Builder::new().name("video-encode".into()).spawn({
        let (shared, stop) = (shared.clone(), stop.clone());
        move || {
            let buffer = shared.clone();
            let sink = move |p: &mut Packet| -> Result<()> {
                // A thread that was left behind (stuck, then woke up) must not mix its
                // packets into the new video's buffer.
                if stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let info = PacketInfo {
                    stream: VIDEO,
                    time: t0 + p.pts() * clock::SECOND / fps,
                    pts: p.pts(),
                    dts: p.dts(),
                    duration: 1,
                    key: p.is_key(),
                };
                buffer.buffer.lock().unwrap().push(info, p.data());
                Ok(())
            };
            if let Err(e) = encode_frames(encoder, device, queue, &shared, max_bitrate, sink) {
                warning!("video: the encoder stopped: {e:#}");
            }
        }
    })?;
    let tick = |i: i64| start + Duration::from_nanos((i as u128 * 1_000_000_000 / fps as u128) as u64);

    // Constant frame rate: every tick encodes the newest capture, or repeats
    // the previous frame if the screen didn't change.
    let mut last_generation = 0;
    // The capture's picture count, for the stats line's "captured".
    let mut seen_generation = 0;
    let mut capture_size = {
        let latest = capture.latest().lock();
        (latest.width, latest.height)
    };
    // A new capture has its own texture, so the converter is made again.
    let mut fresh_capture = false;
    let mut reviver = Reviver::new(Instant::now());
    let mut next_check = Instant::now() + REVIVE_CHECK_EVERY;
    // Size changes counted by captures that were replaced.
    let mut resizes_before = 0;
    // When ticks were last skipped (the GPU too busy to keep up).
    let mut last_skip: Option<Instant> = None;
    let mut next_memory = Instant::now();
    let mut i: i64 = 0;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let deadline = tick(i);
        if deadline > now {
            thread::sleep(deadline - now);
        } else if now - deadline > MAX_CATCH_UP {
            let behind = ((now - start).as_nanos() * fps as u128 / 1_000_000_000) as i64 - i;
            shared.skipped_ticks.fetch_add(behind as u64, Ordering::Relaxed);
            i += behind;
            last_skip = Some(now);
        } else if now - deadline > Duration::from_nanos(1_000_000_000 / fps as u64) {
            shared.late_frames.fetch_add(1, Ordering::Relaxed);
        }

        // A capture that has stopped for good is started again; until it
        // sends a picture, the last one repeats. While ticks are being
        // skipped, a still picture is the busy GPU, and a new capture can't help.
        if now >= next_check {
            next_check = now + REVIVE_CHECK_EVERY;
            let behind = last_skip.is_some_and(|t| now.duration_since(t) < BEHIND_GRACE);
            if let Some(why) = reviver.check(now, capture.latest().closed(), if behind { None } else { input_age() }) {
                let first = reviver.first_try();
                if first {
                    match why {
                        Why::Closed => info!("video: the screen went away; starting the capture again"),
                        Why::Still => info!("video: the picture stopped changing while the PC is in use; starting the capture again"),
                    }
                }
                match restart_capture(&gpu, s) {
                    Ok(c) => {
                        resizes_before += capture.latest().resizes();
                        capture = c;
                        last_generation = 0;
                        seen_generation = 0;
                        fresh_capture = true;
                        shared.capture_restarts.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) if first => warning!("video: the capture didn't start ({e:#}); trying again"),
                    Err(_) => {}
                }
            }
            let blind = reviver.blind();
            if shared.capture_blind.swap(blind, Ordering::Relaxed) != blind && blind {
                warning!("video: starting the capture again doesn't bring the picture back; clips show a still picture");
            }
        }

        if now >= next_memory {
            next_memory = now + MEMORY_CHECK_EVERY;
            if let Some((used, budget)) = gpu.memory() {
                shared.vram_used_mb.store(used, Ordering::Relaxed);
                shared.vram_budget_mb.store(budget, Ordering::Relaxed);
            }
        }

        let generation = capture.latest().generation();
        if generation > seen_generation {
            shared.captured.fetch_add(generation - seen_generation, Ordering::Relaxed);
            seen_generation = generation;
        }
        // Performance mode in a busy game: a new picture only every
        // `stride` ticks, the one before repeated in between (half or a
        // third of the frame rate in the clip, without restarting the video).
        let stride = shared.perf_stride.load(Ordering::Relaxed).max(1) as i64;
        let job = if generation != last_generation && i % stride == 0 {
            let waiting = Instant::now();
            let latest = capture.latest().lock();
            let wait = waiting.elapsed();
            if fresh_capture || latest.size_generation != converter_generation {
                let size = (latest.width, latest.height);
                if size != capture_size {
                    info!("video: capture size changed to {}x{}; scaling to {}x{}", size.0, size.1, out_size.0, out_size.1);
                }
                converter = Converter::new(&gpu, size, out_size, s.fps)?;
                converter_generation = latest.size_generation;
                capture_size = size;
                fresh_capture = false;
            }
            let frame = pool.alloc()?;
            let (texture, slice) = frame.texture();
            crate::gpu::keep_resident(texture);
            let converting = Instant::now();
            converter.convert(&latest.texture, texture, slice)?;
            shared.steps.converted(wait, converting.elapsed());
            drop(latest);
            last_generation = generation;
            shared.new_frames.fetch_add(1, Ordering::Relaxed);
            if reviver.picture(Instant::now()) {
                info!("video: the new capture works; the picture is back");
            }
            Job::New(frame, i)
        } else {
            Job::Repeat(i)
        };
        let new = matches!(job, Job::New(..));
        shared.steps.queued();
        match jobs.try_send(job) {
            Ok(()) => {}
            // The encoder is that far behind: this frame is left out (the
            // clip shows the one before a little longer), and it counts as
            // skipped, so the watchdog sees the card can't keep up.
            Err(TrySendError::Full(_)) => {
                shared.steps.dequeued();
                shared.steps.dropped();
                shared.skipped_ticks.fetch_add(1, Ordering::Relaxed);
                if new {
                    // Converted again next tick rather than repeating an older picture.
                    last_generation = 0;
                }
            }
            Err(TrySendError::Disconnected(_)) => bail!("the encoder stopped"),
        }
        shared.capture_resizes.store(resizes_before + capture.latest().resizes(), Ordering::Relaxed);
        i += 1;
    }
    // The encoder finishes what's queued and stops; one stuck in the driver is left behind.
    drop(jobs);
    let deadline = Instant::now() + STOP_TIMEOUT;
    while !encoding.is_finished() {
        if Instant::now() >= deadline {
            warning!("video: the encoder is stuck (in the graphics driver?); leaving it behind");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = encoding.join();
    Ok(())
}

/// Frames waiting for the encoder. The encoder takes a texture only once the
/// card has drawn it, and a game keeping the card busy makes that a wait of
/// about one of its frames; queued, it holds up only the encoder, not the
/// frame clock or the capture (FIX-17).
const QUEUE: usize = 8;

enum Job {
    /// A new picture, as frame number `pts`.
    New(HwFrame, i64),
    /// The screen didn't change: the last picture again, as frame `pts`.
    Repeat(i64),
}

/// The encoder's thread: encodes queued frames in order until the video loop
/// stops, at the bitrate ceiling Performance mode asks for (`max_bitrate`, the
/// setting, when it asks for none).
fn encode_frames(
    mut encoder: VideoEncoder,
    device: HwDevice,
    queue: mpsc::Receiver<Job>,
    shared: &Shared,
    max_bitrate: u32,
    mut sink: impl FnMut(&mut Packet) -> Result<()>,
) -> Result<()> {
    let _priority = crate::mmcss::CaptureThread::register();
    let mut last: Option<HwFrame> = None;
    let mut ceiling = max_bitrate;
    for job in queue {
        shared.steps.dequeued();
        let asked = match shared.perf_max_bitrate.load(Ordering::Relaxed) {
            0 => max_bitrate,
            b => b.min(max_bitrate),
        };
        if asked != ceiling {
            encoder.set_max_bitrate(asked);
            ceiling = asked;
        }
        let pts = match job {
            Job::New(frame, pts) => {
                last = Some(frame);
                pts
            }
            Job::Repeat(pts) => pts,
        };
        let Some(frame) = last.as_mut() else { continue };
        let sending = Instant::now();
        encoder.send(frame, pts)?;
        let sent = sending.elapsed();
        let receiving = Instant::now();
        encoder.drain(&mut sink)?;
        shared.steps.encoded(sent, receiving.elapsed());
        shared.frames.fetch_add(1, Ordering::Relaxed);
    }
    drop(last);
    encoder.finish(&mut sink)?;
    // The encoder goes before the device it was made on.
    drop(encoder);
    drop(device);
    Ok(())
}
