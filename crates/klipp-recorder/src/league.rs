//! League of Legends through Riot's Live Client Data API.
//!
//! While a game runs, the League game client answers on
//! https://127.0.0.1:2999 with what has happened so far: every event, timed
//! on the game clock. Klipp asks about once a second, and only while
//! `League of Legends.exe` runs. It's Riot's official local API; Klipp reads
//! nothing from the game itself, which is what Vanguard allows.
//!
//! Events name players by Riot ID ("Name#TAG"), by the name alone, or in
//! streamer mode by champion, so "you" is every name you go by in the game.

use std::collections::{BTreeSet, VecDeque};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde_json::Value;
use windows::Win32::Networking::WinHttp::*;
use windows::core::{HSTRING, PCWSTR, w};

use crate::autoclip::Rules;
use crate::clock::{self, SECOND};
use crate::events::{EventKind, GameEvent, MatchContext};
use crate::log::{info, warning};
use crate::settings::{self, MultiKillMode};

pub const GAME_NAME: &str = "League of Legends";
/// The game itself (the client you queue in is LeagueClient.exe): it runs from
/// the loading screen until the game closes, the same as the API.
const EXE: &str = "league of legends.exe";
const PORT: u16 = 2999;
/// How often Klipp asks for new events during a game.
const POLL: Duration = Duration::from_secs(1);
/// How often it looks for the game.
const LOOK: Duration = Duration::from_secs(3);
/// Who's who is asked again this often, and more often while it's unknown.
const IDENTITY_EVERY: i64 = 30 * SECOND;
const IDENTITY_RETRY: i64 = 5 * SECOND;
/// League counts kills this close together as a multi-kill, and gives a
/// quadra this long to become a penta.
const MULTI_KILL_WINDOW: i64 = 10 * SECOND;
const PENTA_WINDOW: i64 = 30 * SECOND;
/// A team ace is yours too if you killed or assisted this soon before it (game seconds).
const ACE_PART_SECS: f64 = 30.0;
/// Clock samples kept; the earliest is the truest (delays only make them later).
const CLOCK_SAMPLES: usize = 10;
/// A game that hasn't answered this long after it started gets one warning.
const SILENT_WARNING: Duration = Duration::from_secs(180);
/// Answers bigger than this aren't League's.
const MAX_BODY: usize = 4 << 20;
/// Not games of League: Teamfight Tactics and Swarm (PvE) run in the same client.
const SKIPPED_MODES: &[&str] = &["TFT", "STRAWBERRY"];
/// Events with nothing in them for Klipp.
const IGNORED_EVENTS: &[&str] = &["GameStart", "MinionsSpawning", "FirstBrick", "InhibRespawningSoon", "InhibRespawned"];

/// League's rules, from its section of the settings: multi-kills counted the
/// game's way (it sends the count with each one). Events are seen up to a
/// poll after they happened, so chains wait that much longer.
pub fn rules(base: &Rules, s: &settings::League) -> Rules {
    Rules {
        headshot: false,
        multi_kill_min: s.multi_kill_min,
        multi_kill_mode: MultiKillMode::Round,
        any_kill: s.any_kill,
        knife_or_zeus_kill: false,
        death: s.death,
        mvp: false,
        victory: false,
        first_blood: s.first_blood,
        steal: s.steal,
        objective: s.objective,
        team_ace: s.ace,
        goal: false,
        assist: false,
        save: false,
        epic_save: false,
        demolition: false,
        lateness: POLL.as_millis() as i64 * SECOND / 1000 + SECOND / 2,
        ..base.clone()
    }
}

/// League is installed (for setup only: the integration doesn't need to
/// know). Riot's launcher keeps this file for it; it isn't documented.
pub fn installed() -> bool {
    std::env::var_os("ProgramData").is_some_and(|d| {
        std::path::Path::new(&d)
            .join(r"Riot Games\Metadata\league_of_legends.live\league_of_legends.live.product_settings.yaml")
            .exists()
    })
}

pub struct LeagueIntegration {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub context: Arc<Mutex<MatchContext>>,
}

