//! Reads what games show on screen. While Fortnite is in front, small areas of
//! the frame go to `fortnite` twice a second, in memory only. Nothing touches
//! the game: the pictures come from the frames Klipp already captures.
//!
//! With a profiles.json in "Klipp HUD test" next to the clips folder (nothing
//! makes that file by itself; it's for working on detectors), the games it
//! lists have pictures of their HUD saved there too, a folder per game and per
//! session: the areas where events show up (kept every 2 s), a full screenshot
//! every 15 s, and everything around each press of the save key. League of
//! Legends' live event feed, Minecraft's latest.log and World of Warcraft's
//! combat log are saved next to them. The file is read again when it changes.
//!
//! Threads: the video thread only copies the frame into a CPU-readable
//! texture twice a second (`Tap::tick`). The writer thread reads it back,
//! crops, encodes JPEGs and writes files at below-normal priority. The main
//! thread decides when a session starts and ends (`HudTest::poll`).

use std::collections::VecDeque;
use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ERROR_WAS_STILL_DRAWING, IDXGIFactory1, IDXGIOutput6};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::System::Com::StructuredStorage::{IPropertyBag2, PROPBAG2};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STREAM_SEEK_END, STREAM_SEEK_SET};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{
    GetCurrentThread, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, SetThreadPriority,
    THREAD_PRIORITY_BELOW_NORMAL,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId};
use windows::core::{Interface, PCWSTR, PWSTR, w};

use crate::capture::LatestFrame;
use crate::events::{EventKind, GameEvent};
use crate::fortnite;
use crate::gpu::Gpu;
use crate::log::{info, warning};
use crate::state::Shared;

/// The folder next to the clips folder.
pub const FOLDER: &str = "Klipp HUD test";
/// A session ends when its game hasn't been in front for this long.
const SESSION_GAP: Duration = Duration::from_secs(120);
/// CPU-readable copies of the frame in flight at once; more means the writer is behind.
const STAGING: usize = 3;
/// Log copies stop at this size.
const TAIL_LIMIT: u64 = 500 << 20;

// Settings and profiles (profiles.json)

/// An area of the screen, as fractions of its width and height.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// 0.5 saves it at half size (big areas like banners).
    #[serde(default = "one")]
    pub scale: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// Process file names, lowercase.
    pub exes: Vec<String>,
    /// The window title must contain this (Minecraft Java runs as javaw.exe).
    #[serde(default)]
    pub title_contains: Option<String>,
    /// Or the exe's folder contains this (a game whose exe name isn't known yet).
    #[serde(default)]
    pub folder_contains: Option<String>,
    /// "league", "minecraft" or "wow": what to record next to the pictures.
    #[serde(default)]
    pub extra: Option<String>,
    pub regions: Vec<Region>,
}

impl Profile {
    fn matches(&self, exe: &Path, title: &str) -> bool {
        let file = exe.file_name().and_then(|f| f.to_str()).unwrap_or_default().to_lowercase();
        let by_exe = self.exes.iter().any(|e| e.eq_ignore_ascii_case(&file));
        let by_folder = self.folder_contains.as_ref().is_some_and(|f| exe.to_string_lossy().to_lowercase().contains(&f.to_lowercase()));
        let title_ok = self.title_contains.as_ref().is_none_or(|t| title.to_lowercase().contains(&t.to_lowercase()));
        (by_exe || by_folder) && title_ok
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// How often the HUD areas are read.
    pub sample_every_ms: u64,
    /// How often a reading is kept outside markers.
    pub keep_every_ms: u64,
    /// How often a full screenshot is saved.
    pub frame_every_s: u64,
    /// Seconds kept before and after a press of the save key.
    pub marker_before_s: u64,
    pub marker_after_s: u64,
    /// Full screenshots right after a press.
    pub marker_frames: u32,
    /// JPEG quality, 0 to 1.
    pub jpeg_quality: f32,
    /// Stops saving when the whole HUD test folder is this big.
    pub max_total_gb: f64,
    pub games: Vec<Profile>,
}

fn r(name: &str, x: f32, y: f32, w: f32, h: f32) -> Region {
    Region { name: name.into(), x, y, w, h, scale: 1.0 }
}

fn half(name: &str, x: f32, y: f32, w: f32, h: f32) -> Region {
    Region { name: name.into(), x, y, w, h, scale: 0.5 }
}

fn game(name: &str, exes: &[&str], extra: Option<&str>, regions: Vec<Region>) -> Profile {
    Profile {
        name: name.into(),
        exes: exes.iter().map(|e| e.to_string()).collect(),
        title_contains: None,
        folder_contains: None,
        extra: extra.map(str::to_string),
        regions,
    }
}

/// Only reading Fortnite: no pictures, no other games.
fn reader_config() -> Config {
    let mut config = default_config();
    config.games.retain(|g| g.name == fortnite::GAME_NAME);
    config.games.iter_mut().for_each(|g| g.regions.clear());
    config
}

/// Areas are generous first guesses for the default HUD; the full screenshots
/// allow cropping again later.
pub fn default_config() -> Config {
    let mut minecraft = game(
        "Minecraft",
        &["javaw.exe", "java.exe", "minecraft.windows.exe"],
        Some("minecraft"),
        vec![r("chat", 0.0, 0.55, 0.45, 0.40), half("center", 0.20, 0.10, 0.60, 0.45), r("toasts", 0.70, 0.0, 0.30, 0.15)],
    );
    minecraft.title_contains = Some("Minecraft".into());
    let mut fc = game(
        "EA SPORTS FC",
        &["fc27.exe", "fc26.exe", "fc27_trial.exe"],
        None,
        vec![r("scoreboard", 0.0, 0.0, 0.35, 0.10), half("center", 0.20, 0.20, 0.60, 0.60)],
    );
    fc.folder_contains = Some("EA SPORTS FC".into());
    Config {
        sample_every_ms: 500,
        keep_every_ms: 2000,
        frame_every_s: 15,
        marker_before_s: 6,
        marker_after_s: 4,
        marker_frames: 3,
        jpeg_quality: 0.9,
        max_total_gb: 10.0,
        games: vec![
            game(
                "League of Legends",
                &["league of legends.exe"],
                Some("league"),
                vec![r("announcer", 0.25, 0.08, 0.50, 0.25), r("killfeed", 0.76, 0.14, 0.24, 0.40), r("scoreboard", 0.70, 0.0, 0.30, 0.06)],
            ),
            game(
                "Fortnite",
                &["fortniteclient-win64-shipping.exe"],
                None,
                vec![
                    r("top-right", 0.76, 0.0, 0.24, 0.42),
                    r("messages", 0.30, 0.55, 0.40, 0.25),
                    r("feed", 0.0, 0.45, 0.32, 0.35),
                    half("center", 0.20, 0.08, 0.60, 0.40),
                ],
            ),
            game(
                "VALORANT",
                &["valorant-win64-shipping.exe"],
                None,
                vec![
                    r("killfeed", 0.68, 0.06, 0.32, 0.30),
                    r("kill-banner", 0.36, 0.70, 0.28, 0.16),
                    r("top-bar", 0.33, 0.0, 0.34, 0.09),
                    half("center", 0.20, 0.12, 0.60, 0.38),
                ],
            ),
            minecraft,
            game("World of Warcraft", &["wow.exe", "wowclassic.exe", "wow-64.exe"], Some("wow"), vec![half("center", 0.25, 0.10, 0.50, 0.35)]),
            game(
                "Rainbow Six Siege X",
                &["rainbowsix.exe", "rainbowsix_vulkan.exe", "rainbowsix_be.exe"],
                None,
                vec![r("killfeed", 0.66, 0.06, 0.34, 0.32), r("top-bar", 0.30, 0.0, 0.40, 0.12), half("center", 0.20, 0.15, 0.60, 0.55)],
            ),
            fc,
        ],
    }
}

/// Reads profiles.json, writing the defaults first if it isn't there.
fn load_config(root: &Path) -> Config {
    let path = root.join("profiles.json");
    if !path.exists() {
        let _ = fs::create_dir_all(root);
        let _ = fs::write(&path, serde_json::to_vec_pretty(&default_config()).unwrap_or_default());
    }
    match fs::read(&path).map_err(anyhow::Error::from).and_then(|b| Ok(serde_json::from_slice::<Config>(&b)?)) {
        Ok(c) => c,
        Err(e) => {
            warning!("HUD test: profiles.json can't be read ({e:#}); using the defaults");
            default_config()
        }
    }
}

// Time

#[derive(Clone, Debug)]
struct Stamp {
    unix_ms: i64,
    date: String,
    /// "21-43-05-120", for file names.
    file: String,
    /// "21:43:05.120", for text.
    clock: String,
}

impl Stamp {
    fn now() -> Self {
        let t = unsafe { GetLocalTime() };
        let unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        Self {
            unix_ms,
            date: format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay),
            file: format!("{:02}-{:02}-{:02}-{:03}", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds),
            clock: format!("{:02}:{:02}:{:02}.{:03}", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds),
        }
    }
}

