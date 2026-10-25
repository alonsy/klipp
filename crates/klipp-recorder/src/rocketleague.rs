//! Rocket League through Psyonix's Stats API.
//!
//! During a match Rocket League streams JSON to whoever connects to a local
//! TCP port (49123): the match's state several times a second, and each
//! event the moment it happens (a goal, a save, a demolition). It's official
//! and Klipp reads nothing from the game itself, so Easy Anti-Cheat has
//! nothing to object to. The game only streams when `PacketSendRate` in its
//! TAStatsAPI.ini is above 0: Klipp sets it, and checks it again whenever the
//! game closes, since a patch (v2.72) reset it once.
//!
//! The stream never says which car is yours. While you play, the camera
//! follows your car, so you're the car it followed most this match.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::fs;
use std::io::{ErrorKind, Read};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use crate::autoclip::Rules;
use crate::clock::{self, SECOND};
use crate::events::{EventKind, GameEvent};
use crate::log::{info, warning};
use crate::settings;

pub const GAME_NAME: &str = "Rocket League";
const EXE: &str = "rocketleague.exe";
const SECTION: &str = "TAGame.MatchStatsExporter_TA";
const DEFAULT_PORT: u16 = 49123;
/// State updates per second Klipp asks for: plenty to tell whose car the
/// camera follows, and little to read. Events come at once whatever the rate.
/// A rate that's already set (for an overlay) is kept.
const PACKET_SEND_RATE: u32 = 10;
/// How often Klipp looks for the game.
const LOOK: Duration = Duration::from_secs(3);
/// While the game is closed, its ini is checked this often (an update may
/// reset it); and once this long after the game starts, since a patched game
/// rewrites it as it launches.
const INI_CHECK: Duration = Duration::from_secs(60);
/// The game running this long without sending anything gets a warning.
const SILENT_WARNING: Duration = Duration::from_secs(10 * 60);
/// Between connection attempts while the game runs (the stream may only be
/// there during a match).
const RETRY_MIN: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(5);
/// Updates in which the camera followed a car before it counts as yours.
const MIN_FOLLOWED: u32 = 10;
/// Stat feed entries about one moment come close together (an aerial goal is
/// the goal plus an "AerialGoal"), so events wait this long before they go out.
const SETTLE: i64 = SECOND;
/// More than this without a complete message isn't the stream: start over.
const MAX_BUFFER: usize = 1 << 20;
/// Stat feed names that describe a goal, most notable first, and what the clip says.
const GOAL_STYLES: &[(&str, &str)] = &[
    ("OvertimeGoal", "overtime"),
    ("BicycleGoal", "bicycle"),
    ("AerialGoal", "aerial"),
    ("BackwardsGoal", "backwards"),
    ("LongGoal", "long"),
    ("TurtleGoal", "turtle"),
    ("PoolShot", "pool"),
    ("HoopsSwishGoal", "swish"),
];

/// Rocket League's rules, from its section of the settings. Events wait
/// `SETTLE` before they go out, so chains wait that much longer.
pub fn rules(base: &Rules, s: &settings::RocketLeague) -> Rules {
    Rules {
        headshot: false,
        multi_kill_min: 0,
        any_kill: false,
        knife_or_zeus_kill: false,
        death: false,
        mvp: false,
        victory: false,
        first_blood: false,
        steal: false,
        objective: false,
        team_ace: false,
        goal: s.goal,
        assist: s.assist,
        save: s.save,
        epic_save: s.epic_save,
        demolition: s.demolition,
        lateness: SETTLE + SECOND / 2,
        ..base.clone()
    }
}

/// Rocket League has run on this PC (it made its settings folder).
pub fn installed() -> bool {
    ini_path().is_some()
}

