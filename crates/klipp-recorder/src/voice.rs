//! "Klipp that": saves a clip when you say so.
//!
//! The audio thread copies the microphone into a short queue and never waits;
//! this file's thread takes it down to 16 kHz, leaves out what's too quiet to
//! be speech, and hands the rest to `klipp-voice.exe`, the program that does
//! the listening at low priority. Nothing is kept, and nothing leaves this PC.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use klipp_media::SAMPLE_RATE;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};

use crate::clock::{self, SECOND};
use crate::guard::{Budget, Limit};
use crate::log::{info, warning};
use crate::state::Shared;

/// To the main thread: the phrase was heard, save like the hotkey does.
pub const WM_VOICE_SAVE: u32 = WM_APP + 9;

const VOICE_RATE: usize = 16_000;
const DOWN: usize = SAMPLE_RATE as usize / VOICE_RATE;
/// 100 ms at 16 kHz.
const BLOCK: usize = VOICE_RATE / 10;
/// About 2 s of audio frames; beyond that a stalled listener loses audio
/// rather than holding up recording.
const QUEUE_FRAMES: usize = 100;
/// Quieter than this isn't speech (-55 dBFS). Low on purpose: quiet mics exist.
const SPEECH_LEVEL: f32 = 0.0018;
/// Listening goes on this long after the last speech...
const HOLD_BLOCKS: u32 = 20;
/// ...and starts this far before it, so the phrase's start isn't lost.
const PRE_ROLL_BLOCKS: usize = 5;
/// One moment, one clip, however many times it's said.
const COOLDOWN: i64 = 5 * SECOND;
/// Hits that can save, whatever the model thinks it heard: three moments in a
/// minute is a lot to say out loud, and anything repeating past that (a voice
/// on speakers, a video) is left out instead of filling the drive. Saying it
/// again within the merge gap makes one clip longer, so this keeps that short too.
const HITS: &[Limit] = &[(3, 60 * SECOND), (8, 600 * SECOND)];

/// `%APPDATA%\Klipp\voice.json`. A file of its own: settings.toml is shared
/// with builds that don't have this.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct VoiceSettings {
    pub enabled: bool,
    /// How the speech model hears this person say it, from teaching.
    pub personal: Vec<String>,
}

impl VoiceSettings {
    fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("Klipp").join("voice.json"))
    }

    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self) -> Result<()> {
        let path = Self::path().context("no APPDATA")?;
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    fn stamp() -> Option<SystemTime> {
        Self::path().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
    }
}

fn program() -> Result<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe()?;
    Ok((exe.with_file_name("klipp-voice.exe"), exe.with_file_name("voice-model")))
}

/// Windowed-sinc low-pass that stops at the new rate's limit, so the
/// downsampling doesn't fold the highs back into speech.
struct Downsampler {
    taps: Vec<f32>,
    history: Vec<f32>,
}

impl Downsampler {
    const TAPS: usize = 31;

    fn new() -> Self {
        let cutoff = 0.45 / DOWN as f32;
        let middle = (Self::TAPS - 1) as f32 / 2.0;
        let pi = std::f32::consts::PI;
        let mut taps: Vec<f32> = (0..Self::TAPS)
            .map(|i| {
                let t = i as f32 - middle;
                let sinc = if t == 0.0 { 2.0 * cutoff } else { (2.0 * pi * cutoff * t).sin() / (pi * t) };
                sinc * (0.5 - 0.5 * (2.0 * pi * i as f32 / (Self::TAPS - 1) as f32).cos())
            })
            .collect();
        let sum: f32 = taps.iter().sum();
        taps.iter_mut().for_each(|t| *t /= sum);
        Self { taps, history: Vec::with_capacity(Self::TAPS + 2048) }
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.history.extend_from_slice(input);
        let mut start = 0;
        while start + Self::TAPS <= self.history.len() {
            out.push(self.history[start..start + Self::TAPS].iter().zip(&self.taps).map(|(a, b)| a * b).sum());
            start += DOWN;
        }
        self.history.drain(..start);
    }
}

/// Lets through speech, plus a little before and after it.
#[derive(Default)]
struct Gate {
    before: VecDeque<Vec<f32>>,
    hold: u32,
}