// The video thread's side

/// A CPU-readable copy of a frame, on its way to the writer.
struct Staged {
    texture: ID3D11Texture2D,
    context: ID3D11DeviceContext,
    width: u32,
    height: u32,
}

// SAFETY: D3D11 resources are free-threaded and the immediate context is
// multithread-protected (gpu.rs), so the writer may map the texture.
unsafe impl Send for Staged {}

/// What the video thread samples for the current session.
struct Plan {
    id: u64,
    regions: Arc<Vec<Region>>,
    sample_every: Duration,
    frame_every: Duration,
}

#[derive(Default)]
struct TapState {
    plan_id: u64,
    next_sample: Option<Instant>,
    next_full: Option<Instant>,
    made: usize,
    failed: bool,
}

/// Lives in `Shared`; the video thread calls `tick` every frame.
#[derive(Default)]
pub struct Tap {
    on: AtomicBool,
    /// Fortnite is being read: its auto-clips are on and it fills the screen.
    reading: AtomicBool,
    /// Its limits have stopped its clips for now.
    fortnite_paused: AtomicBool,
    plan: Mutex<Option<Arc<Plan>>>,
    /// Full screenshots asked for by markers.
    full_requests: AtomicU32,
    out: Mutex<Option<SyncSender<Msg>>>,
    pool: Mutex<Vec<Staged>>,
    state: Mutex<TapState>,
}

impl Tap {
    /// Copies the newest frame for the writer when a sample is due. Costs one
    /// GPU copy twice a second while a picked game is in front, nothing otherwise.
    pub fn tick(&self, gpu: &Gpu, frame: &LatestFrame) {
        if !self.on.load(Ordering::Relaxed) {
            return;
        }
        let Some(plan) = self.plan.lock().unwrap().clone() else { return };
        let mut st = self.state.lock().unwrap();
        if st.plan_id != plan.id {
            let made = st.made;
            *st = TapState { plan_id: plan.id, made, ..Default::default() };
        }
        let now = Instant::now();
        if st.next_sample.is_some_and(|t| now < t) || frame.generation() == 0 {
            return;
        }
        st.next_sample = Some(now + plan.sample_every);
        let latest = frame.lock();
        let (w, h) = (latest.width, latest.height);
        let staged = loop {
            let reused = self.pool.lock().unwrap().pop();
            match reused {
                Some(s) if (s.width, s.height) == (w, h) => break Some(s),
                Some(_) => st.made -= 1,
                None if st.made < STAGING => match staging(&gpu.device, w, h) {
                    Ok(texture) => {
                        st.made += 1;
                        break Some(Staged { texture, context: gpu.context.clone(), width: w, height: h });
                    }
                    Err(e) => {
                        if !st.failed {
                            warning!("HUD test: no readable texture ({e:#})");
                            st.failed = true;
                        }
                        break None;
                    }
                },
                // The writer is behind: skip this sample.
                None => break None,
            }
        };
        let Some(staged) = staged else { return };
        unsafe { gpu.context.CopyResource(&staged.texture, &latest.texture) };
        drop(latest);
        let periodic = st.next_full.is_none_or(|t| now >= t);
        if periodic {
            st.next_full = Some(now + plan.frame_every);
        }
        // Only this thread takes requests away, so checking first can't go below zero.
        let asked = self.full_requests.load(Ordering::Relaxed) > 0 && self.full_requests.fetch_sub(1, Ordering::Relaxed) > 0;
        let msg = Msg::Frame(Frame { id: plan.id, at: Stamp::now(), clock: crate::clock::now(), staged, periodic, asked, regions: plan.regions.clone() });
        let out = self.out.lock().unwrap();
        if let Some(Err(mpsc::TrySendError::Full(Msg::Frame(f)) | mpsc::TrySendError::Disconnected(Msg::Frame(f)))) = out.as_ref().map(|o| o.try_send(msg)) {
            self.give_back(f.staged);
        }
    }