impl LeagueIntegration {
    /// Starts looking for League games. There's nothing to install: the game
    /// serves the API by itself.
    pub fn start(events: Sender<(String, GameEvent)>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let context = Arc::new(Mutex::new(MatchContext::default()));
        let thread = thread::Builder::new().name("league".into()).spawn({
            let (stop, context) = (stop.clone(), context.clone());
            move || watch(&stop, &events, &context)
        })?;
        info!("league: watching for League of Legends games");
        Ok(Self { stop, thread: Some(thread), context })
    }
}

impl Drop for LeagueIntegration {
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

fn watch(stop: &AtomicBool, events: &Sender<(String, GameEvent)>, context: &Mutex<MatchContext>) {
    let mut session: Option<Session> = None;
    let mut looked: Option<(Instant, bool)> = None;
    loop {
        // A look at the process list every few seconds, even mid-game.
        let running = match looked {
            Some((at, running)) if at.elapsed() < LOOK => running,
            _ => {
                let running = crate::games::running(EXE);
                looked = Some((Instant::now(), running));
                running
            }
        };
        if !running {
            if let Some(s) = session.take() {
                s.finish();
            }
            if !nap(stop, LOOK) {
                return;
            }
            continue;
        }
        let started = Instant::now();
        if session.is_none() {
            match Session::new() {
                Ok(s) => session = Some(s),
                Err(e) => {
                    warning!("league: {e:#}; trying again in a minute");
                    if !nap(stop, Duration::from_secs(60)) {
                        return;
                    }
                    continue;
                }
            }
        }
        if let Some(s) = &mut session {
            s.poll(events, context);
        }
        if !nap(stop, POLL.saturating_sub(started.elapsed())) {
            return;
        }
    }
}

/// One run of the game: its connection and what's been seen.
struct Session {
    http: LocalHttps,
    tracker: Tracker,
    started: Instant,
    answered: bool,
    warned: bool,
    last_error: String,
}

impl Session {
    fn new() -> Result<Self> {
        let http = LocalHttps::new()?;
        info!("league: the game is running");
        Ok(Self { http, tracker: Tracker::default(), started: Instant::now(), answered: false, warned: false, last_error: String::new() })
    }

    /// Asks the game what happened since the last time.
    fn poll(&mut self, events: &Sender<(String, GameEvent)>, context: &Mutex<MatchContext>) {
        let before = clock::now();
        // Until the game has loaded, connections are refused or answered with 404s.
        let stats = match self.http.json("/liveclientdata/gamestats") {
            Ok(v) if v["gameTime"].is_number() => v,
            Ok(_) => return,
            Err(e) => {
                self.last_error = format!("{e:#}");
                if !self.answered && !self.warned && self.started.elapsed() > SILENT_WARNING {
                    self.warned = true;
                    warning!("league: the game's API hasn't answered in {} s ({}); no auto-clips from this game", SILENT_WARNING.as_secs(), self.last_error);
                }
                return;
            }
        };
        let after = clock::now();
        if !self.answered {
            self.answered = true;
            info!("league: the game's API answers");
        }
        if self.tracker.wants_identity(after) {
            let active = self.http.json("/liveclientdata/activeplayer").ok();
            let players = self.http.json("/liveclientdata/playerlist").ok();
            let before = self.tracker.identity.as_ref().map(Identity::describe);
            self.tracker.set_identity(active.as_ref(), players.as_ref(), after);
            let now = self.tracker.identity.as_ref().map(Identity::describe);
            if now != before {
                match now {
                    Some(me) => info!("league: you're {me}"),
                    None => info!("league: none of this game's players is you (spectating, or a replay); no auto-clips from it"),
                }
            }
        }
        let Ok(list) = self.http.json("/liveclientdata/eventdata") else { return };
        let found = self.tracker.on_poll(&stats, (before, after), &list, clock::now());
        for name in std::mem::take(&mut self.tracker.new_unknown) {
            info!("league: an event Klipp doesn't know yet, {name}; ignored");
        }
        if self.tracker.match_started {
            info!("league: a game started ({} on {})", self.tracker.context.mode, self.tracker.context.map);
            crate::overlay::post(crate::overlay::Indicator::MatchStarted);
        }
        *context.lock().unwrap() = self.tracker.context.clone();
        for e in found {
            let _ = events.send((GAME_NAME.to_string(), e));
        }
    }

