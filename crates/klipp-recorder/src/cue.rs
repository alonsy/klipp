//! The sound played when a clip is saved (or fails to save).
//!
//! It plays on a device we don't record, by default the headset itself rather
//! than Sonar's Gaming channel, so the chime never ends up in the next clip.
//! A new sound cuts off the one playing and starts over, so pressing the
//! hotkey quickly never queues up a long string of chimes.

use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoInitializeEx};

use crate::log::warning;
use crate::settings;
use crate::wasapi::{self, SourceKind};

#[derive(Clone, Copy, Debug)]
pub enum Sound {
    /// A clip saved with the hotkey or tray menu.
    Saved,
    /// A clip saved by the auto-clip engine.
    AutoSaved,
    Error,
}

/// Decoded audio, interleaved f32.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub rate: u32,
}

/// Cheap to clone; every clone plays through the same player thread.
#[derive(Clone)]
pub struct CueHandle {
    tx: Sender<Sound>,
    settings: Arc<Mutex<settings::Cue>>,
}

impl CueHandle {
    pub fn play(&self, sound: Sound) {
        let _ = self.tx.send(sound);
    }

    pub fn update(&self, cfg: &settings::Cue) {
        *self.settings.lock().unwrap() = cfg.clone();
    }
}

pub struct CuePlayer {
    handle: Option<CueHandle>,
    thread: Option<JoinHandle<()>>,
}

impl CuePlayer {
    pub fn start(cfg: &settings::Cue) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let settings = Arc::new(Mutex::new(cfg.clone()));
        let shared = settings.clone();
        let thread = thread::Builder::new().name("cue".into()).spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let (chime, error) = (builtin(SAVED_WAV), builtin(ERROR_WAV));
            // Custom WAVs by path, loaded on first use.
            let mut custom: Vec<(String, Option<Clip>)> = Vec::new();
            let mut next: Option<Sound> = None;
            while let Some(mut sound) = next.take().or_else(|| rx.recv().ok()) {
                // Only the newest request counts.
                while let Ok(s) = rx.try_recv() {
                    sound = s;
                }
                let cfg = shared.lock().unwrap().clone();
                if !cfg.enabled || (matches!(sound, Sound::AutoSaved) && !cfg.auto_clips) {
                    continue;
                }
                let wav = match sound {
                    Sound::AutoSaved if !cfg.auto_sound.is_empty() => cfg.auto_sound.as_str(),
                    Sound::Saved | Sound::AutoSaved => cfg.sound.as_str(),
                    Sound::Error => "",
                };
                let clip = if wav.is_empty() {
                    if matches!(sound, Sound::Error) { &error } else { &chime }
                } else {
                    if !custom.iter().any(|(p, _)| p == wav) {
                        let loaded = load_wav(Path::new(wav))
                            .inspect_err(|e| warning!("cue: can't use {wav}: {e:#}; playing the built-in sound"))
                            .ok();
                        custom.push((wav.to_string(), loaded));
                    }
                    custom.iter().find(|(p, _)| p == wav).and_then(|(_, c)| c.as_ref()).unwrap_or(&chime)
                };
                match play(clip, &cfg.device, cfg.volume, &rx) {
                    Ok(interrupted) => next = interrupted,
                    Err(e) => warning!("cue: playing failed: {e:#}"),
                }
            }
        })?;
        Ok(Self { handle: Some(CueHandle { tx, settings }), thread: Some(thread) })
    }

    pub fn handle(&self) -> CueHandle {
        self.handle.clone().expect("cue player is running")
    }
}