    fn give_back(&self, staged: Staged) {
        self.pool.lock().unwrap().push(staged);
    }

    fn start(&self, plan: Plan) {
        *self.plan.lock().unwrap() = Some(Arc::new(plan));
        self.on.store(true, Ordering::Relaxed);
    }

    fn pause(&self) {
        self.on.store(false, Ordering::Relaxed);
    }

    fn resume(&self) {
        if self.plan.lock().unwrap().is_some() {
            self.on.store(true, Ordering::Relaxed);
        }
    }

    fn stop(&self) {
        self.on.store(false, Ordering::Relaxed);
        *self.plan.lock().unwrap() = None;
        self.full_requests.store(0, Ordering::Relaxed);
    }
}

fn staging(device: &ID3D11Device, width: u32, height: u32) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture))? };
    texture.context("no texture")
}

// Messages to the writer

struct Frame {
    id: u64,
    at: Stamp,
    /// When it was copied, on the shared clock (for game events).
    clock: i64,
    staged: Staged,
    periodic: bool,
    asked: bool,
    regions: Arc<Vec<Region>>,
}

enum Msg {
    Start { id: u64, game: String, profile: Profile, exe: PathBuf, config: Config },
    Frame(Frame),
    Marker { id: u64, at: Stamp, label: String },
    Event { id: u64, at: Stamp, event: Value },
    File { id: u64, name: String, bytes: Vec<u8> },
    Stop { id: u64 },
    /// Answers once everything before it is written (when Klipp quits).
    Flush(SyncSender<()>),
}

// The main thread's side

struct Live {
    id: u64,
    game: String,
    last_seen: Instant,
    in_front: bool,
    league: Option<Arc<AtomicBool>>,
}

pub struct HudTest {
    shared: Arc<Shared>,
    tx: SyncSender<Msg>,
    root: PathBuf,
    /// profiles.json exists: pictures are saved.
    pictures: bool,
    choice_stamp: Option<SystemTime>,
    /// Fortnite doesn't fill the screen, and the log has said so.
    told_windowed: bool,
    config: Config,
    config_stamp: Option<SystemTime>,
    live: Option<Live>,
    next_id: u64,
}

impl HudTest {
    /// `events` takes Fortnite's eliminations.
    pub fn start(shared: Arc<Shared>, clips_dir: &Path, events: Sender<(String, GameEvent)>) -> Result<Self> {
        let root = clips_dir.parent().map_or_else(|| clips_dir.join("HUD test"), |p| p.join(FOLDER));
        let pictures = root.join("profiles.json").exists();
        let config = if pictures { load_config(&root) } else { reader_config() };
        if pictures {
            let _ = fs::write(root.join("README.txt"), readme(&config));
        }
        let (tx, rx) = mpsc::sync_channel(32);
        *shared.hud.out.lock().unwrap() = Some(tx.clone());
        let writer_shared = shared.clone();
        let writer_root = root.clone();
        thread::Builder::new().name("hud-test".into()).spawn(move || writer(rx, writer_shared, writer_root, events, pictures))?;
        if pictures {
            info!(
                "HUD test build: saving HUD pictures of {} to {}",
                config.games.iter().map(|g| g.name.as_str()).collect::<Vec<_>>().join(", "),
                root.display()
            );
        }
        let config_stamp = modified(&root.join("profiles.json"));
        Ok(Self { shared, tx, root, pictures, choice_stamp: None, told_windowed: false, config, config_stamp, live: None, next_id: 1 })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether pictures are being saved, so the tray offers their folder.
    pub fn pictures(&self) -> bool {
        self.pictures
    }

    /// For the tray tooltip; empty when there's nothing to say.
    pub fn status(&self) -> String {
        if self.shared.hud.fortnite_paused.load(Ordering::Relaxed) {
            return "Fortnite auto-clips paused: they looked wrong".into();
        }
        match &self.live {
            Some(l) if self.pictures && l.in_front => format!("HUD test: saving {}", l.game),
            _ if self.pictures => "HUD test build".into(),
            _ => String::new(),
        }
    }

    /// Once a second: is a picked game in front? `auto_clips`: the setting.
    pub fn poll(&mut self, auto_clips: bool) {
        fortnite::follow_choice(&mut self.choice_stamp);
        let stamp = modified(&self.root.join("profiles.json"));
        if self.pictures && stamp != self.config_stamp {
            self.config_stamp = stamp;
            self.config = load_config(&self.root);
            info!("HUD test: profiles.json changed; new sessions use it");
        }
        let front = foreground();
        let found = front.as_ref().and_then(|(exe, title, _)| self.config.games.iter().find(|p| p.matches(exe, title)).map(|p| (p.clone(), exe.clone())));
        // In a window the HUD is somewhere else on the picture.
        let windowed = found.as_ref().is_some_and(|(p, _)| p.name == fortnite::GAME_NAME) && front.as_ref().is_some_and(|(.., fills)| !fills);
        if windowed && !self.told_windowed {
            info!("fortnite: it's in a window, so its screen isn't read until it fills the screen");
        }
        self.told_windowed = windowed;
        let wanted = auto_clips && fortnite::enabled() && !windowed;
        self.shared.hud.reading.store(wanted, Ordering::Relaxed);
        // Without pictures there's only Fortnite's reader to run.
        let matched = found.filter(|_| self.pictures || wanted);
        let now = Instant::now();
        match matched {
            Some((profile, _)) if self.live.as_ref().is_some_and(|l| l.game == profile.name) => {
                let live = self.live.as_mut().unwrap();
                live.last_seen = now;
                if !live.in_front {
                    live.in_front = true;
                    self.shared.hud.resume();
                }
            }
            Some((profile, exe)) => {
                self.end();
                self.begin(profile, exe);
            }
            None => {
                let ended = match &mut self.live {
                    Some(live) => {
                        if live.in_front {
                            live.in_front = false;
                            self.shared.hud.pause();
                        }
                        now.duration_since(live.last_seen) > SESSION_GAP
                    }
                    None => false,
                };
                if ended {
                    self.end();
                }
            }
        }
    }

    fn begin(&mut self, profile: Profile, exe: PathBuf) {
        let id = self.next_id;
        self.next_id += 1;
        if self.pictures {
            info!("HUD test: {} is in front ({}); saving its HUD", profile.name, exe.display());
        } else {
            info!("fortnite: in front; reading its screen");
        }
        let league = (profile.extra.as_deref() == Some("league")).then(|| {
            let alive = Arc::new(AtomicBool::new(true));
            let (alive2, tx, shared, frames) = (alive.clone(), self.tx.clone(), self.shared.clone(), self.config.marker_frames);
            let _ = thread::Builder::new().name("hud-league".into()).spawn(move || league_watch(id, alive2, tx, shared, frames));
            alive
        });
        let _ = self.tx.send(Msg::Start { id, game: profile.name.clone(), profile: profile.clone(), exe, config: self.config.clone() });
        self.shared.hud.start(Plan {
            id,
            regions: Arc::new(profile.regions.clone()),
            sample_every: Duration::from_millis(self.config.sample_every_ms.max(100)),
            frame_every: Duration::from_secs(self.config.frame_every_s.max(2)),
        });
        self.live = Some(Live { id, game: profile.name, last_seen: Instant::now(), in_front: true, league });
    }

    fn end(&mut self) {
        let Some(live) = self.live.take() else { return };
        self.shared.hud.stop();
        if let Some(alive) = live.league {
            alive.store(false, Ordering::Relaxed);
        }
        let _ = self.tx.send(Msg::Stop { id: live.id });
    }

    /// The save key was pressed: a marker, with the moments around it kept in full.
    pub fn marker(&mut self, label: &str) {
        let Some(live) = self.live.as_ref().filter(|_| self.pictures) else { return };
        if !live.in_front && live.last_seen.elapsed() > Duration::from_secs(10) {
            return;
        }
        self.shared.hud.full_requests.fetch_add(self.config.marker_frames, Ordering::Relaxed);
        let _ = self.tx.send(Msg::Marker { id: live.id, at: Stamp::now(), label: label.into() });
    }
}

impl Drop for HudTest {
    fn drop(&mut self) {
        self.end();
        // Let the writer finish the session's files before Klipp exits.
        let (done, wait) = mpsc::sync_channel(1);
        if self.tx.send(Msg::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(3));
        }
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The window covers its whole screen: fullscreen or borderless.
fn fills_screen(hwnd: HWND) -> bool {
    unsafe {
        let mut rect = RECT::default();
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetWindowRect(hwnd, &mut rect).is_err() || !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            return true;
        }
        let m = info.rcMonitor;
        rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
    }
}

/// The foreground window's exe, title and whether it fills its screen.
fn foreground() -> Option<(PathBuf, String, bool)> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return None;
        }
        let mut title = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        Some((PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])), String::from_utf16_lossy(&title[..n]), fills_screen(hwnd)))
    }
}