pub struct RocketLeagueIntegration {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RocketLeagueIntegration {
    /// Turns the Stats API on if needed and starts watching for the game.
    /// Returns `Ok(None)` if Rocket League has never run here.
    pub fn start(events: Sender<(String, GameEvent)>) -> Result<Option<Self>> {
        let Some(ini) = ini_path() else {
            info!("rocket league: not installed (no settings folder in Documents\\My Games); auto-clips for Rocket League are off");
            return Ok(None);
        };
        turn_on(&ini, crate::games::running(EXE));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new().name("rocket-league".into()).spawn({
            let stop = stop.clone();
            move || watch(&stop, &ini, &events)
        })?;
        info!("rocket league: watching for Rocket League matches");
        Ok(Some(Self { stop, thread: Some(thread) }))
    }
}

impl Drop for RocketLeagueIntegration {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Sleeps in short steps, so quitting never waits long. False once stopped.
fn nap(stop: &AtomicBool, time: Duration) -> bool {
    let until = Instant::now() + time;
    while !stop.load(Ordering::Relaxed) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        thread::sleep(left.min(Duration::from_millis(100)));
    }
    false
}

fn watch(stop: &AtomicBool, ini: &Path, events: &Sender<(String, GameEvent)>) {
    let mut checked = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if crate::games::running(EXE) {
            // It read its ini at launch: if the rate was 0, this run sends nothing.
            turn_on(ini, true);
            play(stop, ini, events);
            // The game may have written its ini as it closed.
            turn_on(ini, false);
            checked = Instant::now();
        } else if checked.elapsed() >= INI_CHECK {
            turn_on(ini, false);
            checked = Instant::now();
        }
        if !nap(stop, LOOK) {
            return;
        }
    }
}

/// While the game runs: reads its stream, connecting again whenever it isn't
/// there or drops, and sends what it means.
fn play(stop: &AtomicBool, ini: &Path, events: &Sender<(String, GameEvent)>) {
    let port = read_ini(ini).map_or(DEFAULT_PORT, |text| stats_api(&text).1);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    info!("rocket league: the game is running; reading its Stats API on 127.0.0.1:{port}");
    let started = Instant::now();
    let (mut heard, mut warned, mut rechecked) = (false, false, false);
    let mut looked = Instant::now();
    let mut retry = RETRY_MIN;
    let mut tracker = Tracker::default();
    let send = |found: Vec<GameEvent>| {
        for e in found {
            let _ = events.send((GAME_NAME.to_string(), e));
        }
    };
    'game: while !stop.load(Ordering::Relaxed) {
        if looked.elapsed() >= LOOK {
            looked = Instant::now();
            if !crate::games::running(EXE) {
                break;
            }
        }
        if !rechecked && started.elapsed() >= INI_CHECK {
            rechecked = true;
            turn_on(ini, true);
        }
        if !heard && !warned && started.elapsed() >= SILENT_WARNING {
            warned = true;
            warning!(
                "rocket league: the game has run for {} minutes and its Stats API sent nothing. If you played a match, a patch may have reset PacketSendRate in {}; Klipp sets it again, so restart Rocket League",
                SILENT_WARNING.as_secs() / 60,
                ini.display()
            );
            turn_on(ini, true);
        }
        let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(500)) else {
            send(tracker.flush(clock::now()));
            if !nap(stop, retry) {
                break;
            }
            retry = (retry * 2).min(RETRY_MAX);
            continue;
        };
        retry = RETRY_MIN;
        let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
        let mut messages = JsonStream::default();
        let mut buf = vec![0u8; 64 * 1024];
        while !stop.load(Ordering::Relaxed) {
            if looked.elapsed() >= LOOK {
                looked = Instant::now();
                if !crate::games::running(EXE) {
                    break 'game;
                }
            }
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    for msg in messages.feed(&buf[..n]) {
                        if !heard {
                            heard = true;
                            info!("rocket league: the Stats API is sending");
                        }
                        send(tracker.on_message(&msg, clock::now()));
                        if tracker.match_started {
                            info!("rocket league: a match started");
                            crate::overlay::post(crate::overlay::Indicator::MatchStarted);
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
            for note in std::mem::take(&mut tracker.notes) {
                info!("rocket league: {note}");
            }
            send(tracker.flush(clock::now()));
        }
    }
    send(tracker.finish());
    if tracker.goals_seen > 0 && !tracker.identified {
        warning!("rocket league: the game closed; {} goals were scored but the camera never followed one car long enough to tell which is yours", tracker.goals_seen);
    } else {
        info!("rocket league: the game closed ({} goals seen, {} yours)", tracker.goals_seen, tracker.goals_mine);
    }
}

/// Splits the stream into its JSON messages. They come one after another
/// with nothing between them, and a read can end anywhere, even mid-message.
#[derive(Default)]
struct JsonStream {
    buf: Vec<u8>,
}

