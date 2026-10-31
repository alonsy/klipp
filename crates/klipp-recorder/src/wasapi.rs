//! WASAPI capture: what a playback device is playing (loopback) or what a
//! microphone hears, converted by Windows to 48 kHz stereo float.

use std::cell::Cell;
use std::slice;

use anyhow::Result;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};

use crate::clock;
use crate::log::warning;

/// Where a track's audio comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// What a playback device is playing, e.g. "SteelSeries Sonar - Gaming".
    Loopback,
    /// A recording device, e.g. a microphone.
    Capture,
}

pub struct DeviceInfo {
    pub name: String,
    pub kind: SourceKind,
    pub is_default: bool,
}

const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const CHANNELS: usize = 2;
/// WASAPI's own buffer. We read every 10 ms, so this leaves plenty of slack.
const BUFFER_100NS: i64 = 2_000_000;
/// No packet can be older than that buffer, or from the future: a device whose
/// times are further off than this keeps a clock of its own.
const CLOCK_TOLERANCE: i64 = clock::SECOND;
/// A device whose clock keeps moving is logged at most this often.
const CLOCK_LOG_EVERY: i64 = 600 * clock::SECOND;

/// Puts a device's packet times on the shared clock. Some drivers stamp their
/// packets with a clock of their own (SteelSeries Sonar's microphone was 102
/// hours off), and that sound landed outside the timeline as silence. Such a
/// device is timed by when its packets arrive, keeping the device's spacing.
#[derive(Clone, Copy, Default)]
struct DeviceClock {
    /// Added to the device's times; 0 while its clock is right.
    offset: i64,
    /// How often the offset was set, and until when that isn't logged again.
    moves: u32,
    quiet_until: i64,
}

impl DeviceClock {
    /// When a packet of `frames` stamped `stamp` began, read at `now`; and, to
    /// log, how far off the device's clock was and how often it moved so far.
    fn place(&mut self, stamp: i64, frames: usize, now: i64) -> (i64, Option<(i64, u32)>) {
        let time = stamp + self.offset;
        if (now - time).abs() <= CLOCK_TOLERANCE {
            return (time, None);
        }
        let arrived = now - frames as i64 * clock::SECOND / klipp_media::SAMPLE_RATE as i64;
        self.offset = arrived - stamp;
        self.moves += 1;
        if now < self.quiet_until {
            return (arrived, None);
        }
        self.quiet_until = now + CLOCK_LOG_EVERY;
        (arrived, Some((now - stamp, self.moves)))
    }
}

/// "102.5 hours" or "3.2 s".
fn span(t: i64) -> String {
    let s = t as f64 / clock::SECOND as f64;
    if s.abs() >= 3600.0 { format!("{:.1} hours", s / 3600.0) } else { format!("{s:.1} s") }
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    Ok(unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? })
}

fn flow(kind: SourceKind) -> EDataFlow {
    match kind {
        SourceKind::Loopback => eRender,
        SourceKind::Capture => eCapture,
    }
}

fn friendly_name(device: &IMMDevice) -> Result<String> {
    let store = unsafe { device.OpenPropertyStore(STGM_READ)? };
    Ok(unsafe { store.GetValue(&PKEY_Device_FriendlyName)? }.to_string())
}

fn device_id(device: &IMMDevice) -> Result<String> {
    unsafe {
        let id = device.GetId()?;
        let s = id.to_string();
        CoTaskMemFree(Some(id.0 as *const _));
        Ok(s?)
    }
}

/// All active playback (loopback) and recording devices.
pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let e = enumerator()?;
    let mut out = Vec::new();
    for kind in [SourceKind::Loopback, SourceKind::Capture] {
        let default_id = unsafe { e.GetDefaultAudioEndpoint(flow(kind), eConsole) }.ok().and_then(|d| device_id(&d).ok());
        let devices = unsafe { e.EnumAudioEndpoints(flow(kind), DEVICE_STATE_ACTIVE)? };
        for i in 0..unsafe { devices.GetCount()? } {
            let d = unsafe { devices.Item(i)? };
            let is_default = default_id.as_deref() == device_id(&d).ok().as_deref();
            out.push(DeviceInfo { name: friendly_name(&d)?, kind, is_default });
        }
    }
    Ok(out)
}