// The writer thread

struct Crop {
    name: String,
    width: u32,
    height: u32,
    bgr: Vec<u8>,
}

struct RingEntry {
    unix_ms: i64,
    region: String,
    file: String,
    jpeg: Arc<Vec<u8>>,
}

struct Window {
    dir: PathBuf,
    until_ms: i64,
}

struct Tail {
    what: &'static str,
    source: Option<PathBuf>,
    offset: u64,
    dest: PathBuf,
    copied: u64,
    /// World of Warcraft: the Logs folder, scanned for the newest combat log.
    scan: Option<PathBuf>,
    last_scan: Instant,
    started: SystemTime,
}

struct Session {
    id: u64,
    game: String,
    profile: Profile,
    exe: PathBuf,
    config: Config,
    dir: PathBuf,
    started: Stamp,
    screen: Option<(u32, u32)>,
    displays: Vec<Value>,
    last_kept_ms: i64,
    ring: VecDeque<RingEntry>,
    window: Option<Window>,
    region_files: u64,
    frames: u64,
    markers: u64,
    events: u64,
    bytes: u64,
    full_disk: bool,
    tails: Vec<Tail>,
    last_json: Instant,
    /// Pictures and session files are saved.
    pictures: bool,
    /// Fortnite: its eliminations counter, knocks and the Victory Royale banner, read from every sample.
    reader: Option<fortnite::Reader>,
    /// The log has said this picture can't be read.
    told_size: bool,
    eliminations: u64,
    knocked: u64,
    victories: u64,
}

struct Writer {
    shared: Arc<Shared>,
    root: PathBuf,
    wic: Option<IWICImagingFactory>,
    session: Option<Session>,
    /// Size of the HUD test folder when the session started.
    root_bytes: u64,
    events: Sender<(String, GameEvent)>,
    pictures: bool,
}

impl Session {
    fn saving(&self) -> bool {
        self.pictures && !self.full_disk
    }
}

fn writer(rx: Receiver<Msg>, shared: Arc<Shared>, root: PathBuf, events: Sender<(String, GameEvent)>, pictures: bool) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
    let wic: Option<windows::core::Result<IWICImagingFactory>> = pictures.then(|| unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) });
    if let Some(Err(e)) = &wic {
        warning!("HUD test: no image encoder ({e:#}); nothing will be saved");
    }
    let mut w = Writer { shared, root, wic: wic.and_then(Result::ok), session: None, root_bytes: 0, events, pictures };
    let mut last_pump = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(msg) => {
                if let Err(e) = w.handle(msg) {
                    warning!("HUD test: {e:#}");
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if last_pump.elapsed() >= Duration::from_secs(2) {
            last_pump = Instant::now();
            w.pump();
        }
    }
    w.finish();
}

impl Writer {
    fn handle(&mut self, msg: Msg) -> Result<()> {
        match msg {
            Msg::Start { id, game, profile, exe, config } => {
                self.finish();
                self.begin(id, game, profile, exe, config)?;
            }
            Msg::Frame(f) => {
                let result = self.frame(&f);
                self.shared.hud.give_back(f.staged);
                result?;
            }
            Msg::Marker { id, at, label } => self.marker(id, at, &label)?,
            Msg::Event { id, at, event } => {
                if let Some(s) = self.session.as_mut().filter(|s| s.id == id) {
                    s.events += 1;
                    let line = json!({ "received": at.clock, "event": event }).to_string();
                    append(&s.dir.join("events.jsonl"), format!("{line}\n").as_bytes())?;
                }
            }
            Msg::File { id, name, bytes } => {
                if let Some(s) = self.session.as_mut().filter(|s| s.id == id) {
                    fs::write(s.dir.join(name), &bytes)?;
                    s.bytes += bytes.len() as u64;
                }
            }
            Msg::Stop { id } => {
                if self.session.as_ref().is_some_and(|s| s.id == id) {
                    self.finish();
                }
            }
            Msg::Flush(done) => {
                let _ = done.send(());
            }
        }
        Ok(())
    }