impl Drop for CuePlayer {
    /// The thread ends once every handle (including clones held elsewhere) is gone.
    fn drop(&mut self) {
        self.handle = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The built-in sounds, compiled into the program (assets/sounds).
const SAVED_WAV: &[u8] = include_bytes!("../../../assets/sounds/recorded.wav");
const ERROR_WAV: &[u8] = include_bytes!("../../../assets/sounds/error.wav");

fn builtin(wav: &[u8]) -> Clip {
    parse_wav(wav).expect("built-in sounds are valid WAVs (checked by a test)")
}

/// Reads a PCM (16/24/32-bit) or float WAV file, mono or stereo, up to 5 s.
pub fn load_wav(path: &Path) -> Result<Clip> {
    parse_wav(&fs::read(path)?)
}

fn parse_wav(data: &[u8]) -> Result<Clip> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        bail!("not a WAV file");
    }
    let (mut fmt, mut pcm) = (None, None);
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into()?) as usize;
        let body = data.get(pos + 8..pos + 8 + size).context("truncated WAV chunk")?;
        match id {
            b"fmt " => fmt = Some(body),
            b"data" => pcm = Some(body),
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    let fmt = fmt.context("WAV has no fmt chunk")?;
    let pcm = pcm.context("WAV has no data chunk")?;
    if fmt.len() < 16 {
        bail!("WAV fmt chunk too short");
    }
    let u16_at = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
    let mut tag = u16_at(0);
    let channels = u16_at(2);
    let rate = u32::from_le_bytes(fmt[4..8].try_into()?);
    let bits = u16_at(14);
    // WAVE_FORMAT_EXTENSIBLE keeps the real format in the first bytes of its subformat GUID.
    if tag == 0xFFFE && fmt.len() >= 26 {
        tag = u16_at(24);
    }
    if !(1..=2).contains(&channels) {
        bail!("only mono and stereo WAVs are supported");
    }
    let samples: Vec<f32> = match (tag, bits) {
        (1, 16) => pcm.as_chunks::<2>().0.iter().map(|&b| i16::from_le_bytes(b) as f32 / 32768.0).collect(),
        (1, 24) => pcm.as_chunks::<3>().0.iter().map(|b| (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0).collect(),
        (1, 32) => pcm.as_chunks::<4>().0.iter().map(|&b| i32::from_le_bytes(b) as f32 / 2_147_483_648.0).collect(),
        (3, 32) => pcm.as_chunks::<4>().0.iter().map(|&b| f32::from_le_bytes(b)).collect(),
        _ => bail!("unsupported WAV format (tag {tag}, {bits}-bit); use 16/24-bit PCM or 32-bit float"),
    };
    if samples.len() > 5 * rate as usize * channels as usize {
        bail!("WAV is longer than 5 seconds");
    }
    Ok(Clip { samples, channels, rate })
}

/// Plays `clip`, or stops early and returns the next sound if one is requested meanwhile.
fn play(clip: &Clip, device_name: &str, volume: f32, requests: &Receiver<Sound>) -> Result<Option<Sound>> {
    let device = match wasapi::find_device(device_name, SourceKind::Loopback)? {
        Some(d) if !device_name.is_empty() => d,
        _ => {
            if !device_name.is_empty() {
                warning!("cue: no playback device matching \"{device_name}\"; using the default device");
            }
            wasapi::default_device(SourceKind::Loopback)?
        }
    };
    unsafe {
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let block = clip.channels as u32 * 4;
        let format = WAVEFORMATEX {
            wFormatTag: 3, // IEEE float
            nChannels: clip.channels,
            nSamplesPerSec: clip.rate,
            nAvgBytesPerSec: clip.rate * block,
            nBlockAlign: block as u16,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 5_000_000, 0, &format, None)?;
        let render: IAudioRenderClient = client.GetService()?;
        let capacity = client.GetBufferSize()?;
        let channels = clip.channels as usize;
        let total = clip.samples.len() / channels;
        let mut written = 0;
        let mut started = false;
        let deadline = Instant::now() + Duration::from_secs(8);
        while written < total && Instant::now() < deadline {
            if let Ok(next) = requests.try_recv() {
                client.Stop()?;
                return Ok(Some(next));
            }
            let free = (capacity - client.GetCurrentPadding()?) as usize;
            let n = free.min(total - written);
            if n > 0 {
                let out = std::slice::from_raw_parts_mut(render.GetBuffer(n as u32)? as *mut f32, n * channels);
                for (o, s) in out.iter_mut().zip(&clip.samples[written * channels..]) {
                    *o = s * volume;
                }
                render.ReleaseBuffer(n as u32, 0)?;
                written += n;
            }
            if !started {
                client.Start()?;
                started = true;
            }
            if written < total {
                thread::sleep(Duration::from_millis(10));
            }
        }
        // Let it finish playing before releasing the device.
        while client.GetCurrentPadding()? > 0 && Instant::now() < deadline {
            if let Ok(next) = requests.try_recv() {
                client.Stop()?;
                return Ok(Some(next));
            }
            thread::sleep(Duration::from_millis(10));
        }
        client.Stop()?;
    }
    Ok(None)
}