impl Gate {
    /// What to pass on for this block: nothing while it's quiet.
    fn pass(&mut self, block: Vec<f32>) -> Vec<Vec<f32>> {
        let level = (block.iter().map(|s| s * s).sum::<f32>() / block.len().max(1) as f32).sqrt();
        if level >= SPEECH_LEVEL {
            self.hold = HOLD_BLOCKS;
            let mut out: Vec<Vec<f32>> = self.before.drain(..).collect();
            out.push(block);
            out
        } else if self.hold > 0 {
            self.hold -= 1;
            vec![block]
        } else {
            self.before.push_back(block);
            if self.before.len() > PRE_ROLL_BLOCKS {
                self.before.pop_front();
            }
            Vec::new()
        }
    }
}

fn write_block(stdin: &mut ChildStdin, block: &[f32]) -> std::io::Result<()> {
    let bytes: Vec<u8> = block.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    stdin.write_all(&bytes)
}

fn feed(frames: Receiver<Vec<f32>>, mut stdin: ChildStdin) {
    let (mut down, mut gate) = (Downsampler::new(), Gate::default());
    let (mut pending, mut heard, mut listened) = (Vec::with_capacity(BLOCK * 2), 0u64, 0u64);
    for frame in frames {
        down.push(&frame, &mut pending);
        while pending.len() >= BLOCK {
            let block: Vec<f32> = pending.drain(..BLOCK).collect();
            heard += 1;
            for b in gate.pass(block) {
                listened += 1;
                if write_block(&mut stdin, &b).is_err() {
                    return;
                }
            }
        }
    }
    info!("voice: listened to {} s of {} s", listened / 10, heard / 10);
}

/// Which hits become saves.
struct Hits {
    last: Option<i64>,
    budget: Budget,
}

impl Hits {
    fn new() -> Self {
        Self { last: None, budget: Budget::new(HITS) }
    }

    /// Why a hit at `now` is left out, if it is.
    fn left_out(&mut self, now: i64) -> Option<&'static str> {
        if self.last.is_some_and(|t| now - t < COOLDOWN) {
            return Some("just saved");
        }
        if !self.budget.take(now) {
            return Some("too many lately");
        }
        self.last = Some(now);
        None
    }
}

fn watch(out: impl std::io::Read, main_thread: u32) {
    let mut hits = Hits::new();
    for line in BufReader::new(out).lines().map_while(Result::ok) {
        if line == "ready" {
            info!("voice: listening for \"Klipp that\"");
        } else if let Some(what) = line.strip_prefix("hit ") {
            match hits.left_out(clock::now()) {
                Some(why) => info!("voice: heard {what} ({why}; left out)"),
                None => {
                    info!("voice: heard {what}");
                    let _ = unsafe { PostThreadMessageW(main_thread, WM_VOICE_SAVE, WPARAM(0), LPARAM(0)) };
                }
            }
        }
    }
}

/// The running listener. Dropping it stops listening.
pub struct Voice {
    shared: Arc<Shared>,
    child: Child,
    threads: Vec<JoinHandle<()>>,
}

impl Voice {
    pub fn start(settings: &VoiceSettings, shared: Arc<Shared>, main_thread: u32) -> Result<Self> {
        let (exe, model) = program()?;
        let mut child = Command::new(&exe)
            .arg("listen")
            .arg(&model)
            .args(&settings.personal)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW.0 | BELOW_NORMAL_PRIORITY_CLASS.0)
            .spawn()
            .with_context(|| format!("starting {}", exe.display()))?;
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let (tx, rx) = sync_channel(QUEUE_FRAMES);
        let threads = vec![
            thread::Builder::new().name("voice-feed".into()).spawn(move || feed(rx, stdin))?,
            thread::Builder::new().name("voice-watch".into()).spawn(move || watch(stdout, main_thread))?,
        ];
        *shared.voice.lock().unwrap() = Some(tx);
        Ok(Self { shared, child, threads })
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        // Ends the feed, which closes stdin, which ends the program.
        *self.shared.voice.lock().unwrap() = None;
        let _ = self.child.wait();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        info!("voice: stopped");
    }
}

/// Follows voice.json, which the window writes.
pub struct VoiceWatcher {
    stamp: Option<SystemTime>,
    shown: Option<VoiceSettings>,
    running: Option<Voice>,
    main_thread: u32,
}

impl VoiceWatcher {
    pub fn new(main_thread: u32) -> Self {
        Self { stamp: None, shown: None, running: None, main_thread }
    }