    /// The game closed: one line on how it went, to check names were recognized.
    fn finish(self) {
        let t = &self.tracker;
        if t.kills_seen == 0 {
            info!("league: the game closed");
        } else if t.kills_mine == 0 && t.identity.is_some() {
            let form = if t.tagged > 0 { "with a #tag" } else { "without a #tag" };
            warning!("league: the game closed; none of its {} champion kills named you (killer names came {form})", t.kills_seen);
        } else {
            info!("league: the game closed ({} champion kills, {} with you in them)", t.kills_seen, t.kills_mine);
        }
    }
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void, what: &str) -> Result<Self> {
        if raw.is_null() {
            bail!("{what}: {}", windows::core::Error::from_thread());
        }
        Ok(Self(raw))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

/// HTTPS to the game on this PC. Its certificate is self-signed (and expired
/// once, in January 2026), so certificate errors are ignored, here only: the
/// address is fixed to 127.0.0.1, nothing leaves the PC, and a program posing
/// as the game could only cause false clips. WinHTTP is part of Windows.
struct LocalHttps {
    _session: Handle,
    connect: Handle,
}

impl LocalHttps {
    fn new() -> Result<Self> {
        unsafe {
            let agent = HSTRING::from(format!("Klipp/{}", env!("CARGO_PKG_VERSION")));
            // No proxy: loopback traffic must never go through one.
            let session = Handle::new(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "opening WinHTTP")?;
            WinHttpSetTimeouts(session.0, 500, 500, 2_000, 2_000)?;
            let connect = Handle::new(WinHttpConnect(session.0, w!("127.0.0.1"), PORT, 0), "connecting")?;
            Ok(Self { _session: session, connect })
        }
    }

    /// GET `path`: the status and the answer.
    fn get(&self, path: &str) -> Result<(u32, Vec<u8>)> {
        unsafe {
            let request = Handle::new(
                WinHttpOpenRequest(self.connect.0, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
                "opening the request",
            )?;
            let ignore = SECURITY_FLAG_IGNORE_UNKNOWN_CA
                | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
                | SECURITY_FLAG_IGNORE_CERT_CN_INVALID
                | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
            WinHttpSetOption(Some(request.0 as *const c_void), WINHTTP_OPTION_SECURITY_FLAGS, Some(&ignore.to_ne_bytes()))?;
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
            let mut body = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            loop {
                let mut read = 0u32;
                WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)?;
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read as usize]);
                if body.len() > MAX_BODY {
                    bail!("the answer is too large");
                }
            }
            Ok((status, body))
        }
    }

    /// The JSON at `path`. Anything but a 200 with JSON is an error: while
    /// the game loads, it answers 404 or not at all.
    fn json(&self, path: &str) -> Result<Value> {
        let (status, body) = self.get(path)?;
        if status != 200 {
            bail!("HTTP {status}");
        }
        Ok(serde_json::from_slice(&body)?)
    }
}

/// Maps League's game clock onto the shared clock. Each poll gives a sample:
/// the middle of the request, minus the game time it answered. Delays only
/// make a sample later, so the earliest recent one is the truest; one that's
/// over a second off (a pause, a replay seek) starts over.
#[derive(Default)]
struct ClockSync {
    samples: VecDeque<i64>,
}

impl ClockSync {
    fn add(&mut self, (before, after): (i64, i64), game_secs: f64) {
        let sample = before + (after - before) / 2 - secs(game_secs);
        if self.offset().is_some_and(|o| (sample - o).abs() > SECOND) {
            self.samples.clear();
        }
        self.samples.push_back(sample);
        if self.samples.len() > CLOCK_SAMPLES {
            self.samples.pop_front();
        }
    }

    fn offset(&self) -> Option<i64> {
        self.samples.iter().min().copied()
    }

    /// When an event at `game_secs` happened; never later than `now`, which
    /// is also the answer before the first sample.
    fn time(&self, game_secs: f64, now: i64) -> i64 {
        self.offset().map_or(now, |o| (o + secs(game_secs)).min(now))
    }
}

fn secs(s: f64) -> i64 {
    (s * SECOND as f64) as i64
}

