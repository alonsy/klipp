//! Counter-Strike 2 through Valve's Game State Integration (GSI).
//!
//! CS2 reads every `gamestate_integration_*.cfg` in its cfg folder when it
//! starts and POSTs JSON game state to the URL in it. We install our own file
//! (next to SteelSeries' and Logitech's, without touching them), listen on
//! localhost, and turn state changes into game events. GSI is an official
//! Valve feature, so it's safe with VAC.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::core::w;

use crate::clock;
use crate::events::{EventKind, GameEvent};
use crate::log::{info, warning};

pub const GAME_NAME: &str = "Counter-Strike 2";
const CFG_NAME: &str = "gamestate_integration_klipp.cfg";
const STEAM_APP_ID: &str = "730";
/// Deathmatch has no rounds; kills this close together count as a multi-kill.
const DEATHMATCH_STREAK: i64 = 10 * clock::SECOND;
/// Requests bigger than this aren't game state; drop the connection.
const MAX_BODY: usize = 1 << 20;
/// The timing log is rotated when it gets this big.
const MAX_LOG_BYTES: u64 = 20 * 1024 * 1024;

/// What CS2 is currently playing, for naming and grouping clips.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchContext {
    pub map: String,
    pub mode: String,
    /// When this was last seen (shared clock); 0 = never.
    pub seen: i64,
}

pub struct Cs2Integration {
    stop: Arc<AtomicBool>,
    port: u16,
    thread: Option<JoinHandle<()>>,
    pub context: Arc<Mutex<MatchContext>>,
}

impl Cs2Integration {
    /// Installs the GSI config if needed and starts listening. Returns `Ok(None)`
    /// if CS2 isn't installed.
    pub fn start(port: u16, debug_log: bool, events: Sender<GameEvent>) -> Result<Option<Self>> {
        let Some(cfg_dir) = find_cfg_dir() else {
            info!("cs2: not installed (no Steam library has app {STEAM_APP_ID}); auto-clips for CS2 are off");
            return Ok(None);
        };
        let token = install_cfg(&cfg_dir.join(CFG_NAME), port)?;
        let listener = TcpListener::bind(("127.0.0.1", port)).with_context(|| format!("listening on 127.0.0.1:{port}"))?;
        let log = if debug_log { GsiLog::open().map(Arc::new) } else { None };
        let stop = Arc::new(AtomicBool::new(false));
        let context = Arc::new(Mutex::new(MatchContext::default()));
        let tracker = Arc::new(Mutex::new(Tracker::default()));
        let thread = thread::Builder::new().name("cs2-gsi".into()).spawn({
            let (stop, context) = (stop.clone(), context.clone());
            move || {
                let connections = AtomicU64::new(0);
                for stream in listener.incoming() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let n = connections.fetch_add(1, Ordering::Relaxed);
                    // One short-lived thread per request, so a slow or idle
                    // connection can never hold up the next update.
                    let job = Job {
                        token: token.clone(),
                        tracker: tracker.clone(),
                        events: events.clone(),
                        context: context.clone(),
                        log: log.clone(),
                    };
                    let _ = thread::Builder::new().name("cs2-request".into()).spawn(move || {
                        if let Err(e) = job.handle(stream, n) {
                            warning!("cs2: request failed: {e:#}");
                        }
                    });
                }
            }
        })?;
        info!("cs2: listening for game state on 127.0.0.1:{port}{}", if debug_log { " (timing log on)" } else { "" });
        Ok(Some(Self { stop, port, thread: Some(thread), context }))
    }
}

impl Drop for Cs2Integration {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the blocking accept so the thread sees `stop`.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Job {
    token: String,
    tracker: Arc<Mutex<Tracker>>,
    events: Sender<GameEvent>,
    context: Arc<Mutex<MatchContext>>,
    log: Option<Arc<GsiLog>>,
}

impl Job {
    /// Reads one request, answers it and closes the connection, then turns the
    /// game state into events.
    fn handle(&self, mut stream: TcpStream, connection: u64) -> Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut buf = Vec::with_capacity(8 * 1024);
        let mut chunk = [0u8; 16 * 1024];
        let (body_start, body_len) = loop {
            if let Some(found) = complete_request(&buf)? {
                break found;
            }
            match stream.read(&mut chunk) {
                Ok(0) => return Ok(()),
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(()),
                Err(e) => return Err(e.into()),
            }
            if buf.len() > MAX_BODY + 16 * 1024 {
                bail!("request too large");
            }
        };
        let arrived = clock::now();
        let arrived_unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        // Answer and close straight away: CS2 waits for the answer before
        // sending its next update.
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        let _ = stream.shutdown(Shutdown::Both);