    /// Once a second.
    pub fn poll(&mut self, shared: &Arc<Shared>) {
        let stamp = VoiceSettings::stamp();
        if stamp == self.stamp && self.shown.is_some() {
            return;
        }
        self.stamp = stamp;
        let settings = VoiceSettings::load();
        if self.shown.as_ref() == Some(&settings) {
            return;
        }
        self.running = None;
        if settings.enabled {
            match Voice::start(&settings, shared.clone(), self.main_thread) {
                Ok(v) => self.running = Some(v),
                Err(e) => warning!("voice: couldn't start listening: {e:#}"),
            }
        }
        self.shown = Some(settings);
    }
}

/// `--voice-teach`: takes the phrase three times from the microphone Klipp
/// records and keeps how the model hears it, then listens. One line of JSON
/// per step, for the window: `listen`, then `heard` or `retry`, and `done` or
/// `failed` at the end.
pub fn teach(settings: &crate::settings::Settings) -> Result<()> {
    crate::log::to_console_only();
    let mut voice = VoiceSettings::load();
    let was_on = voice.enabled;
    if was_on {
        // Or every take would save a clip. The recorder looks once a second.
        voice.enabled = false;
        voice.save()?;
        thread::sleep(Duration::from_millis(1500));
    }
    let learned = takes(settings);
    voice.enabled = was_on;
    if let Ok(Some(personal)) = &learned {
        voice.enabled = true;
        voice.personal = personal.clone();
    }
    voice.save()?;
    if learned?.is_some() {
        say(serde_json::json!({ "step": "done", "personal": voice.personal }));
    }
    Ok(())
}

fn say(v: serde_json::Value) {
    println!("{v}");
    let _ = std::io::stdout().flush();
}

/// How the model heard each take; `None` if it heard nothing usable.
fn takes(settings: &crate::settings::Settings) -> Result<Option<Vec<String>>> {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    const TAKES: usize = 3;
    const TAKE: Duration = Duration::from_millis(2500);
    const GAP: Duration = Duration::from_millis(900);
    /// Fewer sounds than this is a cough, or half the phrase.
    const FEWEST: usize = 5;
    /// More than this isn't the phrase alone.
    const MOST: usize = 14;

    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let source = settings.audio_sources().into_iter().find(|c| c.kind == crate::wasapi::SourceKind::Capture);
    let opened = source.as_ref().map(|c| crate::audio::open(c, None, true)).transpose()?.flatten();
    let Some(opened) = opened else {
        say(serde_json::json!({ "step": "failed", "why": "no microphone" }));
        return Ok(None);
    };
    let mut personal = Vec::new();
    let (mut take, mut tries) = (1, 0);
    while take <= TAKES && tries < TAKES * 3 {
        tries += 1;
        thread::sleep(GAP);
        opened.capturer.read(|_, _, _| {})?;
        say(serde_json::json!({ "step": "listen", "take": take, "of": TAKES }));
        let mut pcm = Vec::new();
        let end = Instant::now() + TAKE;
        while Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
            opened.capturer.read(|_, data, frames| match data {
                Some(d) => pcm.extend(d.chunks_exact(2).map(|s| (s[0] + s[1]) * 0.5)),
                None => pcm.resize(pcm.len() + frames, 0.0),
            })?;
        }
        let peak = pcm.iter().fold(0f32, |p, s| p.max(s.abs()));
        let mut down = Downsampler::new();
        let mut samples = Vec::with_capacity(pcm.len() / DOWN);
        down.push(&pcm, &mut samples);
        let heard = hear(&samples)?;
        let count = heard.split_whitespace().count();
        if peak < SPEECH_LEVEL * 5.0 {
            say(serde_json::json!({ "step": "retry", "why": "quiet", "take": take }));
        } else if !(FEWEST..=MOST).contains(&count) {
            say(serde_json::json!({ "step": "retry", "why": if count < FEWEST { "short" } else { "long" }, "take": take }));
        } else {
            say(serde_json::json!({ "step": "heard", "take": take, "heard": heard }));
            if !personal.contains(&heard) {
                personal.push(heard);
            }
            take += 1;
        }
    }
    if personal.is_empty() {
        say(serde_json::json!({ "step": "failed", "why": "nothing heard" }));
        return Ok(None);
    }
    Ok(Some(personal))
}

fn hear(samples: &[f32]) -> Result<String> {
    let (exe, model) = program()?;
    let mut child = Command::new(&exe)
        .arg("decode")
        .arg(Path::new(&model))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let bytes: Vec<u8> = samples.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    let writer = thread::spawn(move || stdin.write_all(&bytes));
    let out = child.wait_with_output()?;
    let _ = writer.join();
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