/// One player of the game, and the names events may use for them.
#[derive(Clone, Debug, PartialEq)]
struct Player {
    /// "Name#TAG"; empty if the game didn't say.
    riot_id: String,
    /// The name without the tag, the summoner name, and the champion when
    /// nobody else plays it (streamer mode names players by champion).
    names: Vec<String>,
    champion: String,
    /// "ORDER" or "CHAOS".
    team: String,
}

impl Player {
    fn from(p: &Value) -> Self {
        let text = |k: &str| p[k].as_str().unwrap_or_default().trim().to_string();
        let (game_name, tag) = (text("riotIdGameName"), text("riotIdTagLine"));
        let summoner = text("summonerName");
        let mut riot_id = text("riotId");
        if riot_id.is_empty() && !game_name.is_empty() && !tag.is_empty() {
            riot_id = format!("{game_name}#{tag}");
        }
        if riot_id.is_empty() && summoner.contains('#') {
            riot_id = summoner.clone();
        }
        let names = [game_name, summoner].into_iter().filter(|n| !n.is_empty() && !n.contains('#')).collect();
        Self { riot_id, names, champion: text("championName"), team: text("team") }
    }

    /// Whether an event's name is this player: a name with a tag must be the
    /// whole Riot ID (the same name with another tag is someone else).
    fn is(&self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        if name.contains('#') {
            return self.riot_id.eq_ignore_ascii_case(name);
        }
        self.names.iter().any(|n| n.eq_ignore_ascii_case(name))
    }
}

/// Everyone in the game, and which of them is you.
#[derive(Clone, Debug, PartialEq)]
struct Identity {
    players: Vec<Player>,
    me: usize,
}

impl Identity {
    /// From `activeplayer` and `playerlist`. `None` when no player is you:
    /// spectating and replays have no player of their own.
    fn new(active: &Value, list: &Value) -> Option<Self> {
        let mut players: Vec<Player> = list.as_array()?.iter().map(Player::from).collect();
        // Champions only identify players who are alone on them (not in One for All).
        let raw: Vec<String> = list.as_array()?.iter().map(raw_champion).collect();
        for i in 0..players.len() {
            let champion = players[i].champion.clone();
            if !champion.is_empty() && players.iter().filter(|p| p.champion.eq_ignore_ascii_case(&champion)).count() == 1 {
                players[i].names.push(champion);
                if !raw[i].is_empty() && !raw[i].eq_ignore_ascii_case(&players[i].champion) {
                    players[i].names.push(raw[i].clone());
                }
            }
        }
        let you = Player::from(active);
        let me = if you.riot_id.is_empty() {
            you.names.iter().find_map(|n| players.iter().position(|p| p.is(n)))
        } else {
            // A Riot ID must be in the list; the list's entries may lack the tag.
            players
                .iter()
                .position(|p| p.riot_id.eq_ignore_ascii_case(&you.riot_id))
                .or_else(|| players.iter().position(|p| p.riot_id.is_empty() && you.names.iter().any(|n| p.is(n))))
        }?;
        Some(Self { players, me })
    }

    fn me(&self) -> &Player {
        &self.players[self.me]
    }

    fn is_me(&self, name: &str) -> bool {
        self.me().is(name)
    }

    fn player(&self, name: &str) -> Option<&Player> {
        self.players.iter().find(|p| p.is(name))
    }

    /// For the log: your champion and side, never your name.
    fn describe(&self) -> String {
        let me = self.me();
        let champion = if me.champion.is_empty() { "playing" } else { me.champion.as_str() };
        format!("{champion} on the {} side", if me.team == "CHAOS" { "red" } else { "blue" })
    }
}

/// "MissFortune" from `rawChampionName` ("game_character_displayname_MissFortune"):
/// events may use this form of the champion's name.
fn raw_champion(p: &Value) -> String {
    p["rawChampionName"].as_str().unwrap_or_default().rsplit('_').next().unwrap_or_default().to_string()
}