        let Ok(json) = serde_json::from_slice::<Value>(&buf[body_start..body_start + body_len]) else { return Ok(()) };
        if json["auth"]["token"].as_str() != Some(self.token.as_str()) {
            return Ok(());
        }
        if let Some(log) = &self.log {
            log.write(connection, body_len, &json, arrived_unix_ms);
        }
        let (events, context) = {
            let mut tracker = self.tracker.lock().unwrap();
            (tracker.update(&json, arrived), tracker.context.clone())
        };
        if !context.map.is_empty() {
            *self.context.lock().unwrap() = MatchContext { seen: arrived, ..context };
        }
        for e in events {
            let _ = self.events.send(e);
        }
        Ok(())
    }
}

/// If `buf` starts with a complete HTTP request, returns (body offset, body length).
fn complete_request(buf: &[u8]) -> Result<Option<(usize, usize)>> {
    let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else { return Ok(None) };
    let headers = std::str::from_utf8(&buf[..header_end]).context("request headers aren't text")?;
    let length = headers
        .lines()
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length")))
        .map(|(_, v)| v.trim().parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    if length > MAX_BODY {
        bail!("request body too large");
    }
    let body_start = header_end + 4;
    Ok((buf.len() >= body_start + length).then_some((body_start, length)))
}

/// One line per game-state update in %LOCALAPPDATA%\Klipp\cs2.log, to see how
/// late updates arrive compared with CS2's own timestamp.
struct GsiLog {
    path: PathBuf,
    file: Mutex<(File, u64)>,
}

impl GsiLog {
    fn open() -> Option<Self> {
        let path = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Klipp").join("cs2.log");
        fs::create_dir_all(path.parent()?).ok()?;
        if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES / 4) {
            let _ = fs::rename(&path, path.with_extension("old.log"));
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
        let size = file.metadata().map_or(0, |m| m.len());
        Some(Self { path, file: Mutex::new((file, size)) })
    }

    fn write(&self, connection: u64, bytes: usize, json: &Value, arrived_unix_ms: i64) {
        let t = unsafe { GetLocalTime() };
        // CS2's timestamp is whole seconds, so this "lag" reads 0–999 ms high.
        let lag = json["provider"]["timestamp"].as_i64().map(|ts| arrived_unix_ms - ts * 1000);
        let state = &json["player"]["state"];
        let own = json["player"]["steamid"].as_str() == json["provider"]["steamid"].as_str();
        let line = format!(
            "{:02}:{:02}:{:02}.{:03} conn={connection} bytes={bytes} lag={} own={own} phase={} kills={} hs={} hp={} weapon={}\n",
            t.wHour,
            t.wMinute,
            t.wSecond,
            t.wMilliseconds,
            lag.map_or("?".to_string(), |l| format!("{l}ms")),
            json["round"]["phase"].as_str().unwrap_or("-"),
            state["round_kills"].as_u64().map_or("-".to_string(), |v| v.to_string()),
            state["round_killhs"].as_u64().map_or("-".to_string(), |v| v.to_string()),
            state["health"].as_u64().map_or("-".to_string(), |v| v.to_string()),
            parse_weapons(&json["player"]).iter().find(|w| w.active).map_or("-", |w| w.name.as_str()),
        );
        let mut guard = self.file.lock().unwrap();
        if guard.1 > MAX_LOG_BYTES {
            let _ = fs::rename(&self.path, self.path.with_extension("old.log"));
            if let Ok(f) = OpenOptions::new().create(true).append(true).open(&self.path) {
                *guard = (f, 0);
            }
        }
        if guard.0.write_all(line.as_bytes()).is_ok() {
            guard.1 += line.len() as u64;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Weapon {
    name: String,
    kind: String,
    ammo: Option<u64>,
    active: bool,
}

fn parse_weapons(player: &Value) -> Vec<Weapon> {
    player["weapons"]
        .as_object()
        .map(|all| {
            all.values()
                .map(|w| Weapon {
                    name: w["name"].as_str().unwrap_or_default().trim_start_matches("weapon_").to_string(),
                    kind: w["type"].as_str().unwrap_or_default().to_string(),
                    ammo: w["ammo_clip"].as_u64(),
                    active: matches!(w["state"].as_str(), Some("active" | "reloading")),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The weapon a kill was made with. A weapon whose magazine dropped since the
/// last update fired the shot; otherwise it's what was in hand *before* this
/// update, since players often switch to the knife right after a kill.
fn kill_weapon<'a>(prev: &'a [Weapon], cur: &'a [Weapon]) -> Option<&'a Weapon> {
    let fired = cur.iter().find(|w| {
        let before = prev.iter().find(|p| p.name == w.name).and_then(|p| p.ammo);
        matches!((w.ammo, before), (Some(now), Some(before)) if now < before)
    });
    fired.or_else(|| prev.iter().find(|w| w.active)).or_else(|| cur.iter().find(|w| w.active))
}

/// What we remember about the local player between updates.
#[derive(Clone, Debug, PartialEq)]
struct PlayerState {
    map: String,
    round_kills: u64,
    round_headshots: u64,
    health: u64,
    mvps: u64,
    weapons: Vec<Weapon>,
}

/// Turns successive game-state snapshots into events.
#[derive(Default)]
pub struct Tracker {
    last: Option<PlayerState>,
    /// Recent kill times (deathmatch).
    streak: VecDeque<i64>,
    /// Our team ("CT" or "T"), from our own snapshots.
    team: String,
    round_phase: String,
    bomb: String,
    context: MatchContext,
}

impl Tracker {
    pub fn update(&mut self, json: &Value, now: i64) -> Vec<GameEvent> {
        let mut events = Vec::new();
        if let Some(map) = json["map"]["name"].as_str() {
            self.context.map = map.to_string();
            self.context.mode = json["map"]["mode"].as_str().unwrap_or_default().to_string();
        }
        self.round_events(json, now, &mut events);
        self.player_events(json, now, &mut events);
        events
    }

    /// Round results and defuses. These come from the round itself, so they
    /// count even while you're dead and spectating.
    fn round_events(&mut self, json: &Value, now: i64, events: &mut Vec<GameEvent>) {
        let round = &json["round"];
        if let Some(phase) = round["phase"].as_str() {
            if phase == "over" && self.round_phase != "over" && !self.team.is_empty() {
                match round["win_team"].as_str() {
                    Some(winner) if winner == self.team => events.push(GameEvent::new(EventKind::RoundWin, now, "")),
                    Some(_) => events.push(GameEvent::new(EventKind::RoundLoss, now, "")),
                    None => {}
                }
            }
            self.round_phase = phase.to_string();
        }
        let bomb = round["bomb"].as_str().unwrap_or_default();
        if bomb == "defused" && self.bomb != "defused" && self.team == "CT" {
            events.push(GameEvent::new(EventKind::BombDefused, now, ""));
        }
        if !round.is_null() {
            self.bomb = bomb.to_string();
        }
    }

    fn player_events(&mut self, json: &Value, now: i64, events: &mut Vec<GameEvent>) {
        let provider = json["provider"]["steamid"].as_str();
        let player = &json["player"];
        // After you die, `player` is whoever you're spectating. Only our own stats count.
        if provider.is_none() || player["steamid"].as_str() != provider || player["state"].is_null() {
            return;
        }
        if let Some(team) = player["team"].as_str() {
            self.team = team.to_string();
        }
        let state = &player["state"];
        let num = |v: &Value| v.as_u64().unwrap_or(0);
        let cur = PlayerState {
            map: self.context.map.clone(),
            round_kills: num(&state["round_kills"]),
            round_headshots: num(&state["round_killhs"]),
            health: num(&state["health"]),
            mvps: num(&player["match_stats"]["mvps"]),
            weapons: parse_weapons(player),
        };
        let deathmatch = json["map"]["mode"].as_str() == Some("deathmatch");

        match &self.last {
            // Same map and round (kills only go down when a new round starts).
            Some(prev) if prev.map == cur.map && cur.round_kills >= prev.round_kills => {
                let new_kills = cur.round_kills - prev.round_kills;
                let new_headshots = cur.round_headshots.saturating_sub(prev.round_headshots).min(new_kills);
                let weapon = kill_weapon(&prev.weapons, &cur.weapons);
                let detail = weapon.map_or(String::new(), |w| w.name.clone());
                for k in 0..new_kills {
                    let kind = if k < new_headshots { EventKind::Headshot } else { EventKind::Kill };
                    events.push(GameEvent::new(kind, now, detail.clone()));
                    match weapon {
                        Some(w) if w.kind == "Knife" => events.push(GameEvent::new(EventKind::KnifeKill, now, detail.clone())),
                        Some(w) if w.name == "taser" => events.push(GameEvent::new(EventKind::ZeusKill, now, detail.clone())),
                        _ => {}
                    }
                }
                if new_kills > 0 {
                    if deathmatch {
                        self.streak.retain(|&t| now - t <= DEATHMATCH_STREAK);
                        self.streak.extend(std::iter::repeat_n(now, new_kills as usize));
                        if self.streak.len() >= 2 {
                            let n = self.streak.len().min(u8::MAX as usize) as u8;
                            events.push(GameEvent::new(EventKind::MultiKill(n), now, detail.clone()));
                        }
                    } else {
                        for n in prev.round_kills + 1..=cur.round_kills {
                            if n >= 2 {
                                events.push(GameEvent::new(EventKind::MultiKill(n.min(u8::MAX as u64) as u8), now, detail.clone()));
                            }
                        }
                    }
                }
                if prev.health > 0 && cur.health == 0 {
                    events.push(GameEvent::new(EventKind::Death, now, ""));
                }
                if cur.mvps > prev.mvps {
                    events.push(GameEvent::new(EventKind::Mvp, now, ""));
                }
            }
            // First update, new map or new round: this is the new baseline.
            _ => {
                if self.last.as_ref().is_none_or(|p| p.map != cur.map) {
                    self.streak.clear();
                }
            }
        }
        self.last = Some(cur);
    }
}

/// CS2's cfg folder in whichever Steam library has it installed.
fn find_cfg_dir() -> Option<PathBuf> {
    let steam = steam_path()?;
    let vdf = fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")).ok()?;
    let mut libraries = vec![steam];
    libraries.extend(vdf_paths(&vdf));
    libraries.into_iter().find_map(|lib| {
        let apps = lib.join("steamapps");
        apps.join(format!("appmanifest_{STEAM_APP_ID}.acf"))
            .exists()
            .then(|| apps.join(r"common\Counter-Strike Global Offensive\game\csgo\cfg"))
            .filter(|dir| dir.is_dir())
    })
}

fn steam_path() -> Option<PathBuf> {
    let mut buf = [0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Valve\Steam"),
            w!("SteamPath"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    // Steam stores it with forward slashes.
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len]).replace('/', r"\")))
}

/// The `"path"` values in Steam's libraryfolders.vdf.
fn vdf_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);
            (quoted.next()? == "path").then(|| PathBuf::from(quoted.next().unwrap_or_default().replace(r"\\", r"\")))
        })
        .collect()
}

/// Writes our GSI config (keeping an existing token) and returns the token.
fn install_cfg(path: &Path, port: u16) -> Result<String> {
    let existing = fs::read_to_string(path).ok();
    let token = match existing.as_deref().and_then(|t| cfg_value(t, "token")) {
        Some(t) => t,
        None => random_token()?,
    };
    let wanted = cfg_text(port, &token);
    if existing.as_deref() != Some(wanted.as_str()) {
        fs::write(path, &wanted).with_context(|| format!("writing {}", path.display()))?;
        info!("cs2: installed {} (restart CS2 if it's running so it picks it up)", path.display());
    }
    Ok(token)
}

fn uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

/// Valve's sample values, except no buffering or throttling: every change is
/// sent at once, and we answer within milliseconds.
fn cfg_text(port: u16, token: &str) -> String {
    format!(
        "\"Klipp\"\n{{\n \"uri\" \"{}\"\n \"timeout\" \"5.0\"\n \"buffer\" \"0.0\"\n \"throttle\" \"0.0\"\n \"heartbeat\" \"10.0\"\n \
         \"auth\"\n {{\n   \"token\" \"{token}\"\n }}\n \"data\"\n {{\n   \"provider\" \"1\"\n   \"map\" \"1\"\n   \"round\" \"1\"\n   \
         \"player_id\" \"1\"\n   \"player_state\" \"1\"\n   \"player_weapons\" \"1\"\n   \"player_match_stats\" \"1\"\n }}\n}}\n",
        uri(port)
    )
}

/// Value of the first `"key" "value"` line with this key.
fn cfg_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut quoted = line.split('"').skip(1).step_by(2);
        (quoted.next()? == key).then(|| quoted.next().map(str::to_string)).flatten()
    })
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 16];
    unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG).ok()? };
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
