//! Minecraft (Java Edition) through its own log.
//!
//! The game writes its chat to `.minecraft\logs\latest.log` as it plays, and
//! the moments worth a clip are chat messages: "Alex was slain by Steve",
//! "Steve has made the advancement [Diamonds!]". Klipp reads the end of that
//! file as it grows, the way it reads any text file; nothing touches the game.
//!
//! The log says whose game it is once, when the game starts ("Setting user:
//! Steve"). Klipp started in the middle of a session finds that line by
//! reading the file once, then follows only what comes after.
//!
//! Bedrock Edition (Minecraft for Windows) keeps no such log, so it isn't
//! covered. Servers that rewrite chat (plugins, other languages) may not match.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;

use crate::autoclip::Rules;
use crate::clock::{self, SECOND};
use crate::events::{EventKind, GameEvent};
use crate::log::{info, warning};
use crate::settings::{self, MultiKillMode};

pub const GAME_NAME: &str = "Minecraft";
/// How often the log is looked at once it exists. One metadata read when nothing changed.
const POLL: Duration = Duration::from_millis(250);
/// Reading more than this at once means something else is filling the log
/// (a mod's debug output): the rest is skipped rather than held in memory.
const MAX_READ: u64 = 4 << 20;
/// Singleplayer logs each death twice (the built-in server, then the chat):
/// the same message this close together is one moment.
const SAME_MOMENT: i64 = 3 * SECOND;
/// Messages remembered for that: one explosion can kill several players at
/// once, and their lines come in as A, B, A, B.
const RECENT_MAX: usize = 32;
/// Until the log exists (Minecraft never ran here), it's looked for this often.
const LOOK_FOR_LOG: Duration = Duration::from_secs(3);
/// How much of a log Klipp finds already there it reads for who's playing.
const HEAD: u64 = 1 << 20;

/// Minecraft's rules, from its section of the settings. Kills close together
/// are a multi-kill; lines are read a poll after they're written.
pub fn rules(base: &Rules, s: &settings::Minecraft) -> Rules {
    Rules {
        headshot: false,
        multi_kill_min: s.multi_kill_min,
        multi_kill_mode: MultiKillMode::Streak,
        any_kill: s.any_kill,
        knife_or_zeus_kill: false,
        death: s.death,
        mvp: false,
        victory: false,
        knock: false,
        first_blood: false,
        steal: false,
        objective: false,
        team_ace: false,
        goal: false,
        assist: false,
        save: false,
        epic_save: false,
        demolition: false,
        advancement: s.advancement,
        milestone: s.milestone,
        lateness: POLL.as_millis() as i64 * SECOND / 1000 + SECOND / 2,
        ..base.clone()
    }
}

/// The log the settings name, or the official launcher's.
pub fn log_path(s: &settings::Minecraft) -> Option<PathBuf> {
    if !s.log.trim().is_empty() {
        return Some(PathBuf::from(s.log.trim()));
    }
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join(".minecraft").join("logs").join("latest.log"))
}