/// Turns what the game reports into events.
#[derive(Default)]
struct Tracker {
    identity: Option<Identity>,
    /// When who's who was last asked (shared clock).
    identity_asked: Option<i64>,
    clock: ClockSync,
    /// The newest EventID handled; `None` until the first answer of a game,
    /// which is only a baseline (Klipp may have started mid-game).
    last_id: Option<i64>,
    /// Game time of your recent kills and assists, for whether an ace was yours.
    involved: VecDeque<f64>,
    context: MatchContext,
    /// The last poll found a new game (the "Auto-clips ON" indicator's cue).
    match_started: bool,
    announced: bool,
    /// Event names not known yet: each is logged once.
    unknown: BTreeSet<String>,
    new_unknown: Vec<String>,
    /// For the line when the game closes: champion kills, those with you in
    /// them, and those whose killer had a #tag.
    kills_seen: u32,
    kills_mine: u32,
    tagged: u32,
}

impl Tracker {
    fn wants_identity(&self, now: i64) -> bool {
        let every = if self.identity.is_some() { IDENTITY_EVERY } else { IDENTITY_RETRY };
        self.identity_asked.is_none_or(|t| now - t >= every)
    }

    /// Who's who, from the game's answers; a request that failed keeps what was known.
    fn set_identity(&mut self, active: Option<&Value>, players: Option<&Value>, now: i64) {
        self.identity_asked = Some(now);
        if let (Some(active), Some(players)) = (active, players) {
            self.identity = Identity::new(active, players);
        }
    }

    /// A poll's answers: `gamestats` (asked between `sample`'s two times) and `eventdata`.
    fn on_poll(&mut self, stats: &Value, sample: (i64, i64), data: &Value, now: i64) -> Vec<GameEvent> {
        self.match_started = false;
        let Some(all) = data["Events"].as_array() else { return Vec::new() };
        let mut list: Vec<(i64, &Value)> = all.iter().filter_map(|e| Some((e["EventID"].as_i64()?, e))).collect();
        list.sort_by_key(|(id, _)| *id);
        let newest = list.last().map_or(-1, |(id, _)| *id);
        // IDs going backwards: a new game in the same client.
        if self.last_id.is_some_and(|last| newest < last) {
            *self = Tracker { unknown: std::mem::take(&mut self.unknown), ..Tracker::default() };
        }
        if let Some(t) = stats["gameTime"].as_f64() {
            self.clock.add(sample, t);
        }
        let mode = stats["gameMode"].as_str().unwrap_or_default().to_string();
        self.context = MatchContext { map: stats["mapName"].as_str().unwrap_or_default().to_string(), mode, seen: now };
        let skipped = SKIPPED_MODES.contains(&self.context.mode.as_str());
        if self.identity.is_some() && !skipped && !self.announced {
            self.announced = true;
            self.match_started = true;
        }
        let previous = self.last_id;
        self.last_id = Some(previous.map_or(newest, |last| last.max(newest)));
        let Some(last) = previous else { return Vec::new() };
        let Some(me) = self.identity.clone().filter(|_| !skipped) else { return Vec::new() };
        let mut out = Vec::new();
        for (_, e) in list.into_iter().filter(|(id, _)| *id > last) {
            self.event(e, &me, now, &mut out);
        }
        out
    }