impl JsonStream {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Value> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut used = 0;
        // Anything between messages (whitespace, the rest of a broken one) is skipped.
        while let Some(open) = self.buf[used..].iter().position(|&b| b == b'{').map(|i| used + i) {
            let Some(len) = object_len(&self.buf[open..]) else {
                used = open;
                break;
            };
            if let Ok(v) = serde_json::from_slice(&self.buf[open..open + len]) {
                out.push(v);
            }
            used = open + len;
        }
        if !self.buf[used..].contains(&b'{') {
            used = self.buf.len();
        }
        self.buf.drain(..used);
        if self.buf.len() > MAX_BUFFER {
            self.buf.clear();
        }
        out
    }
}

/// The length of the JSON object `bytes` starts with, once all of it is
/// there: braces are counted outside strings.
fn object_len(bytes: &[u8]) -> Option<usize> {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The game's own view of a player in events: `{ Name, Shortcut, TeamNum }`.
fn same_player(p: &Value, (name, team): &(String, i64)) -> bool {
    p["Name"].as_str() == Some(name.as_str()) && p["TeamNum"].as_i64() == Some(*team)
}

/// Turns the stream's messages into events.
#[derive(Default)]
struct Tracker {
    /// Updates in which the camera followed each car this match: ((name, team), count).
    followed: Vec<((String, i64), u32)>,
    /// Updates this match in which every car's boost showed (spectating: a
    /// player only sees their own team's) and in which it didn't.
    views: (u32, u32),
    overtime: bool,
    /// Events waiting `SETTLE`, in time order.
    pending: Vec<GameEvent>,
    /// A goal style from the stat feed that came before its goal.
    style: Option<(&'static str, i64)>,
    /// Your last epic save: the plain save the stat feed may add is left out.
    epic_save: Option<i64>,
    /// The last message started a match (the "Auto-clips ON" indicator's cue).
    match_started: bool,
    /// Lines for the log, about who you are.
    notes: Vec<String>,
    announced: bool,
    /// For the line when the game closes.
    identified: bool,
    goals_seen: u32,
    goals_mine: u32,
}

impl Tracker {
    /// You: the car the camera followed most this match, once it has for a
    /// moment. Nobody while spectating.
    fn me(&self) -> Option<&(String, i64)> {
        if self.views.0 > self.views.1 {
            return None;
        }
        self.followed.iter().max_by_key(|(_, n)| *n).filter(|(_, n)| *n >= MIN_FOLLOWED).map(|(car, _)| car)
    }

    fn is_me(&self, p: &Value) -> bool {
        self.me().is_some_and(|me| same_player(p, me))
    }

    fn on_message(&mut self, msg: &Value, now: i64) -> Vec<GameEvent> {
        self.match_started = false;
        // Some versions send `Data` as a string of JSON.
        let data = match &msg["Data"] {
            Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
            d => d.clone(),
        };
        let mut out = Vec::new();
        match msg["Event"].as_str().unwrap_or_default() {
            "UpdateState" => self.update(&data),
            "MatchCreated" | "MatchDestroyed" => {
                out = self.finish();
                *self = Tracker {
                    identified: self.identified,
                    goals_seen: self.goals_seen,
                    goals_mine: self.goals_mine,
                    notes: std::mem::take(&mut self.notes),
                    ..Tracker::default()
                };
            }
            "MatchInitialized" => self.match_started = true,
            "ClockUpdatedSeconds" => self.overtime = data["bOvertime"].as_bool().unwrap_or(self.overtime),
            "GoalScored" => self.goal(&data, now),
            "StatfeedEvent" => self.statfeed(&data, now),
            "MatchEnded" => {
                let winner = data["WinnerTeamNum"].as_i64().filter(|&w| w >= 0);
                if let (Some(winner), Some((_, team))) = (winner, self.me()) {
                    let kind = if winner == *team { EventKind::MatchWin } else { EventKind::MatchLoss };
                    self.pending.push(GameEvent::new(kind, now, ""));
                }
            }
            _ => {}
        }
        out
    }

    fn update(&mut self, data: &Value) {
        let game = &data["Game"];
        if let Some(overtime) = game["bOvertime"].as_bool() {
            self.overtime = overtime;
        }
        if let Some(players) = data["Players"].as_array().filter(|p| !p.is_empty()) {
            let teams: BTreeSet<i64> = players.iter().filter_map(|p| p["TeamNum"].as_i64()).collect();
            if teams.len() > 1 && players.iter().all(|p| !p["Boost"].is_null()) {
                self.views.0 += 1;
            } else {
                self.views.1 += 1;
            }
        }
        // Goal replays show other cars; they don't count.
        let target = &game["Target"];
        if game["bHasTarget"].as_bool() == Some(true)
            && game["bReplay"].as_bool() != Some(true)
            && let (Some(name), Some(team)) = (target["Name"].as_str().filter(|n| !n.is_empty()), target["TeamNum"].as_i64())
        {
            match self.followed.iter_mut().find(|(car, _)| car.0 == name && car.1 == team) {
                Some((_, n)) => *n += 1,
                None => self.followed.push(((name.to_string(), team), 1)),
            }
        }
        if !self.announced {
            if self.views.0 > self.views.1 && self.views.0 >= MIN_FOLLOWED {
                self.announced = true;
                self.notes.push("spectating (every car's boost shows); no auto-clips from this match".into());
            } else if let Some(team) = self.me().map(|(_, team)| *team) {
                self.announced = true;
                self.identified = true;
                let side = if team == 0 { "blue" } else { "orange" };
                self.notes.push(format!("you're the {side} car the camera follows"));
            }
        }
    }

    fn goal(&mut self, data: &Value, now: i64) {
        self.goals_seen += 1;
        if self.is_me(&data["Scorer"]) {
            self.goals_mine += 1;
            let mut detail = if self.overtime { "overtime" } else { "" };
            if let Some((style, at)) = self.style.take()
                && now - at <= SETTLE
                && detail.is_empty()
            {
                detail = style;
            }
            self.pending.push(GameEvent::new(EventKind::Goal, now, detail));
        }
        if self.is_me(&data["Assister"]) {
            self.pending.push(GameEvent::new(EventKind::Assist, now, ""));
        }
    }

    /// The stat feed's entries for you (`MainTarget`). Its names are the
    /// game's: "Demolish" is one car, "Demolition" is every opponent's.
    fn statfeed(&mut self, data: &Value, now: i64) {
        if !self.is_me(&data["MainTarget"]) {
            return;
        }
        let name = data["EventName"].as_str().unwrap_or_default();
        let recent = |t: i64| now - t <= SETTLE;
        match name {
            "Save" if !self.epic_save.is_some_and(recent) => self.pending.push(GameEvent::new(EventKind::Save, now, "")),
            "EpicSave" => {
                self.pending.retain(|e| !(e.kind == EventKind::Save && recent(e.time)));
                self.epic_save = Some(now);
                self.pending.push(GameEvent::new(EventKind::EpicSave, now, ""));
            }
            "Demolish" => self.pending.push(GameEvent::new(EventKind::Demolition, now, "")),
            "Demolition" => self.pending.push(GameEvent::new(EventKind::Demolition, now, "extermination")),
            // Not a goal of yours after all.
            "OwnGoal" => self.pending.retain(|e| !(e.kind == EventKind::Goal && recent(e.time))),
            _ => {
                let Some(rank) = GOAL_STYLES.iter().position(|(n, _)| *n == name) else { return };
                let style = GOAL_STYLES[rank].1;
                let better = |detail: &str| GOAL_STYLES.iter().position(|(_, s)| *s == detail).is_none_or(|r| rank < r);
                match self.pending.iter_mut().rev().find(|e| e.kind == EventKind::Goal && recent(e.time)) {
                    Some(goal) if better(&goal.detail) => goal.detail = style.to_string(),
                    Some(_) => {}
                    None => self.style = Some((style, now)),
                }
            }
        }
    }

    /// Events that have waited long enough, oldest first.
    fn flush(&mut self, now: i64) -> Vec<GameEvent> {
        let ready = self.pending.iter().take_while(|e| now - e.time >= SETTLE).count();
        self.pending.drain(..ready).collect()
    }

    /// Everything still waiting (the match or the game is over).
    fn finish(&mut self) -> Vec<GameEvent> {
        std::mem::take(&mut self.pending)
    }
}

/// `Documents\My Games\Rocket League\TAGame\Config\TAStatsAPI.ini`, which the
/// game reads when it starts (the one in its install folder is only the
/// template). `None` if the game has never run here.
fn ini_path() -> Option<PathBuf> {
    let config = documents()?.join(r"My Games\Rocket League\TAGame\Config");
    config.is_dir().then(|| config.join("TAStatsAPI.ini"))
}

/// The Documents folder, wherever it was moved (OneDrive moves it).
fn documents() -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string();
        CoTaskMemFree(Some(path.0 as *const c_void));
        text.ok().map(PathBuf::from)
    }
}

