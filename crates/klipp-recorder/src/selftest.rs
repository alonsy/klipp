//! `klipp-recorder --test-recording SECONDS FILE [--settings FILE]`: records
//! the screen for a few seconds with the video settings, straight into an
//! MP4, without the tray, the hotkey or the running recorder. For checking a
//! capture method or a graphics card's encoder on someone's PC.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Context, Result};
use klipp_media::Mp4Writer;

use crate::clock;
use crate::cue::{self, Sound};
use crate::replay::{ReplayBuffer, VIDEO};
use crate::wasapi::Capturer;
use crate::settings::Settings;
use crate::state::Shared;
use crate::video::VideoPipeline;

/// `klipp-recorder --test-audio SECONDS FILE.wav`: records "everything but
/// Klipp" while this process plays the save sound 1.5 s in (it must not be in
/// the file), and prints the loudness every quarter second.
pub fn run_audio(seconds: u32, out: &Path) -> Result<()> {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let capture = Capturer::open_excluding_klipp()?;
    // The usual capture of the default device alongside, for comparison: the chime must be there.
    let usual = Capturer::open("", crate::wasapi::SourceKind::Loopback)?.context("no default playback device")?;
    let chime = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(1500));
        cue::play_once(Sound::Saved, "", 1.0)
    });
    let (mut samples, mut stamped, mut packets) = (Vec::<f32>::new(), 0usize, 0usize);
    let mut usual_samples = Vec::<f32>::new();
    let keep = |into: &mut Vec<f32>, data: Option<&[f32]>, frames: usize| match data {
        Some(d) => into.extend_from_slice(d),
        None => into.extend(std::iter::repeat_n(0.0, frames * 2)),
    };
    let end = std::time::Instant::now() + Duration::from_secs(seconds as u64);
    while std::time::Instant::now() < end {
        capture.read(|time, data, frames| {
            packets += 1;
            stamped += (time != 0) as usize;
            keep(&mut samples, data, frames);
        })?;
        usual.read(|_, data, frames| keep(&mut usual_samples, data, frames))?;
        std::thread::sleep(Duration::from_millis(10));
    }
    chime.join().map_err(|_| anyhow::anyhow!("the chime thread panicked"))??;
    write_wav(out, &samples)?;
    write_wav(&out.with_extension("usual.wav"), &usual_samples)?;
    println!("{packets} packets ({stamped} with a capture time), {:.1} s of audio", samples.len() as f64 / 2.0 / 48_000.0);
    // Loudness per quarter second (RMS, dBFS).
    let window = 48_000 / 4 * 2;
    let line: Vec<String> = samples
        .chunks(window)
        .enumerate()
        .map(|(i, c)| {
            let rms = (c.iter().map(|s| s * s).sum::<f32>() / c.len().max(1) as f32).sqrt();
            format!("{:.2}s {:.0}", i as f32 / 4.0, 20.0 * rms.max(1e-6).log10())
        })
        .collect();
    println!("{}", line.join(" | "));
    let (usual, skipped) = (chime_match(&usual_samples), chime_match(&samples));
    println!("Klipp's save sound, best match: {usual:.2} in the usual capture, {skipped:.2} in everything-but-Klipp");
    println!(
        "{}",
        if usual < 0.6 {
            "Inconclusive: the save sound didn't show up in the usual capture either (is the default device muted?)."
        } else if skipped < 0.3 {
            "Works: Klipp's own sound was left out."
        } else {
            "Doesn't work here: Klipp's own sound was recorded."
        }
    );
    Ok(())
}

