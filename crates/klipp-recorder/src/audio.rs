//! The audio thread: capture every source → line them up on one timeline →
//! encode a premixed track plus one track per source → replay buffer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use klipp_media::{AudioEncoder, Packet, SAMPLE_RATE};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

use crate::clock;
use crate::log::{info, warning};
use crate::replay::PacketInfo;
use crate::state::Shared;
use crate::timeline::Timeline;
use crate::wasapi::{Capturer, SourceKind};

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSourceConfig {
    /// Track name in the clip, e.g. "Game".
    pub title: String,
    /// Part of the device's name, e.g. "Sonar - Gaming".
    pub device: String,
    pub kind: SourceKind,
    /// The save sound plays on this same device: record everything but Klipp
    /// instead (`process_loopback`), unless an audio router is installed.
    pub skip_klipp: bool,
}

/// Buffer stream id of the premixed track; sources follow from 2.
pub const MIX_STREAM: u8 = 1;
const MIX_BITRATE: u32 = 192_000;
const TRACK_BITRATE: u32 = 160_000;
/// The mixer runs this far behind real time, so packets that arrive a bit
/// late still make it into the mix.
const LATENCY_FRAMES: i64 = SAMPLE_RATE as i64 / 10;
/// Each source's timeline holds this much audio.
const TIMELINE_FRAMES: usize = SAMPLE_RATE as usize * 2;
const POLL_EVERY: Duration = Duration::from_millis(10);
/// How often to look for a device that's missing (e.g. Sonar not running yet).
const RETRY_EVERY: Duration = Duration::from_secs(5);

struct Source {
    cfg: AudioSourceConfig,
    capturer: Option<Capturer>,
    next_retry: Instant,
    timeline: Timeline,
    encoder: AudioEncoder,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Source {
    fn try_open(&mut self, quiet: bool) {
        if self.cfg.skip_klipp {
            let devices = crate::wasapi::list_devices().unwrap_or_default();
            let playback: Vec<String> = devices.into_iter().filter(|d| d.kind == SourceKind::Loopback).map(|d| d.name).collect();
            if crate::process_loopback::router_present(&playback) {
                if !quiet {
                    info!("audio: {}: an audio router is installed, so the save sound should play on another device", self.cfg.title);
                }
            } else {
                match Capturer::open_excluding_klipp() {
                    Ok(c) => {
                        info!("audio: {} <- everything this PC plays, except Klipp's own sounds", self.cfg.title);
                        self.capturer = Some(c);
                        return;
                    }
                    Err(e) => warning!("audio: can't leave Klipp's sounds out ({e:#}); recording the device as is"),
                }
            }
        }
        match Capturer::open(&self.cfg.device, self.cfg.kind) {
            Ok(Some(c)) => {
                info!("audio: {} <- {}", self.cfg.title, c.device_name);
                self.capturer = Some(c);
            }
            Ok(None) if !quiet => warning!("audio: no device matching \"{}\" for {}; will keep looking", self.cfg.device, self.cfg.title),
            Err(e) if !quiet => warning!("audio: opening \"{}\" failed: {e:#}; will retry", self.cfg.device),
            _ => {}
        }
        self.next_retry = Instant::now() + RETRY_EVERY;
    }
}

pub struct AudioPipeline {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioPipeline {
    pub fn start(sources: Vec<AudioSourceConfig>, shared: Arc<Shared>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new().name("audio".into()).spawn({
            let stop = stop.clone();
            move || {
                if let Err(e) = run(sources, &shared, &stop, ready_tx) {
                    warning!("audio pipeline stopped: {e:#}");
                }
            }
        })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { stop, thread: Some(thread) }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(anyhow!("audio thread exited during setup")),
        }
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(configs: Vec<AudioSourceConfig>, shared: &Shared, stop: &AtomicBool, ready: SyncSender<Result<()>>) -> Result<()> {
    let _priority = crate::mmcss::CaptureThread::register();
    let setup = || -> Result<_> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        let mix = AudioEncoder::new(MIX_BITRATE)?;
        let mut sources = Vec::with_capacity(configs.len());
        for cfg in configs {
            let encoder = AudioEncoder::new(TRACK_BITRATE)?;
            let n = encoder.frame_size();
            sources.push(Source {
                cfg,
                capturer: None,
                next_retry: Instant::now(),
                timeline: Timeline::new(TIMELINE_FRAMES),
                encoder,
                left: vec![0.0; n],
                right: vec![0.0; n],
            });
        }
        let mut streams = vec![mix.stream_info()?.with_title("Mix").as_default()];
        for s in &sources {
            streams.push(s.encoder.stream_info()?.with_title(&s.cfg.title));
        }
        *shared.audio_streams.lock().unwrap() = streams;
        for s in &mut sources {
            s.try_open(false);
        }
        Ok((mix, sources))
    };
    let (mut mix, mut sources) = match setup() {
        Ok(parts) => {
            let _ = ready.send(Ok(()));
            parts
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return Ok(());
        }
    };

    let frame = mix.frame_size();
    let (mut mix_l, mut mix_r) = (vec![0.0f32; frame], vec![0.0f32; frame]);
    let t0 = clock::now();
    let to_frame = |time: i64| ((time - t0) as i128 * SAMPLE_RATE as i128 / clock::SECOND as i128) as i64;
    let sink = |stream: u8| {
        move |p: &mut Packet| -> Result<()> {
            let info = PacketInfo {
                stream,
                time: t0 + p.pts() * clock::SECOND / SAMPLE_RATE as i64,
                pts: p.pts(),
                dts: p.dts(),
                duration: frame as i64,
                key: true,
            };
            shared.buffer.lock().unwrap().push(info, p.data());
            Ok(())
        }
    };

    let mut mixed: i64 = 0;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(POLL_EVERY);

        for s in &mut sources {
            if s.capturer.is_none() && Instant::now() >= s.next_retry {
                s.try_open(true);
            }
            let Source { capturer, timeline, cfg, next_retry, .. } = s;
            if let Some(c) = capturer {
                let result = c.read(|time, samples, frames| timeline.write(to_frame(time), samples, frames, mixed));
                if let Err(e) = result {
                    warning!("audio: lost {} ({e:#}); will retry", cfg.title);
                    *capturer = None;
                    *next_retry = Instant::now() + RETRY_EVERY;
                }
            }
        }

        // Mix and encode every complete frame that's old enough.
        let ready_until = to_frame(clock::now()) - LATENCY_FRAMES;
        while mixed + frame as i64 <= ready_until {
            mix_l.fill(0.0);
            mix_r.fill(0.0);
            for s in &mut sources {
                s.timeline.take(mixed, &mut s.left, &mut s.right);
                for i in 0..frame {
                    mix_l[i] += s.left[i];
                    mix_r[i] += s.right[i];
                }
            }
            for x in mix_l.iter_mut().chain(mix_r.iter_mut()) {
                *x = x.clamp(-1.0, 1.0);
            }
            mix.encode(&mix_l, &mix_r, mixed, &mut sink(MIX_STREAM))?;
            for (k, s) in sources.iter_mut().enumerate() {
                s.encoder.encode(&s.left, &s.right, mixed, &mut sink(MIX_STREAM + 1 + k as u8))?;
            }
            mixed += frame as i64;
        }
    }

    mix.finish(&mut sink(MIX_STREAM))?;
    for (k, s) in sources.iter_mut().enumerate() {
        s.encoder.finish(&mut sink(MIX_STREAM + 1 + k as u8))?;
    }
    Ok(())
}