/// Makes sure the Stats API is on in `ini`, saying so in the log if it had to
/// change it. A failure is only a warning: a rate set by hand may still work.
fn turn_on(ini: &Path, game_running: bool) {
    match enable_file(ini) {
        Ok(false) => {}
        Ok(true) if game_running => info!(
            "rocket league: turned on the Stats API in {} (PacketSendRate={PACKET_SEND_RATE}); restart Rocket League once so it reads it",
            ini.display()
        ),
        Ok(true) => info!("rocket league: turned on the Stats API in {} (PacketSendRate={PACKET_SEND_RATE})", ini.display()),
        Err(e) => warning!("rocket league: couldn't turn on the Stats API: {e:#}"),
    }
}

/// Unreal writes its ini files as UTF-16 or UTF-8, with or without a BOM;
/// a changed file keeps its encoding.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16,
}

fn read_ini(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|bytes| decode(&bytes).0)
}

fn decode(bytes: &[u8]) -> (String, Encoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
        (String::from_utf16_lossy(&units), Encoding::Utf16)
    } else if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (String::from_utf8_lossy(rest).into_owned(), Encoding::Utf8Bom)
    } else {
        (String::from_utf8_lossy(bytes).into_owned(), Encoding::Utf8)
    }
}

fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => [&[0xEF, 0xBB, 0xBF], text.as_bytes()].concat(),
        Encoding::Utf16 => [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect(),
    }
}