/// `klipp-recorder --test-chat SECONDS FILE.wav [--device NAME]`: records the
/// Discord split three ways at once (everything but Discord, only Klipp, only
/// Discord) while this process plays the save sound 1.5 s in, then takes
/// Klipp's own sound out of the first and reports what's left. Writes the
/// Game track as Klipp would make it to FILE, the others next to it.
pub fn run_chat(seconds: u32, out: &Path, device: &str) -> Result<()> {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use crate::process_loopback::Target;
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let discord = crate::discord::main_process();
    let me = std::process::id();
    let everything_but = Capturer::open_programs(Target::AllBut(discord.unwrap_or(me)), "everything but Discord")?;
    let klipp = Capturer::open_programs(Target::Only(me), "Klipp")?;
    let chat = discord.map(|pid| Capturer::open_programs(Target::Only(pid), "Discord")).transpose()?;
    println!("Discord: {}", discord.map_or("not running".into(), |pid| format!("process {pid}")));

    let device = device.to_string();
    let chime = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        cue::play_once(Sound::Saved, &device, 1.0)
    });
    // Every packet, placed by its capture time on the shared clock.
    let t0 = clock::now();
    let frames = (seconds as usize + 1) * 48_000;
    let place = |into: &mut Vec<f32>, time: i64, data: Option<&[f32]>, n: usize| {
        let start = ((time - t0) as i128 * 48_000 / clock::SECOND as i128).max(0) as usize;
        if let Some(d) = data {
            for (i, s) in d.iter().enumerate().take(n * 2) {
                if let Some(slot) = into.get_mut(start * 2 + i) {
                    *slot = *s;
                }
            }
        }
    };
    let (mut game, mut own, mut voices) = (vec![0f32; frames * 2], vec![0f32; frames * 2], vec![0f32; frames * 2]);
    let end = std::time::Instant::now() + Duration::from_secs(seconds as u64);
    while std::time::Instant::now() < end {
        everything_but.read(|t, d, n| place(&mut game, t, d, n))?;
        klipp.read(|t, d, n| place(&mut own, t, d, n))?;
        if let Some(c) = &chat {
            c.read(|t, d, n| place(&mut voices, t, d, n))?;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    chime.join().map_err(|_| anyhow::anyhow!("the chime thread panicked"))??;

    // Through the recorder's own canceller, frame by frame as the audio thread runs it.
    let loud: Vec<usize> = own.chunks(2).enumerate().filter(|(_, s)| s[0].abs() + s[1].abs() > 1e-4).map(|(i, _)| i).collect();
    let (Some(&first), Some(&last)) = (loud.first(), loud.last()) else {
        anyhow::bail!("Klipp's own sound wasn't captured (is the save sound device muted?)");
    };
    let (mut track, mut cancel) = (crate::timeline::Timeline::new(96_000), crate::cancel::Canceller::new(96_000));
    let (mut clean, mut mixed, mut written) = (Vec::with_capacity(game.len()), 0usize, 0usize);
    while mixed + 1024 <= frames.saturating_sub(4800) {
        while written < (mixed + 4800).min(frames) {
            let k = 480.min(frames - written);
            track.write(written as i64, Some(&game[written * 2..(written + k) * 2]), k, mixed as i64);
            cancel.write(written as i64, Some(&own[written * 2..(written + k) * 2]), k, mixed as i64);
            written += k;
        }
        let (mut l, mut r) = (vec![0.0; 1024], vec![0.0; 1024]);
        track.take(mixed as i64, &mut l, &mut r);
        cancel.apply(mixed as i64, &mut l, &mut r, &track);
        clean.extend(l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]));
        mixed += 1024;
    }
    clean.resize(game.len(), 0.0);
    let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
    let window = first * 2..(last + 1) * 2;
    let db = 10.0 * (energy(&clean[window.clone()]).max(1e-12) / energy(&game[window]).max(1e-12)).log10();
    println!(
        "Klipp's sound: {:.2} s long at {:.2} s | in everything-but-Discord: match {:.2} | after taking it out: {db:.1} dB left",
        (last - first) as f32 / 48_000.0,
        first as f32 / 48_000.0,
        chime_match(&game),
    );
    println!(
        "{}",
        if db < -30.0 {
            "Works: Klipp's own sound was taken out of the Game track."
        } else if discord.is_none() {
            "Discord isn't running, so this only tested the usual way of leaving Klipp out."
        } else {
            "Doesn't work here: Klipp's own sound is still in the Game track."
        }
    );
    if chat.is_some() {
        println!("Discord's track: {:.1} s with sound", voices.chunks(2).filter(|s| s[0].abs() + s[1].abs() > 1e-4).count() as f32 / 48_000.0);
    }
    write_wav(out, &clean)?;
    write_wav(&out.with_extension("before.wav"), &game)?;
    write_wav(&out.with_extension("klipp.wav"), &own)?;
    write_wav(&out.with_extension("discord.wav"), &voices)?;
    Ok(())
}