    fn begin(&mut self, id: u64, game: String, profile: Profile, exe: PathBuf, config: Config) -> Result<()> {
        let started = Stamp::now();
        let game_dir = self.root.join(safe(&game));
        let dir = game_dir.join(format!("{} {}", started.date, &started.file[..8]));
        if self.pictures {
            fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            let notes = game_dir.join("notes.txt");
            if !notes.exists() {
                let _ = fs::write(&notes, notes_template(&game));
            }
            self.root_bytes = folder_size(&self.root);
        }
        let mut tails = Vec::new();
        match profile.extra.as_deref() {
            Some("minecraft") if !exe.to_string_lossy().to_lowercase().contains("minecraft.windows") => {
                let log = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(".minecraft").join("logs").join("latest.log"));
                tails.push(Tail::new("Minecraft's latest.log", log, None, dir.join("latest.log")));
            }
            Some("wow") => {
                let logs = exe.parent().map(|p| p.join("Logs"));
                let newest = logs.as_deref().and_then(newest_combat_log);
                tails.push(Tail::new("the combat log", newest, logs, dir.join("combat-log.txt")));
            }
            _ => {}
        }
        let reader = (game == fortnite::GAME_NAME).then(fortnite::Reader::default);
        let session = Session {
            pictures: self.pictures,
            reader,
            told_size: false,
            eliminations: 0,
            knocked: 0,
            victories: 0,
            id,
            game,
            profile,
            exe,
            config,
            dir,
            started,
            screen: None,
            displays: if self.pictures { displays() } else { Vec::new() },
            last_kept_ms: 0,
            ring: VecDeque::new(),
            window: None,
            region_files: 0,
            frames: 0,
            markers: 0,
            events: 0,
            bytes: 0,
            full_disk: false,
            tails,
            last_json: Instant::now(),
        };
        write_json(&session, None);
        self.session = Some(session);
        Ok(())
    }

    fn finish(&mut self) {
        let Some(mut s) = self.session.take() else { return };
        for t in &mut s.tails {
            s.bytes += t.pump();
        }
        self.shared.hud.fortnite_paused.store(false, Ordering::Relaxed);
        if s.reader.is_some() {
            info!("fortnite: left the game: {} eliminations, {} knocks, {} wins read", s.eliminations, s.knocked, s.victories);
        }
        if !s.pictures {
            return;
        }
        let ended = Stamp::now();
        write_json(&s, Some(&ended));
        info!(
            "HUD test: {} session ended: {} area pictures, {} screenshots, {} markers{}, {:.1} MB in {}",
            s.game,
            s.region_files,
            s.frames,
            s.markers,
            if s.events > 0 { format!(", {} game events", s.events) } else { String::new() },
            s.bytes as f64 / 1e6,
            s.dir.display()
        );
    }

    fn pump(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        for t in &mut s.tails {
            s.bytes += t.pump();
        }
        if s.pictures && s.last_json.elapsed() > Duration::from_secs(60) {
            s.last_json = Instant::now();
            write_json(s, None);
        }
    }

    fn frame(&mut self, f: &Frame) -> Result<()> {
        let Some(s) = self.session.as_mut().filter(|s| s.id == f.id) else { return Ok(()) };
        if !s.saving() && s.reader.is_none() {
            return Ok(());
        }
        let fits = fortnite::picture_fits(f.staged.width, f.staged.height);
        if s.reader.is_some() && !fits && !s.told_size {
            s.told_size = true;
            warning!("fortnite: the picture is {}x{}; its screen can only be read at 16:9, so no auto-clips here", f.staged.width, f.staged.height);
        }
        // Fortnite: the counter's two areas (saved with the others so its readings can be
        // checked), the Victory Royale banner's and the knock's (already in "center" and
        // "messages").
        let mut regions = f.regions.to_vec();
        if s.reader.is_some() {
            regions.extend([fortnite::digits_area(), fortnite::icon_area(), fortnite::victory_area(), fortnite::knock_area()]);
        }
        let (mut crops, full) = read_back(&f.staged, &regions, (f.periodic || f.asked) && s.saving())?;
        if let Some(reader) = s.reader.as_mut() {
            let area = |name: &str| crops.iter().find(|c| c.name == name).map(|c| fortnite::Area { bgr: &c.bgr, width: c.width as usize, height: c.height as usize });
            let areas = fortnite::Areas { digits: area("elims"), icon: area("elims-icon"), victory: area("victory"), knock: area("knock") };
            let notes = if fits && self.shared.hud.reading.load(Ordering::Relaxed) { reader.sample(&areas, f.clock) } else { Vec::new() };
            self.shared.hud.fortnite_paused.store(reader.paused(f.clock), Ordering::Relaxed);
            for note in notes {
                let send = |kind, time, count: u32| {
                    for _ in 0..count {
                        let _ = self.events.send((fortnite::GAME_NAME.to_string(), GameEvent::new(kind, time, "")));
                    }
                };
                let line = match note {
                    fortnite::Note::Counter(n) => {
                        info!("fortnite: the eliminations counter shows {n}");
                        format!("counter {n}")
                    }
                    fortnite::Note::Eliminations { by, to, time } => {
                        info!("fortnite: {by} elimination{} (the counter shows {to})", if by == 1 { "" } else { "s" });
                        s.eliminations += by as u64;
                        send(EventKind::Kill, time, by);
                        format!("+{by} elimination{}, counter {to}", if by == 1 { "" } else { "s" })
                    }
                    fortnite::Note::Knock(time) => {
                        s.knocked += 1;
                        send(EventKind::Knock, time, 1);
                        "knock".to_string()
                    }
                    fortnite::Note::Victory(time) => {
                        info!("fortnite: Victory Royale");
                        s.victories += 1;
                        send(EventKind::VictoryRoyale, time, 1);
                        "Victory Royale".to_string()
                    }
                    fortnite::Note::Odd(what) => {
                        info!("fortnite: odd reading: {what}");
                        format!("odd: {what}")
                    }
                    fortnite::Note::LeftOut(what) => {
                        info!("fortnite: left out a {what}");
                        format!("left out: {what}")
                    }
                    fortnite::Note::Paused { why, for_good } => {
                        let how_long = if for_good { "until you leave Fortnite" } else { "for 5 minutes" };
                        warning!("fortnite: auto-clips paused {how_long}: {}. Is this the default HUD at 16:9?", why.describe());
                        format!("paused {how_long}: {}", why.describe())
                    }
                    fortnite::Note::Resumed => {
                        info!("fortnite: auto-clips back on");
                        "back on".to_string()
                    }
                };
                if s.pictures {
                    append(&s.dir.join("eliminations.txt"), format!("{}  {line}\n", f.at.clock).as_bytes())?;
                }
            }
            crops.retain(|c| c.name != "victory" && c.name != "knock");
        }
        if !s.saving() {
            return Ok(());
        }
        s.screen.get_or_insert((f.staged.width, f.staged.height));
        let Some(wic) = &self.wic else { return Ok(()) };
        let q = s.config.jpeg_quality.clamp(0.3, 1.0);
        let in_window = s.window.as_ref().is_some_and(|w| f.at.unix_ms <= w.until_ms);
        let keep = f.at.unix_ms - s.last_kept_ms >= s.config.keep_every_ms as i64;
        if keep {
            s.last_kept_ms = f.at.unix_ms;
        }
        let mut written = 0u64;
        for c in &crops {
            let jpeg = Arc::new(jpeg(wic, c.width, c.height, &c.bgr, q)?);
            let file = format!("{}.jpg", f.at.file);
            if keep {
                written += save(&s.dir.join("regions").join(&c.name), &file, &jpeg)?;
                s.region_files += 1;
            }
            if in_window && let Some(w) = &s.window {
                written += save(&w.dir.join(&c.name), &file, &jpeg)?;
            }
            s.ring.push_back(RingEntry { unix_ms: f.at.unix_ms, region: c.name.clone(), file, jpeg });
        }
        let horizon = f.at.unix_ms - s.config.marker_before_s as i64 * 1000;
        while s.ring.front().is_some_and(|e| e.unix_ms < horizon) {
            s.ring.pop_front();
        }
        if let Some(full) = full {
            let quality = if f.asked || in_window { 0.95 } else { 0.85 };
            let jpeg = jpeg(wic, full.width, full.height, &full.bgr, quality)?;
            let file = format!("{}.jpg", f.at.file);
            if f.periodic {
                written += save(&s.dir.join("frames"), &file, &jpeg)?;
                s.frames += 1;
            }
            if (f.asked || in_window) && let Some(w) = &s.window {
                written += save(&w.dir, &format!("screen {file}"), &jpeg)?;
            }
        }
        s.bytes += written;
        let limit = (s.config.max_total_gb * 1e9) as u64;
        if self.root_bytes + s.bytes > limit {
            s.full_disk = true;
            warning!("HUD test: the HUD test folder reached {:.0} GB; nothing more is saved until old sessions are deleted", s.config.max_total_gb);
        }
        Ok(())
    }

    fn marker(&mut self, id: u64, at: Stamp, label: &str) -> Result<()> {
        let Some(s) = self.session.as_mut().filter(|s| s.id == id) else { return Ok(()) };
        s.markers += 1;
        append(&s.dir.join("markers.txt"), format!("{}  {label}\n", at.clock).as_bytes())?;
        let until = at.unix_ms + s.config.marker_after_s as i64 * 1000;
        if let Some(w) = s.window.as_mut().filter(|w| at.unix_ms <= w.until_ms) {
            // Close together: one folder that keeps going.
            w.until_ms = w.until_ms.max(until);
            return Ok(());
        }
        let dir = s.dir.join("markers").join(format!("{} {}", &at.file[..8], safe(label)));
        let mut written = 0;
        for e in &s.ring {
            written += save(&dir.join(&e.region), &e.file, &e.jpeg)?;
        }
        fs::create_dir_all(&dir)?;
        s.bytes += written;
        s.window = Some(Window { dir, until_ms: until });
        Ok(())
    }
}

