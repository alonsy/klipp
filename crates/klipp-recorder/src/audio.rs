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

use crate::cancel::Canceller;
use crate::clock;
use crate::log::{info, warning};
use crate::process_loopback::Target;
use crate::replay::PacketInfo;
use crate::state::Shared;
use crate::timeline::Timeline;
use crate::wasapi::{Capturer, SourceKind};

/// Which programs a track hears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Programs {
    /// The device itself: everything it plays, or what the microphone hears.
    Device,
    /// Everything but Klipp's own sounds: the save sound plays on this device.
    AllButKlipp,
    /// Everything but the Discord app, which has its own track, and with
    /// Klipp's own sounds taken out (see `cancel`).
    AllButDiscord,
    /// Only the Discord app (experimental).
    Discord,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSourceConfig {
    /// Track name in the clip, e.g. "Game".
    pub title: String,
    /// Part of the device's name, e.g. "Sonar - Gaming".
    pub device: String,
    pub kind: SourceKind,
    pub programs: Programs,
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
/// How often to look for Discord starting, quitting or restarting itself.
const DISCORD_EVERY: Duration = Duration::from_secs(2);

/// What a source records from, once opened.
pub struct Opened {
    pub capturer: Capturer,
    /// Klipp's own sound, to take back out (`Programs::AllButDiscord` while Discord runs).
    pub own: Option<Capturer>,
}

/// An audio router replays other programs' sound from its own process, so
/// leaving programs out of a capture doesn't work while one is installed.
fn router_installed() -> bool {
    let devices = crate::wasapi::list_devices().unwrap_or_default();
    let playback: Vec<String> = devices.into_iter().filter(|d| d.kind == SourceKind::Loopback).map(|d| d.name).collect();
    crate::process_loopback::router_present(&playback)
}

/// Opens what `cfg` records, with `discord` the Discord app's main process if
/// it runs. `Ok(None)`: nothing to record from now (no such device, or
/// Discord isn't running). Logs what it picked unless `quiet`.
pub fn open(cfg: &AudioSourceConfig, discord: Option<u32>, quiet: bool) -> Result<Option<Opened>> {
    let title = &cfg.title;
    let opened = |capturer: Capturer| Ok(Some(Opened { capturer, own: None }));
    match cfg.programs {
        Programs::Device => {}
        Programs::AllButKlipp => {
            if router_installed() {
                if !quiet {
                    info!("audio: {title}: an audio router is installed, so the save sound should play on another device");
                }
            } else {
                match Capturer::open_excluding_klipp() {
                    Ok(c) => {
                        info!("audio: {title} <- everything this PC plays, except Klipp's own sounds");
                        return opened(c);
                    }
                    Err(e) => warning!("audio: can't leave Klipp's sounds out ({e:#}); recording the device as is"),
                }
            }
        }
        Programs::AllButDiscord => {
            if router_installed() {
                if !quiet {
                    warning!("audio: {title}: Discord can't have its own track while an audio mixer (Sonar, Voicemeeter…) is installed; recording the device, Discord included");
                }
            } else {
                let me = std::process::id();
                let both = discord.map(|pid| -> Result<Opened> {
                    let capturer = Capturer::open_programs(Target::AllBut(pid), "everything but Discord")?;
                    let own = Capturer::open_programs(Target::Only(me), "Klipp")?;
                    Ok(Opened { capturer, own: Some(own) })
                });
                match both {
                    Some(Ok(o)) => {
                        info!("audio: {title} <- everything this PC plays, except Discord and Klipp's own sounds");
                        return Ok(Some(o));
                    }
                    Some(Err(e)) => warning!("audio: can't leave Discord out ({e:#}); recording the device, Discord included"),
                    None => match Capturer::open_excluding_klipp() {
                        Ok(c) => {
                            info!("audio: {title} <- everything this PC plays, except Klipp's own sounds (Discord isn't running)");
                            return opened(c);
                        }
                        Err(e) => warning!("audio: can't leave Klipp's sounds out ({e:#}); recording the device as is"),
                    },
                }
            }
        }
        Programs::Discord => {
            // Never a device: without Discord (or with a router), the track stays silent.
            if router_installed() {
                if !quiet {
                    warning!("audio: {title}: Discord can't have its own track while an audio mixer (Sonar, Voicemeeter…) is installed; this track stays silent");
                }
                return Ok(None);
            }
            let Some(pid) = discord else {
                if !quiet {
                    info!("audio: {title}: Discord isn't running; silent until it starts");
                }
                return Ok(None);
            };
            let c = Capturer::open_programs(Target::Only(pid), "Discord")?;
            info!("audio: {title} <- Discord");
            return opened(c);
        }
    }
    let Some(c) = Capturer::open(&cfg.device, cfg.kind)? else {
        if !quiet {
            warning!("audio: no device matching \"{}\" for {title}; will keep looking", cfg.device);
        }
        return Ok(None);
    };
    info!("audio: {title} <- {}", c.device_name);
    opened(c)
}

struct Source {
    cfg: AudioSourceConfig,
    capturer: Option<Capturer>,
    /// Klipp's own sound and what takes it back out (see `cancel`).
    own: Option<(Capturer, Canceller)>,
    /// The Discord process the capture was opened for.
    discord: Option<u32>,
    next_retry: Instant,
    timeline: Timeline,
    encoder: AudioEncoder,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Source {
    fn follows_discord(&self) -> bool {
        matches!(self.cfg.programs, Programs::AllButDiscord | Programs::Discord)
    }

    fn try_open(&mut self, quiet: bool, discord: Option<u32>) {
        (self.capturer, self.own, self.discord) = (None, None, discord);
        match open(&self.cfg, discord, quiet) {
            Ok(Some(o)) => {
                self.capturer = Some(o.capturer);
                self.own = o.own.map(|c| (c, Canceller::new(TIMELINE_FRAMES)));
            }
            Ok(None) => {}
            Err(e) if !quiet => warning!("audio: opening {} failed: {e:#}; will retry", self.cfg.title),
            Err(_) => {}
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
    let follow_discord = configs.iter().any(|c| matches!(c.programs, Programs::AllButDiscord | Programs::Discord));
    let mut discord = if follow_discord { crate::discord::main_process() } else { None };
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
                own: None,
                discord: None,
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
            s.try_open(false, discord);
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
    let mut next_discord_check = Instant::now() + DISCORD_EVERY;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(POLL_EVERY);

        // Discord started, quit or restarted itself (it does for updates).
        if follow_discord && Instant::now() >= next_discord_check {
            next_discord_check = Instant::now() + DISCORD_EVERY;
            let now = crate::discord::main_process();
            if now != discord {
                discord = now;
                for s in sources.iter_mut().filter(|s| s.follows_discord()) {
                    s.try_open(false, discord);
                }
            }
        }

        for s in &mut sources {
            if s.capturer.is_none() && Instant::now() >= s.next_retry {
                s.try_open(true, discord);
            }
            let Source { capturer, own, timeline, cfg, next_retry, .. } = s;
            if let Some(c) = capturer {
                let result = c.read(|time, samples, frames| timeline.write(to_frame(time), samples, frames, mixed));
                if let Err(e) = result {
                    warning!("audio: lost {} ({e:#}); will retry", cfg.title);
                    (*capturer, *own) = (None, None);
                    *next_retry = Instant::now() + RETRY_EVERY;
                }
            }
            if let Some((c, cancel)) = own {
                if let Err(e) = c.read(|time, samples, frames| cancel.write(to_frame(time), samples, frames, mixed)) {
                    warning!("audio: lost Klipp's own sound for {} ({e:#}); the save sound can be heard in it until it's opened again", cfg.title);
                    *own = None;
                }
            }
        }

        // Mix and encode every complete frame that's old enough.
        let ready_until = to_frame(clock::now()) - LATENCY_FRAMES;
        while mixed + frame as i64 <= ready_until {
            mix_l.fill(0.0);
            mix_r.fill(0.0);
            for s in &mut sources {
                let Source { timeline, own, left, right, .. } = s;
                timeline.take(mixed, left, right);
                if let Some((_, cancel)) = own {
                    cancel.apply(mixed, left, right, timeline);
                }
                for i in 0..frame {
                    mix_l[i] += left[i];
                    mix_r[i] += right[i];
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