    fn event(&mut self, e: &Value, me: &Identity, now: i64, out: &mut Vec<GameEvent>) {
        let name = e["EventName"].as_str().unwrap_or_default();
        let game_secs = e["EventTime"].as_f64().unwrap_or(0.0);
        let t = self.clock.time(game_secs, now);
        let mine = |v: &Value| v.as_str().is_some_and(|n| me.is_me(n));
        let assisted = e["Assisters"].as_array().is_some_and(|a| a.iter().any(mine));
        let champion = |v: &Value| v.as_str().and_then(|n| me.player(n)).map_or(String::new(), |p| p.champion.clone());
        match name {
            "ChampionKill" => {
                let (killed, died) = (mine(&e["KillerName"]), mine(&e["VictimName"]));
                self.kills_seen += 1;
                self.tagged += u32::from(e["KillerName"].as_str().is_some_and(|n| n.contains('#')));
                self.kills_mine += u32::from(killed || died || assisted);
                if killed {
                    out.push(GameEvent::new(EventKind::Kill, t, champion(&e["VictimName"])).holding(t + MULTI_KILL_WINDOW));
                }
                if assisted {
                    out.push(GameEvent::new(EventKind::Assist, t, champion(&e["VictimName"])));
                }
                // Executions too: a turret or minion's kill is still your death.
                if died {
                    out.push(GameEvent::new(EventKind::Death, t, champion(&e["KillerName"])));
                }
                if killed || assisted {
                    self.involved.push_back(game_secs);
                    while self.involved.front().is_some_and(|&s| game_secs - s > ACE_PART_SECS) {
                        self.involved.pop_front();
                    }
                }
            }
            "Multikill" if mine(&e["KillerName"]) => {
                if let Some(n) = e["KillStreak"].as_u64().filter(|n| (2..=5).contains(n)) {
                    let hold = match n {
                        4 => t + PENTA_WINDOW,
                        5 => 0,
                        _ => t + MULTI_KILL_WINDOW,
                    };
                    out.push(GameEvent::new(EventKind::MultiKill(n as u8), t, "").holding(hold));
                }
            }
            "FirstBlood" if mine(&e["Recipient"]) => out.push(GameEvent::new(EventKind::FirstBlood, t, "")),
            "Ace" => {
                let ours = e["AcingTeam"].as_str() == Some(me.me().team.as_str());
                let took_part = self.involved.iter().any(|&s| s <= game_secs && game_secs - s <= ACE_PART_SECS);
                if mine(&e["Acer"]) || (ours && took_part) {
                    out.push(GameEvent::new(EventKind::TeamAce, t, ""));
                }
            }
            "DragonKill" => {
                let kind = e["DragonType"].as_str().unwrap_or_default().to_lowercase();
                let detail = if kind.is_empty() { "dragon".to_string() } else { format!("dragon_{kind}") };
                self.objective(e, mine(&e["KillerName"]), assisted, t, detail, out);
            }
            "BaronKill" => self.objective(e, mine(&e["KillerName"]), assisted, t, "baron".into(), out),
            "HeraldKill" => self.objective(e, mine(&e["KillerName"]), assisted, t, "herald".into(), out),
            "TurretKilled" => self.objective(e, mine(&e["KillerName"]), assisted, t, "turret".into(), out),
            "InhibKilled" => self.objective(e, mine(&e["KillerName"]), assisted, t, "inhibitor".into(), out),
            "GameEnd" => match e["Result"].as_str() {
                Some(r) if r.eq_ignore_ascii_case("win") => out.push(GameEvent::new(EventKind::MatchWin, t, "")),
                Some(r) if !r.is_empty() => out.push(GameEvent::new(EventKind::MatchLoss, t, "")),
                _ => {}
            },
            "Multikill" | "FirstBlood" => {}
            n if IGNORED_EVENTS.contains(&n) => {}
            // Newer monsters (Voidgrubs, Atakhan) aren't in Riot's docs; ones
            // shaped like the others ("…Kill" with a killer) count the same.
            n if n.ends_with("Kill") && n.len() > 4 && e["KillerName"].is_string() => {
                let detail = n.trim_end_matches("Kill").to_lowercase();
                self.objective(e, mine(&e["KillerName"]), assisted, t, detail, out);
                self.note_unknown(n);
            }
            n => self.note_unknown(n),
        }
    }

    /// A monster or building taken with you in it: a steal if you took a
    /// Baron, Dragon or Herald from the other team (`Stolen` is the string
    /// "True" in Riot's sample, so a bool or a string), else an objective.
    fn objective(&self, e: &Value, killed: bool, assisted: bool, t: i64, detail: String, out: &mut Vec<GameEvent>) {
        let stolen = match &e["Stolen"] {
            Value::Bool(b) => *b,
            Value::String(s) => s.eq_ignore_ascii_case("true"),
            _ => false,
        };
        if stolen && killed {
            out.push(GameEvent::new(EventKind::Steal, t, detail));
        } else if killed || assisted {
            out.push(GameEvent::new(EventKind::Objective, t, detail));
        }
    }

    fn note_unknown(&mut self, name: &str) {
        if self.unknown.insert(name.to_string()) {
            self.new_unknown.push(name.to_string());
        }
    }
}