/// Turns the Stats API on in the ini at `path`, creating it if needed. True if it changed.
fn enable_file(path: &Path) -> Result<bool> {
    let (text, encoding) = match fs::read(path) {
        Ok(bytes) => decode(&bytes),
        Err(e) if e.kind() == ErrorKind::NotFound => (String::new(), Encoding::Utf8),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let (Some(new), _) = stats_api(&text) else { return Ok(false) };
    fs::write(path, encode(&new, encoding)).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// The ini with the Stats API on (`PacketSendRate` above 0 in its section,
/// adding the key or the section if missing; everything else stays), or
/// `None` if it's on already. Also the port the game streams on.
fn stats_api(text: &str) -> (Option<String>, u16) {
    let newline = if text.contains("\r\n") || text.is_empty() { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.lines().collect();
    fn header(line: &str) -> Option<&str> {
        line.trim().strip_prefix('[').and_then(|l| l.strip_suffix(']')).map(str::trim)
    }
    let value = |l: &str, key: &str| l.split_once('=').filter(|(k, _)| k.trim().eq_ignore_ascii_case(key)).map(|(_, v)| v.trim().to_string());
    let rate_line = format!("PacketSendRate={PACKET_SEND_RATE}");
    let Some(start) = lines.iter().position(|l| header(l).is_some_and(|h| h.eq_ignore_ascii_case(SECTION))) else {
        let mut out = text.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(newline);
        }
        out.push_str(&format!("[{SECTION}]{newline}{rate_line}{newline}"));
        return (Some(out), DEFAULT_PORT);
    };
    let end = lines[start + 1..].iter().position(|l| header(l).is_some()).map_or(lines.len(), |i| start + 1 + i);
    let section = &lines[start + 1..end];
    let port = section.iter().find_map(|l| value(l, "Port")).and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    let rate = section.iter().position(|l| value(l, "PacketSendRate").is_some());
    let mut lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match rate {
        Some(i) if value(section[i], "PacketSendRate").and_then(|v| v.parse::<f64>().ok()).is_some_and(|r| r > 0.0) => return (None, port),
        Some(i) => lines[start + 1 + i] = rate_line,
        None => lines.insert(start + 1, rate_line),
    }
    let mut out = lines.join(newline);
    if text.ends_with('\n') {
        out.push_str(newline);
    }
    (Some(out), port)
}