/// Maps the copied frame (without blocking the video thread's GPU work), and
/// cuts out the areas and, when asked, the whole screen, as 24-bit BGR.
fn read_back(staged: &Staged, regions: &[Region], full: bool) -> Result<(Vec<Crop>, Option<Crop>)> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    let mut tries = 0;
    loop {
        match unsafe { staged.context.Map(&staged.texture, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut mapped)) } {
            Ok(()) => break,
            Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING && tries < 400 => {
                tries += 1;
                thread::sleep(Duration::from_millis(3));
            }
            Err(e) => return Err(e).context("reading the frame back"),
        }
    }
    let (w, h) = (staged.width as usize, staged.height as usize);
    let pitch = mapped.RowPitch as usize;
    let data = unsafe { std::slice::from_raw_parts(mapped.pData as *const u8, pitch * (h - 1) + w * 4) };
    let crops = regions.iter().filter_map(|r| crop(data, pitch, w, h, r)).collect();
    let whole = full.then(|| crop(data, pitch, w, h, &Region { name: "screen".into(), x: 0.0, y: 0.0, w: 1.0, h: 1.0, scale: 1.0 })).flatten();
    unsafe { staged.context.Unmap(&staged.texture, 0) };
    Ok((crops, whole))
}

fn crop(data: &[u8], pitch: usize, fw: usize, fh: usize, r: &Region) -> Option<Crop> {
    let px = |f: f32, size: usize| ((f.clamp(0.0, 1.0) * size as f32).round() as usize).min(size);
    let (x0, y0) = (px(r.x, fw), px(r.y, fh));
    let (x1, y1) = (px(r.x + r.w, fw), px(r.y + r.h, fh));
    if x1 < x0 + 8 || y1 < y0 + 8 {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    if r.scale < 0.75 {
        // Half size: each pixel is the average of a 2x2 block.
        let (hw, hh) = (w / 2, h / 2);
        let mut bgr = Vec::with_capacity(hw * hh * 3);
        for y in 0..hh {
            let a = &data[(y0 + 2 * y) * pitch + x0 * 4..];
            let b = &data[(y0 + 2 * y + 1) * pitch + x0 * 4..];
            for x in 0..hw {
                for c in 0..3 {
                    let sum = a[8 * x + c] as u32 + a[8 * x + 4 + c] as u32 + b[8 * x + c] as u32 + b[8 * x + 4 + c] as u32;
                    bgr.push((sum / 4) as u8);
                }
            }
        }
        return Some(Crop { name: r.name.clone(), width: hw as u32, height: hh as u32, bgr });
    }
    let mut bgr = Vec::with_capacity(w * h * 3);
    for y in y0..y1 {
        for p in data[y * pitch + x0 * 4..y * pitch + x1 * 4].chunks_exact(4) {
            bgr.extend_from_slice(&p[..3]);
        }
    }
    Some(Crop { name: r.name.clone(), width: w as u32, height: h as u32, bgr })
}

/// A JPEG through Windows' own encoder (WIC), at quality 0 to 1.
fn jpeg(wic: &IWICImagingFactory, width: u32, height: u32, bgr: &[u8], quality: f32) -> Result<Vec<u8>> {
    unsafe {
        let stream = SHCreateMemStream(None).context("no memory stream")?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        let mut options: Option<IPropertyBag2> = None;
        encoder.CreateNewFrame(&mut frame, &mut options)?;
        let frame = frame.context("no frame")?;
        let options = options.context("no encoder options")?;
        let name = w!("ImageQuality");
        let prop = PROPBAG2 { pstrName: PWSTR(name.as_ptr() as *mut u16), ..Default::default() };
        options.Write(1, &prop, &VARIANT::from(quality))?;
        frame.Initialize(&options)?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format)?;
        if format != GUID_WICPixelFormat24bppBGR {
            bail!("the JPEG encoder wants another pixel format");
        }
        frame.WritePixels(height, width * 3, bgr)?;
        frame.Commit()?;
        encoder.Commit()?;
        let mut size = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut size))?;
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let mut out = vec![0u8; size as usize];
        let mut read = 0u32;
        stream.Read(out.as_mut_ptr() as *mut c_void, size as u32, Some(&mut read)).ok()?;
        out.truncate(read as usize);
        Ok(out)
    }
}