/// `klipp-recorder --levels SECONDS`: how loud each track of the settings is,
/// ten times a second, as JSON lines, for the level bars in the Klipp window.
/// Opens the same sources the recorder would, beside it.
pub fn run_levels(seconds: u32, settings: &Settings) -> Result<()> {
    use std::io::Write;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    crate::log::to_console_only();
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let configs = settings.audio_sources();
    let follow = configs.iter().any(|c| matches!(c.programs, crate::audio::Programs::AllButDiscord | crate::audio::Programs::Discord));
    let discord = if follow { crate::discord::main_process() } else { None };
    let opened: Vec<_> = configs.iter().map(|c| crate::audio::open(c, discord, true).ok().flatten()).collect();
    let router = crate::wasapi::list_devices()
        .map(|d| crate::process_loopback::router_present(&d.into_iter().filter(|d| d.kind == crate::wasapi::SourceKind::Loopback).map(|d| d.name).collect::<Vec<_>>()))
        .unwrap_or(false);
    // Why a track records nothing, for the window to say.
    let why = |c: &crate::audio::AudioSourceConfig| match c.programs {
        crate::audio::Programs::Discord if router => "mixer",
        crate::audio::Programs::Discord if discord.is_none() => "no discord",
        _ => "no device",
    };
    let mut peaks = vec![0f32; configs.len()];
    let end = std::time::Instant::now() + Duration::from_secs(seconds as u64);
    let mut next = std::time::Instant::now() + Duration::from_millis(100);
    while std::time::Instant::now() < end {
        for (k, o) in opened.iter().enumerate() {
            if let Some(o) = o {
                o.capturer.read(|_, data, _| {
                    for s in data.unwrap_or_default() {
                        peaks[k] = peaks[k].max(s.abs());
                    }
                })?;
            }
        }
        if std::time::Instant::now() >= next {
            next += Duration::from_millis(100);
            let tracks: Vec<serde_json::Value> = configs
                .iter()
                .zip(&opened)
                .zip(&peaks)
                .map(|((c, o), &p)| {
                    let db = if p > 0.0 { (20.0 * (p as f64).log10()).max(-90.0) } else { -90.0 };
                    let off = if o.is_some() { None } else { Some(why(c)) };
                    serde_json::json!({ "track": c.title, "db": (db * 10.0).round() / 10.0, "off": off })
                })
                .collect();
            println!("{}", serde_json::json!({ "tracks": tracks }));
            let _ = std::io::stdout().flush();
            peaks.fill(0.0);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// How well the built-in save sound matches anywhere in `recording` (48 kHz
/// stereo): normalized cross-correlation at 8 kHz mono, 1.0 = exactly there.
fn chime_match(recording: &[f32]) -> f32 {
    // Mono, then averaged down to 8 kHz from any rate (44.1 kHz doesn't divide evenly).
    let mono_8k = |x: &[f32], channels: usize, rate: usize| -> Vec<f32> {
        let mono: Vec<f32> = x.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect();
        let n = mono.len() * 8000 / rate;
        (0..n)
            .map(|i| {
                let (a, b) = (i * rate / 8000, ((i + 1) * rate / 8000).max(i * rate / 8000 + 1).min(mono.len()));
                mono[a..b].iter().sum::<f32>() / (b - a).max(1) as f32
            })
            .collect()
    };
    let chime = cue::saved_sound();
    let t = mono_8k(&chime.samples, chime.channels as usize, chime.rate as usize);
    let x = mono_8k(recording, 2, 48_000);
    let tn = t.iter().map(|v| v * v).sum::<f32>().sqrt();
    if t.is_empty() || x.len() < t.len() || tn == 0.0 {
        return 0.0;
    }
    (0..=x.len() - t.len())
        .step_by(2)
        .map(|lag| {
            let w = &x[lag..lag + t.len()];
            let dot: f32 = w.iter().zip(&t).map(|(a, b)| a * b).sum();
            let e = w.iter().map(|v| v * v).sum::<f32>().sqrt();
            if e == 0.0 { 0.0 } else { dot / (tn * e) }
        })
        .fold(0.0, f32::max)
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let data_len = (samples.len() * 4) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend(b"RIFF");
    out.extend((36 + data_len).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(3u16.to_le_bytes()); // IEEE float
    out.extend(2u16.to_le_bytes());
    out.extend(48_000u32.to_le_bytes());
    out.extend((48_000u32 * 8).to_le_bytes());
    out.extend(8u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    for s in samples {
        out.extend(s.to_le_bytes());
    }
    std::fs::write(path, out)?;
    Ok(())
}

pub fn run(seconds: u32, out: &Path, settings: &Settings) -> Result<()> {
    let window = (seconds as i64 + 5) * clock::SECOND;
    let shared = Arc::new(Shared::new(ReplayBuffer::new(window, 2_000_000_000)));
    let pipeline = VideoPipeline::start(settings.video_settings(), shared.clone())?;
    std::thread::sleep(Duration::from_secs(seconds as u64));
    // Stopping flushes the encoder's last frames into the buffer.
    drop(pipeline);

    let stream = shared.video_stream.lock().unwrap().clone().context("no video stream")?;
    let clip = shared.buffer.lock().unwrap().snapshot(clock::now(), window).context("nothing was recorded")?;
    let mut writer = Mp4Writer::create(out, &[stream], &[])?;
    let mut base = None;
    let mut packets = 0;
    for (info, range) in clip.packets.iter().filter(|(p, _)| p.stream == VIDEO) {
        let b = *base.get_or_insert(info.pts);
        writer.write(0, &clip.data[range.clone()], info.pts - b, info.dts - b, info.duration, info.key)?;
        packets += 1;
    }
    writer.finish()?;
    let n = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
    println!(
        "{packets} frames written to {} | frames {} (new {}, late {}, skipped {}) | {:.1} MB",
        out.display(),
        n(&shared.frames),
        n(&shared.new_frames),
        n(&shared.late_frames),
        n(&shared.skipped_ticks),
        clip.data.len() as f64 / 1_048_576.0
    );
    Ok(())
}