pub struct MinecraftIntegration {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MinecraftIntegration {
    pub fn start(s: &settings::Minecraft, events: Sender<(String, GameEvent)>) -> Result<Option<Self>> {
        let Some(path) = log_path(s) else {
            info!("minecraft: no APPDATA folder; auto-clips for Minecraft are off");
            return Ok(None);
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new().name("minecraft".into()).spawn({
            let stop = stop.clone();
            let path = path.clone();
            move || watch(&stop, &path, &events)
        })?;
        info!("minecraft: following {}", path.display());
        Ok(Some(Self { stop, thread: Some(thread) }))
    }
}

impl Drop for MinecraftIntegration {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Where Klipp is in the log, and what it knows from it.
struct Follow {
    /// Bytes read so far (always the end of a whole line).
    offset: u64,
    /// The file's creation time: a new one means the game started again.
    created: Option<SystemTime>,
    player: Option<String>,
    /// Moments sent lately, to leave singleplayer's second copies out.
    recent: Recent,
    /// Said once per session that nobody is known to be playing.
    warned: bool,
}

impl Follow {
    /// The game started a new log: read it from its start. That happens
    /// when the game starts and at the first line after midnight while it
    /// keeps running. Only a start names the player again ("Setting
    /// user"), so the name is kept until a new one comes.
    fn new_log(&mut self, created: Option<SystemTime>) {
        *self = Follow { offset: 0, created, player: self.player.take(), recent: Recent::default(), warned: false };
    }
}

/// Messages sent within `SAME_MOMENT`, oldest first.
#[derive(Default)]
struct Recent(VecDeque<(String, i64)>);

impl Recent {
    /// True the first time `message` comes within `SAME_MOMENT`; remembers it.
    fn first(&mut self, message: &str, now: i64) -> bool {
        self.0.retain(|(_, t)| now - t < SAME_MOMENT);
        if self.0.iter().any(|(m, _)| m == message) {
            return false;
        }
        if self.0.len() >= RECENT_MAX {
            self.0.pop_front();
        }
        self.0.push_back((message.to_string(), now));
        true
    }
}

fn watch(stop: &AtomicBool, path: &Path, events: &Sender<(String, GameEvent)>) {
    let mut follow: Option<Follow> = None;
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        let mut wait = POLL;
        match fs::metadata(path) {
            Ok(meta) => {
                let created = meta.created().ok();
                let f = follow.get_or_insert_with(|| attach(path, meta.len(), created));
                // Windows can give a file made again under the same name its
                // old creation time, so a shorter file counts as new too.
                if f.created != created || meta.len() < f.offset {
                    info!("minecraft: a new game log started");
                    f.new_log(created);
                }
                if meta.len() > f.offset {
                    read_new(path, meta.len(), f, events);
                }
            }
            Err(_) => {
                follow = None;
                wait = LOOK_FOR_LOG;
            }
        }
        nap(stop, wait.saturating_sub(started.elapsed()));
    }
}

/// Sleeps in short steps, so quitting never waits long.
fn nap(stop: &AtomicBool, time: Duration) {
    let until = Instant::now() + time;
    while !stop.load(Ordering::Relaxed) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        thread::sleep(left.min(Duration::from_millis(250)));
    }
}

/// Starting with a log that's already there: who's playing, from what it
/// holds, and its end, so nothing that happened before Klipp is clipped.
fn attach(path: &Path, len: u64, created: Option<SystemTime>) -> Follow {
    // The game says it within its first lines; a long session's log can be big.
    let mut head = Vec::new();
    let _ = File::open(path).and_then(|f| f.take(HEAD).read_to_end(&mut head));
    let player = String::from_utf8_lossy(&head).lines().find_map(player_line);
    if let Some(p) = &player {
        info!("minecraft: {p} is playing");
    }
    Follow { offset: len, created, player, recent: Recent::default(), warned: false }
}

fn read_new(path: &Path, len: u64, f: &mut Follow, events: &Sender<(String, GameEvent)>) {
    let Ok(mut file) = File::open(path) else { return };
    if len - f.offset > MAX_READ {
        warning!("minecraft: {} MB were added to the log at once; skipping them", (len - f.offset) >> 20);
        f.offset = len;
        return;
    }
    let mut bytes = Vec::with_capacity((len - f.offset) as usize);
    if file.seek(SeekFrom::Start(f.offset)).is_err() || file.take(len - f.offset).read_to_end(&mut bytes).is_err() {
        return;
    }
    // A line still being written waits for the next look.
    let Some(end) = bytes.iter().rposition(|&b| b == b'\n') else { return };
    f.offset += end as u64 + 1;
    for line in String::from_utf8_lossy(&bytes[..end]).lines() {
        if let Some(p) = player_line(line) {
            info!("minecraft: {p} is playing");
            f.player = Some(p);
            continue;
        }
        let Some(message) = chat_message(line) else { continue };
        let Some(me) = f.player.as_deref() else {
            if !f.warned && is_moment(message) {
                f.warned = true;
                warning!("minecraft: chat came in but the log never said who is playing; restart Minecraft so Klipp sees it");
            }
            continue;
        };
        let Some((kind, detail)) = moment(message, me) else { continue };
        let now = clock::now();
        if !f.recent.first(message, now) {
            continue;
        }
        // The recorder logs every event it gets.
        let _ = events.send((GAME_NAME.to_string(), GameEvent::new(kind, now, detail)));
    }
}

// Reading the log. Plain functions, so they can be tried on sample lines.

/// "[12:00:00] [Render thread/INFO]: Setting user: Steve" → "Steve".
fn player_line(line: &str) -> Option<String> {
    let name = body(line)?.strip_prefix("Setting user: ")?.trim();
    is_name(name).then(|| name.to_string())
}

/// What a log line says, after its time and thread: "[21:14:03] [Render thread/INFO]: …".
/// Forge puts the logger after the level: "[Render thread/INFO] [net.minecraft…ChatComponent/]: …".
fn body(line: &str) -> Option<&str> {
    let line = line.trim_end();
    if !line.starts_with('[') {
        return None;
    }
    let (head, rest) = line.split_once("]: ")?;
    let info = head.ends_with("/INFO") || head.rsplit_once("] [").is_some_and(|(level, _logger)| level.ends_with("/INFO"));
    info.then_some(rest)
}

/// A line's chat message, without the tags newer versions put in front.
/// Players' own chat ("<Steve> …") is left out: typing a death message
/// mustn't make a clip.
fn chat_message(line: &str) -> Option<&str> {
    let mut m = body(line)?;
    // In any order and any number: "[System] [CHAT] ", "[CHAT] [Not Secure] ".
    while let Some(rest) = ["[System] ", "[CHAT] ", "[Not Secure] "].iter().find_map(|tag| m.strip_prefix(tag)) {
        m = rest;
    }
    (!m.starts_with('<') && !m.is_empty()).then_some(m)
}

/// A player name as Minecraft allows them: 3-16 letters, digits or underscores.
fn is_name(s: &str) -> bool {
    (3..=16).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// How vanilla death messages go on after the victim's name. Messages that
/// start with a name and aren't one of these ("joined the game") are left out.
const DEATHS: &[&str] = &[
    "was ",
    "drowned",
    "died",
    "blew up",
    "burned to death",
    "hit the ground too hard",
    "fell ",
    "went up in flames",
    "went off with a bang",
    "walked into ",
    "tried to swim in lava",
    "discovered the floor was lava",
    "suffocated in a wall",
    "starved to death",
    "experienced kinetic energy",
    "withered away",
    "froze to death",
    "didn't want to live in the same world as",
    "left the confines of this world",
];

/// Server messages that start with "was " but aren't deaths.
const NOT_DEATHS: &[&str] = &["was kicked", "was banned"];

fn is_death(rest: &str) -> bool {
    DEATHS.iter().any(|d| rest.starts_with(d)) && !NOT_DEATHS.iter().any(|d| rest.starts_with(d))
}

/// A death or an advancement of anyone's: chat Klipp would clip if it knew who's playing.
fn is_moment(message: &str) -> bool {
    message.split_once(' ').is_some_and(|(who, rest)| is_name(who) && (is_death(rest) || rest.starts_with("has ")))
}

/// The moment a chat message is for `me`, and its detail: an advancement's
/// name, the weapon of a kill, how you died.
fn moment(message: &str, me: &str) -> Option<(EventKind, String)> {
    let (who, rest) = message.split_once(' ')?;
    if !is_name(who) || !is_name(me) {
        return None;
    }
    if let Some(name) = bracketed(rest, "has made the advancement ") {
        return (who == me).then(|| (EventKind::Advancement, name));
    }
    if let Some(name) = bracketed(rest, "has completed the challenge ").or_else(|| bracketed(rest, "has reached the goal ")) {
        return (who == me).then(|| (EventKind::Milestone, name));
    }
    if !is_death(rest) {
        return None;
    }
    // The weapon's name is the player's to choose, so names are looked for before it.
    let (how, weapon) = match rest.split_once(" using ") {
        Some((how, weapon)) => (how, weapon.trim().trim_start_matches('[').trim_end_matches(']')),
        None => (rest, ""),
    };
    if who == me {
        return Some((EventKind::Death, how.trim_end_matches('.').to_string()));
    }
    let named = how.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|word| word == me);
    named.then(|| (EventKind::Kill, weapon.to_string()))
}

/// "has made the advancement [Diamonds!]" with that prefix → "Diamonds!".
fn bracketed(text: &str, prefix: &str) -> Option<String> {
    let inner = text.strip_prefix(prefix)?.trim().strip_prefix('[')?.strip_suffix(']')?;
    (!inner.is_empty()).then(|| inner.to_string())
}