fn save(dir: &Path, file: &str, bytes: &[u8]) -> Result<u64> {
    fs::create_dir_all(dir)?;
    fs::write(dir.join(file), bytes).with_context(|| format!("writing {}", dir.join(file).display()))?;
    Ok(bytes.len() as u64)
}

fn append(path: &Path, bytes: &[u8]) -> Result<()> {
    OpenOptions::new().create(true).append(true).open(path)?.write_all(bytes)?;
    Ok(())
}

/// A name that's safe as a folder name.
fn safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { ' ' }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(48).collect::<String>().trim_end_matches('.').to_string()
}

fn folder_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => folder_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Every display, with its size and whether Windows HDR is on.
fn displays() -> Vec<Value> {
    let mut out = Vec::new();
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return out };
    for a in 0..16u32 {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(a) }) else { break };
        for o in 0..16u32 {
            let Ok(output) = (unsafe { adapter.EnumOutputs(o) }) else { break };
            let Ok(output6) = output.cast::<IDXGIOutput6>() else { continue };
            let Ok(d) = (unsafe { output6.GetDesc1() }) else { continue };
            let name = String::from_utf16_lossy(&d.DeviceName).trim_end_matches('\0').to_string();
            let r = d.DesktopCoordinates;
            out.push(json!({
                "name": name,
                "size": format!("{}x{}", r.right - r.left, r.bottom - r.top),
                "hdr": d.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
                "bits_per_color": d.BitsPerColor,
                "max_nits": d.MaxLuminance,
            }));
        }
    }
    out
}

fn write_json(s: &Session, ended: Option<&Stamp>) {
    let value = json!({
        "game": s.game,
        "exe": s.exe.display().to_string(),
        "build": format!("Klipp {} HUD test build", env!("CARGO_PKG_VERSION")),
        "started": format!("{} {}", s.started.date, s.started.clock),
        "ended": ended.map(|e| format!("{} {}", e.date, e.clock)),
        "captured_size": s.screen.map(|(w, h)| format!("{w}x{h}")),
        "displays": s.displays,
        "area_pictures": s.region_files,
        "screenshots": s.frames,
        "markers": s.markers,
        "game_events": s.events,
        "eliminations_read": s.reader.as_ref().map(|_| s.eliminations),
        "knocks_read": s.reader.as_ref().map(|_| s.knocked),
        "victories_read": s.reader.as_ref().map(|_| s.victories),
        "megabytes": (s.bytes as f64 / 1e5).round() / 10.0,
        "stopped_at_size_limit": s.full_disk,
        "logs": s.tails.iter().map(|t| json!({ "what": t.what, "source": t.source.as_ref().map(|p| p.display().to_string()), "copied_bytes": t.copied })).collect::<Vec<_>>(),
        "areas": s.profile.regions,
    });
    let _ = fs::write(s.dir.join("session.json"), serde_json::to_vec_pretty(&value).unwrap_or_default());
}

// Logs written by the game

fn newest_combat_log(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().to_lowercase().starts_with("wowcombatlog"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

impl Tail {
    fn new(what: &'static str, source: Option<PathBuf>, scan: Option<PathBuf>, dest: PathBuf) -> Self {
        // Only what's written from now on.
        let offset = source.as_ref().and_then(|p| fs::metadata(p).ok()).map_or(0, |m| m.len());
        Self { what, source, offset, dest, copied: 0, scan, last_scan: Instant::now(), started: SystemTime::now() }
    }

    /// Copies what the game wrote since last time. Returns the bytes copied.
    fn pump(&mut self) -> u64 {
        if let Some(dir) = &self.scan
            && self.last_scan.elapsed() > Duration::from_secs(20)
        {
            self.last_scan = Instant::now();
            let newest = newest_combat_log(dir);
            if newest.is_some() && newest != self.source {
                // A new combat log (logging turned on, or a new day): from its start
                // if it began during the session.
                let fresh = newest.as_ref().and_then(|p| fs::metadata(p).ok()).and_then(|m| m.created().ok()).is_some_and(|c| c >= self.started);
                self.offset = if fresh { 0 } else { newest.as_ref().and_then(|p| fs::metadata(p).ok()).map_or(0, |m| m.len()) };
                self.source = newest;
            }
        }
        let Some(source) = &self.source else { return 0 };
        if self.copied >= TAIL_LIMIT {
            return 0;
        }
        let Ok(mut file) = File::open(source) else { return 0 };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // The game started a new file under the same name.
            self.offset = 0;
        }
        if len == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return 0;
        }
        let mut buf = Vec::new();
        let want = (len - self.offset).min(TAIL_LIMIT - self.copied);
        if file.take(want).read_to_end(&mut buf).is_err() {
            return 0;
        }
        if append(&self.dest, &buf).is_err() {
            return 0;
        }
        self.offset += buf.len() as u64;
        self.copied += buf.len() as u64;
        buf.len() as u64
    }
}

// League of Legends' live feed (Riot's Live Client Data API, local, during a match)