/// The device Windows uses by default for `kind` (playback or recording).
pub fn default_device(kind: SourceKind) -> Result<IMMDevice> {
    Ok(unsafe { enumerator()?.GetDefaultAudioEndpoint(flow(kind), eConsole)? })
}

/// First active device of `kind` whose name contains `name` (case-insensitive).
pub fn find_device(name: &str, kind: SourceKind) -> Result<Option<IMMDevice>> {
    let wanted = name.to_lowercase();
    let devices = unsafe { enumerator()?.EnumAudioEndpoints(flow(kind), DEVICE_STATE_ACTIVE)? };
    for i in 0..unsafe { devices.GetCount()? } {
        let d = unsafe { devices.Item(i)? };
        if friendly_name(&d)?.to_lowercase().contains(&wanted) {
            return Ok(Some(d));
        }
    }
    Ok(None)
}

pub struct Capturer {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    times: Cell<DeviceClock>,
    pub device_name: String,
}

impl Capturer {
    /// Opens the device whose name contains `name`, or Windows' default device
    /// if `name` is empty. `Ok(None)` if no such device is active.
    pub fn open(name: &str, kind: SourceKind) -> Result<Option<Self>> {
        let device = if name.is_empty() { default_device(kind).ok() } else { find_device(name, kind)? };
        let Some(device) = device else { return Ok(None) };
        let device_name = friendly_name(&device)?;
        unsafe {
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            let format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
                nChannels: CHANNELS as u16,
                nSamplesPerSec: klipp_media::SAMPLE_RATE,
                nAvgBytesPerSec: klipp_media::SAMPLE_RATE * 8,
                nBlockAlign: 8,
                wBitsPerSample: 32,
                cbSize: 0,
            };
            // Windows converts the device's own format (rate, channel count) to ours.
            let mut flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            if kind == SourceKind::Loopback {
                flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
            }
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER_100NS, 0, &format, None)?;
            let capture: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            Ok(Some(Self { client, capture, times: Cell::default(), device_name }))
        }
    }

    /// Everything this PC plays except Klipp's own sounds (see `process_loopback`).
    pub fn open_excluding_klipp() -> Result<Self> {
        Self::open_programs(crate::process_loopback::Target::AllBut(std::process::id()), "everything but Klipp")
    }

    /// Only some programs' sound, or everything but some (see `process_loopback`).
    pub fn open_programs(target: crate::process_loopback::Target, name: &str) -> Result<Self> {
        let client = crate::process_loopback::activate(target)?;
        unsafe {
            let format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
                nChannels: CHANNELS as u16,
                nSamplesPerSec: klipp_media::SAMPLE_RATE,
                nAvgBytesPerSec: klipp_media::SAMPLE_RATE * 8,
                nBlockAlign: 8,
                wBitsPerSample: 32,
                cbSize: 0,
            };
            // This virtual device has no format of its own: it delivers the one asked for.
            let flags = AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER_100NS, 0, &format, None)?;
            let capture: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            Ok(Self { client, capture, times: Cell::default(), device_name: name.into() })
        }
    }

    /// Hands every waiting packet to `on_packet(time, samples, frames)`. `time` is
    /// the first sample's capture time on the shared clock; `samples` is
    /// interleaved stereo, or `None` when the packet is silence.
    pub fn read(&self, mut on_packet: impl FnMut(i64, Option<&[f32]>, usize)) -> Result<()> {
        unsafe {
            while self.capture.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags, mut qpc) = (std::ptr::null_mut(), 0u32, 0u32, 0u64);
                self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc))?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null();
                let samples = (!silent).then(|| slice::from_raw_parts(data as *const f32, frames as usize * CHANNELS));
                let mut times = self.times.get();
                let (time, moved) = times.place(qpc as i64, frames as usize, clock::now());
                self.times.set(times);
                match moved {
                    Some((off, 1)) => warning!("audio: {}: its clock is {} off; timing its sound by when it arrives", self.device_name, span(off)),
                    Some((off, n)) => warning!("audio: {}: its clock moved again ({n} times so far, now {} off)", self.device_name, span(off)),
                    None => {}
                }
                on_packet(time, samples, frames as usize);
                self.capture.ReleaseBuffer(frames)?;
            }
        }
        Ok(())
    }
}

impl Drop for Capturer {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}