/// Polls the game's local event feed once a second while a League session
/// runs: every event goes to events.jsonl and becomes a marker.
fn league_watch(id: u64, alive: Arc<AtomicBool>, tx: SyncSender<Msg>, shared: Arc<Shared>, marker_frames: u32) {
    let mut last_id: i64 = -1;
    let mut greeted = false;
    while alive.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_secs(1));
        let Ok(body) = local_get(2999, w!("/liveclientdata/eventdata")) else { continue };
        // Joining a match that's already going: its earlier events are logged, not marked.
        let mark = greeted;
        if !greeted {
            greeted = true;
            info!("HUD test: League's live event feed answers");
            if let Ok(all) = local_get(2999, w!("/liveclientdata/allgamedata")) {
                let _ = tx.send(Msg::File { id, name: "league-start.json".into(), bytes: all });
            }
        }
        let Ok(json) = serde_json::from_slice::<Value>(&body) else { continue };
        for event in json["Events"].as_array().into_iter().flatten() {
            let event_id = event["EventID"].as_i64().unwrap_or(-1);
            if event_id <= last_id {
                continue;
            }
            last_id = event_id;
            let name = event["EventName"].as_str().unwrap_or("Event").to_string();
            let at = Stamp::now();
            let _ = tx.send(Msg::Event { id, at: at.clone(), event: event.clone() });
            if (mark || event_id == 0) && !matches!(name.as_str(), "GameStart" | "MinionsSpawning") {
                shared.hud.full_requests.fetch_add(marker_frames, Ordering::Relaxed);
                let _ = tx.send(Msg::Marker { id, at, label: format!("League {name}") });
            }
            if name == "GameEnd"
                && let Ok(all) = local_get(2999, w!("/liveclientdata/allgamedata"))
            {
                let _ = tx.send(Msg::File { id, name: "league-end.json".into(), bytes: all });
            }
        }
    }
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn handle(raw: *mut c_void, what: &str) -> Result<Handle> {
    if raw.is_null() {
        bail!("{what}: {}", windows::core::Error::from_thread());
    }
    Ok(Handle(raw))
}

/// An HTTPS GET to the game on this PC. Its certificate is self-signed, so
/// certificate errors are ignored; the address is 127.0.0.1 only.
fn local_get(port: u16, path: PCWSTR) -> Result<Vec<u8>> {
    unsafe {
        let session = handle(WinHttpOpen(w!("Klipp"), WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "opening WinHTTP")?;
        WinHttpSetTimeouts(session.0, 1000, 1000, 2000, 2000)?;
        let connect = handle(WinHttpConnect(session.0, w!("127.0.0.1"), port, 0), "connecting")?;
        let request = handle(
            WinHttpOpenRequest(connect.0, w!("GET"), path, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "opening the request",
        )?;
        // SECURITY_FLAG_IGNORE_UNKNOWN_CA | _CERT_WRONG_USAGE | _CERT_CN_INVALID | _CERT_DATE_INVALID
        let flags: u32 = 0x100 | 0x200 | 0x1000 | 0x2000;
        WinHttpSetOption(Some(request.0 as *const c_void), WINHTTP_OPTION_SECURITY_FLAGS, Some(&flags.to_le_bytes()))?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;
        let (mut status, mut len) = (0u32, 4u32);
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )?;
        if status != 200 {
            bail!("HTTP {status}");
        }
        let mut body = Vec::new();
        let mut chunk = [0u8; 16384];
        loop {
            let mut read = 0u32;
            WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > 8 << 20 {
                bail!("the answer is too large");
            }
        }
        Ok(body)
    }
}

// Text files

fn readme(config: &Config) -> String {
    format!(
        "Klipp {} HUD test build\r\n\
         \r\n\
         While one of these games is in front, Klipp saves pictures of its HUD here,\r\n\
         so automatic clips can be built for it:\r\n\
         {}\r\n\
         \r\n\
         Nothing touches the game: the pictures come from the screen Klipp records anyway.\r\n\
         Clips and the save key work as usual.\r\n\
         \r\n\
         Each game has a folder, and every time you play, a session folder with:\r\n\
         \x20 frames\\          a full screenshot every {} s\r\n\
         \x20 regions\\<area>\\  the HUD areas where events show up (kill feed, banners,\r\n\
         \x20                  counters), read twice a second and kept every {} s\r\n\
         \x20 markers\\         the {} s before and {} s after every press of your save key,\r\n\
         \x20                  twice a second, with full screenshots\r\n\
         \x20 markers.txt      every marker and its time\r\n\
         \x20 session.json     the screen size, whether HDR was on, and how much was saved\r\n\
         League of Legends: events.jsonl has every event from the game's live feed,\r\n\
         and each one is a marker too.\r\n\
         Minecraft (Java): latest.log, what the game logged while you played.\r\n\
         World of Warcraft: combat-log.txt, if combat logging is on (type /combatlog).\r\n\
         \r\n\
         Fortnite auto-clips (test): Klipp reads your eliminations counter (the number\r\n\
         next to the skull, top right) and saves a clip of every elimination; close ones\r\n\
         become one 2K, 3K... clip. A knock (KNOCKED! below the crosshair) is a clip\r\n\
         when no elimination is close to it, and a Victory Royale is a clip too (either\r\n\
         banner).\r\n\
         eliminations.txt lists what it read and when;\r\n\
         regions\\elims and elims-icon are what it looked at. Works with the default HUD\r\n\
         at 16:9 so far. Turn Fortnite's replays on (Settings, Game, Record Replays): they\r\n\
         let every elimination be checked afterwards.\r\n\
         \r\n\
         Press your save key whenever something happens: a kill, a multi-kill, a win, a\r\n\
         death, a goal. That labels the pictures, and you get the clip as always.\r\n\
         \r\n\
         notes.txt in each game's folder: write down your HUD settings once.\r\n\
         profiles.json: the games and areas; changes apply from the next session.\r\n\
         It stops saving at {} GB in all; delete old sessions to make room.\r\n\
         The next Klipp update replaces this test build.\r\n",
        env!("CARGO_PKG_VERSION"),
        config.games.iter().map(|g| format!("  - {}", g.name)).collect::<Vec<_>>().join("\r\n"),
        config.frame_every_s,
        config.keep_every_ms / 1000,
        config.marker_before_s,
        config.marker_after_s,
        config.max_total_gb,
    )
}

fn notes_template(game: &str) -> String {
    format!(
        "Your settings in {game} (once is enough; they change what the HUD looks like):\r\n\
         \r\n\
         Resolution and display mode in the game:\r\n\
         HUD scale or layout changes:\r\n\
         Colorblind mode:\r\n\
         Language:\r\n\
         HDR in the game (on/off):\r\n\
         \r\n\
         Anything else worth knowing:\r\n"
    )
}
