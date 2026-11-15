//! Auto-backup (Plus and Pro): clips go to the account by themselves, as
//! private clips. Which ones is chosen on klipp.dev (Settings → Account):
//! favorites, every auto-clip, or every clip. Only clips saved after it was
//! turned on go up; what was already in the folder stays on this PC.
//!
//! The recorder runs a pass between games ([`run`]). A pass asks the site who
//! is signed in, what they chose and since when, looks through the clips
//! folder, and uploads what isn't there yet, newest first, a few clips at a
//! time. What went up is written to `.klipp\backup.json` in the clips folder
//! ([`Ledger`]), so a clip goes up once: also after Klipp is installed again,
//! and after the clips folder moves.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::api::{Account, ApiError, Client, NewUpload, Plan};
use crate::http::Cancelled;
use crate::{token, upload};

/// The hidden folder inside the clips folder where Klipp keeps what it knows about clips.
const META_DIR: &str = ".klipp";
/// The window's exports: not clips.
const EXPORTS_DIR: &str = "Exports";
/// Clips that changed this recently wait for a later pass: they may still be
/// written, trimmed or squeezed, and it leaves a moment to delete one that
/// shouldn't go anywhere.
const SETTLE: Duration = Duration::from_secs(600);
/// Uploads in one pass. With a pass every ten minutes that is half of what
/// the site lets one account upload in an hour, so sharing a clip by hand
/// always has room.
pub const PASS_LIMIT: u32 = 5;
/// A thumbnail the site takes.
const THUMB_MAX: u64 = 1024 * 1024;

/// Which clips the account wants backed up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Favorites,
    /// Every auto-clip.
    Auto,
    All,
}

impl Mode {
    /// The account's choice as the site sends it; anything unknown is off.
    pub fn parse(choice: &str) -> Self {
        match choice {
            "favorites" => Self::Favorites,
            "auto" => Self::Auto,
            "all" => Self::All,
            _ => Self::Off,
        }
    }

    fn wants(self, clip: &Clip) -> bool {
        match self {
            Self::Off => false,
            Self::Favorites => clip.favorite,
            Self::Auto => clip.auto,
            Self::All => true,
        }
    }
}

/// A clip in the clips folder, as a pass sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// Its place in the clips folder, `Counter-Strike 2/2026-10-13_21-29-01_2k`: also its name in the ledger.
    pub key: String,
    pub path: PathBuf,
    pub bytes: u64,
    /// When the file last changed, in seconds since 1970.
    pub modified: u64,
    /// When it was saved, in seconds since 1970: the date in its name
    /// (trimming or squeezing a clip later doesn't make it new), or the
    /// file's own date when its name has none.
    pub recorded: u64,
    /// It hasn't changed for a while (see `SETTLE`).
    pub settled: bool,
    pub favorite: bool,
    /// Saved by auto-clips, not by the save key.
    pub auto: bool,
    pub seconds: Option<f64>,
    /// What the site is told about it.
    pub title: String,
    pub game: Option<String>,
    pub tags: Vec<String>,
    pub map: Option<String>,
}

/// A clip that is on the account.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Done {
    /// Its id on klipp.dev.
    pub id: String,
    /// Seconds since 1970.
    pub at: u64,
    pub bytes: u64,
}

/// A clip the plan doesn't take, and for which plan and file that was found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skipped {
    pub why: String,
    pub plan: String,
    pub bytes: u64,
}

/// What auto-backup has done with this clips folder: `.klipp\backup.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    version: u32,
    /// The account the clips went to. Signing in to another one starts over.
    #[serde(default)]
    account: String,
    /// When a pass first found auto-backup on, in seconds since 1970. Only
    /// used with a copy of the site that doesn't say when it was turned on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    since: Option<u64>,
    #[serde(default)]
    done: BTreeMap<String, Done>,
    #[serde(default)]
    skipped: BTreeMap<String, Skipped>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self { version: 1, account: String::new(), since: None, done: BTreeMap::new(), skipped: BTreeMap::new() }
    }
}

impl Ledger {
    pub fn path(clips_dir: &Path) -> PathBuf {
        clips_dir.join(META_DIR).join("backup.json")
    }

    /// The ledger of this clips folder; an empty one when nothing was backed
    /// up yet. A ledger that is there but can't be read is an error: carrying
    /// on without it would upload everything a second time.
    pub fn load(clips_dir: &Path) -> Result<Self> {
        let path = Self::path(clips_dir);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        serde_json::from_str(text.trim_start_matches('\u{feff}')).with_context(|| format!("{} isn't what Klipp wrote", path.display()))
    }

    pub fn save(&self, clips_dir: &Path) -> Result<()> {
        let path = Self::path(clips_dir);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let part = path.with_extension("json.part");
        fs::write(&part, serde_json::to_string_pretty(self)?).with_context(|| format!("writing {}", part.display()))?;
        fs::rename(&part, &path).with_context(|| format!("writing {}", path.display()))
    }

    /// The clip with this key (see [`key_of`]) is on the account.
    pub fn backed_up(&self, key: &str) -> bool {
        self.done.contains_key(key)
    }

    /// How many clips from this folder are on the account.
    pub fn count(&self) -> usize {
        self.done.len()
    }

    /// The ledger to go on with for this account: this one, or a fresh one when it belongs to another account.
    fn for_account(self, id: &str) -> Self {
        if id.is_empty() || self.account == id {
            return self;
        }
        if self.account.is_empty() && self.since.is_none() && self.done.is_empty() && self.skipped.is_empty() {
            return Self { account: id.to_string(), ..self };
        }
        Self { account: id.to_string(), ..Self::default() }
    }

    /// The plan didn't take this file before, and neither changed since.
    fn skips(&self, clip: &Clip, plan: &Plan) -> bool {
        self.skipped.get(&clip.key).is_some_and(|s| s.plan == plan.id && s.bytes == clip.bytes)
    }
}

/// A clip's name in the ledger: its game folder and file name, without the extension.
pub fn key_of(clip: &Path) -> Option<String> {
    let folder = clip.parent()?.file_name()?.to_string_lossy();
    let stem = clip.file_stem()?.to_string_lossy();
    Some(format!("{folder}/{stem}"))
}

/// `<clips>\.klipp\<kind>\<game folder>\<stem>.<ext>`, where the recorder writes a clip's info and thumbnail.
fn meta_file(clips_dir: &Path, folder: &str, stem: &str, kind: &str, ext: &str) -> PathBuf {
    clips_dir.join(META_DIR).join(kind).join(folder).join(format!("{stem}.{ext}"))
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

const DAY: i64 = 86_400;

/// Days since 1970-01-01 of a calendar date (Howard Hinnant's days_from_civil).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A local date and time counted in seconds, as if it were UTC.
fn local_secs(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    days_from_civil(y, mo, d) * DAY + h * 3600 + mi * 60 + s
}

/// "2026-10-13_21-29-01_2k" → when the recorder saved it, in local seconds.
/// Its names are a 19-character date and time, then optionally "_<label>".
fn name_time(stem: &str) -> Option<i64> {
    let b = stem.as_bytes();
    if b.len() < 19 || !matches!(b.get(19), None | Some(b'_')) {
        return None;
    }
    let seps = [(4, b'-'), (7, b'-'), (10, b'_'), (13, b'-'), (16, b'-')];
    if seps.iter().any(|&(i, c)| b[i] != c) {
        return None;
    }
    let num = |from: usize, len: usize| -> Option<i64> {
        let s = stem.get(from..from + len)?;
        s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten()
    };
    let (y, mo, d, h, mi, s) = (num(0, 4)?, num(5, 2)?, num(8, 2)?, num(11, 2)?, num(14, 2)?, num(17, 2)?);
    let valid = (1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && s < 61;
    valid.then(|| local_secs(y, mo, d, h, mi, s))
}

/// This PC's local time minus UTC right now, in seconds.
fn local_offset() -> i64 {
    let t = unsafe { GetLocalTime() };
    let local = local_secs(t.wYear as i64, t.wMonth as i64, t.wDay as i64, t.wHour as i64, t.wMinute as i64, t.wSecond as i64);
    let utc = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    // The two clocks are read a moment apart; time zones differ by quarter hours.
    ((local - utc) as f64 / 900.0).round() as i64 * 900
}

/// When a clip was saved, in seconds since 1970: the date in its name, read
/// as this PC's local time. A name without a date, or with one from the
/// future, falls back to when the file last changed.
fn recorded_at(stem: &str, modified: u64, now: u64, offset: i64) -> u64 {
    name_time(stem).map(|local| local - offset).filter(|&at| at > 0 && at as u64 <= now + DAY as u64).map_or(modified, |at| at as u64)
}

/// Every clip Klipp saved in the game folders: not exports, not `.klipp` or
/// `.trash`, and not another app's videos in a folder that is shared (those
/// have no info file).
pub fn scan(clips_dir: &Path, now: SystemTime) -> Vec<Clip> {
    let mut clips = Vec::new();
    let offset = local_offset();
    let now_secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    for game in fs::read_dir(clips_dir).into_iter().flatten().flatten() {
        let folder = game.file_name().to_string_lossy().into_owned();
        if folder.starts_with('.') || folder.eq_ignore_ascii_case(EXPORTS_DIR) || !game.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for file in fs::read_dir(game.path()).into_iter().flatten().flatten() {
            let path = file.path();
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4")) {
                continue;
            }
            let Ok(meta) = file.metadata() else { continue };
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
            if !meta.is_file() {
                continue;
            }
            let Some(info) = read_json(&meta_file(clips_dir, &folder, &stem, "info", "json")).filter(Value::is_object) else { continue };
            let modified = meta.modified().ok();
            // A time in the future counts as just changed.
            let settled = modified.and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age >= SETTLE);
            let (title, game, tags, map) = describe(&info, &folder);
            let modified = modified.and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            clips.push(Clip {
                key: format!("{folder}/{stem}"),
                path,
                bytes: meta.len(),
                modified,
                recorded: recorded_at(&stem, modified, now_secs, offset),
                settled,
                favorite: info["favorite"] == true,
                auto: info["kind"] == "auto",
                seconds: info["duration_secs"].as_f64(),
                title,
                game,
                tags,
                map,
            });
        }
    }
    clips
}

/// The moments Klipp names, as the window shows them.
const MOMENTS: &[(&str, &str)] = &[
    ("kill", "Kill"),
    ("headshot", "Headshot"),
    ("ace", "ACE"),
    ("knife", "Knife kill"),
    ("zeus", "Zeus kill"),
    ("mvp", "MVP"),
    ("victory", "Round won"),
    ("defeat", "Round lost"),
    ("defuse", "Bomb defused"),
    ("penta", "Pentakill"),
    ("teamace", "Team ace"),
    ("firstblood", "First blood"),
    ("steal", "Steal"),
    ("objective", "Objective"),
    ("win", "Victory"),
    ("loss", "Defeat"),
    ("goal", "Goal"),
    ("save", "Save"),
    ("epicsave", "Epic save"),
    ("demolition", "Demolition"),
    ("knock", "Knock"),
];

/// "2k" → "2K", "epicsave" → "Epic save". A win is a round in CS2, the match in Fortnite.
fn moment_name(kind: &str, game: &str) -> String {
    if kind == "victory" && game == "Fortnite" {
        return "Victory Royale".into();
    }
    if let Some((_, name)) = MOMENTS.iter().find(|(k, _)| *k == kind) {
        return (*name).into();
    }
    let multi_kill = kind.len() >= 2 && kind.ends_with('k') && kind[..kind.len() - 1].bytes().all(|b| b.is_ascii_digit());
    if multi_kill { kind.to_uppercase() } else { kind.to_string() }
}

/// "de_mirage" → "Mirage", "de_dust2" → "Dust II"; names the game reports plainly stay.
fn map_name(map: &str) -> String {
    match map {
        "de_dust2" => return "Dust II".into(),
        "Map11" => return "Summoner's Rift".into(),
        "Map12" => return "Howling Abyss".into(),
        _ => {}
    }
    let bare = ["de_", "cs_", "ar_", "dz_", "gd_", "aim_", "awp_", "fy_"].iter().find_map(|p| map.strip_prefix(p)).unwrap_or(map);
    let mut name = String::new();
    for word in bare.split('_').filter(|w| !w.is_empty()) {
        if !name.is_empty() {
            name.push(' ');
        }
        let mut letters = word.chars();
        if let Some(first) = letters.next() {
            name.extend(first.to_uppercase());
            name.push_str(letters.as_str());
        }
    }
    name
}

/// What the site is told about a clip, from its info file: a title like
/// "ACE on Mirage" when the clip has a moment, the game, the moments in it
/// as tags, and the map.
fn describe(info: &Value, folder: &str) -> (String, Option<String>, Vec<String>, Option<String>) {
    let game = info["game"].as_str().filter(|g| !g.is_empty()).unwrap_or(folder);
    let map = info["map"].as_str().unwrap_or_default();
    let reason = info["reason"].as_str().filter(|r| !r.is_empty());
    let title = match (reason, map_name(map)) {
        (Some(reason), place) if !place.is_empty() => format!("{} on {place}", moment_name(reason, game)),
        (Some(reason), _) => moment_name(reason, game),
        (None, _) => String::new(),
    };
    let mut tags: Vec<String> = Vec::new();
    let moments = info["events"].as_array().into_iter().flatten().filter_map(|e| e["kind"].as_str());
    for kind in reason.into_iter().chain(moments) {
        // Every hotkey clip has a press; it says nothing about what's in it.
        if !kind.is_empty() && kind != "capture" && tags.len() < 8 && !tags.iter().any(|t| t == kind) {
            tags.push(kind.to_string());
        }
    }
    let game = Some(game.to_string()).filter(|g| g != "Desktop");
    (title, game, tags, Some(map.to_string()).filter(|m| !m.is_empty()))
}

/// The clips a pass uploads, newest first: the ones the account asked for,
/// saved since auto-backup was turned on (`since`, seconds since 1970), that
/// aren't on the account yet, that have settled, and that the plan didn't
/// already turn down.
fn queue<'a>(clips: &'a [Clip], mode: Mode, since: u64, ledger: &Ledger, plan: &Plan) -> Vec<&'a Clip> {
    let mut wanted: Vec<&Clip> =
        clips.iter().filter(|c| mode.wants(c) && c.recorded >= since && c.settled && !ledger.backed_up(&c.key) && !ledger.skips(c, plan)).collect();
    wanted.sort_by(|a, b| b.recorded.cmp(&a.recorded).then_with(|| a.key.cmp(&b.key)));
    wanted
}

#[derive(Debug, PartialEq)]
enum Fit {
    Yes,
    /// The plan never takes this file: remembered, so it isn't tried again until the plan or the file changes.
    Never(String),
    /// Not now: the account is full.
    NoSpace,
}

fn plan_name(id: &str) -> &'static str {
    match id {
        "plus" => "Plus",
        "pro" => "Pro",
        _ => "Free",
    }
}

fn size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{mb:.1} MB") }
}

fn count(n: u32) -> String {
    if n == 1 { "1 clip".into() } else { format!("{n} clips") }
}

/// Whether the plan takes this clip, checked before anything is sent. (The
/// site checks the file itself afterwards: resolution, frame rate.)
fn fit(plan: &Plan, used: u64, clip: &Clip) -> Fit {
    let name = plan_name(&plan.id);
    if clip.bytes > plan.max_file {
        return Fit::Never(format!("it is {}, and {name} takes clips up to {}", size(clip.bytes), size(plan.max_file)));
    }
    if clip.seconds.is_some_and(|s| s > f64::from(plan.max_seconds) + 2.0) {
        return Fit::Never(format!("it is longer than the {} minutes {name} takes", plan.max_seconds / 60));
    }
    if used.saturating_add(clip.bytes) > plan.storage { Fit::NoSpace } else { Fit::Yes }
}

/// What a pass did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pass {
    /// Why it ended before everything was up, in a sentence for the log. Empty when there was nothing more to do.
    pub note: String,
    pub uploaded: u32,
    pub uploaded_bytes: u64,
    /// Clips that are still to go up.
    pub waiting: u32,
    /// Clips the plan doesn't take, found in this pass.
    pub skipped: u32,
}

impl Pass {
    fn ended(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }
}

/// One pass over `clips_dir` for whoever is signed in on this PC. Does
/// nothing, and says nothing, when nobody is signed in or auto-backup is off.
/// `cancel` stops it from another thread (a game came to the front); the
/// upload in progress is dropped and starts over in a later pass. `said`
/// hears a line for the log about every clip that went up or wasn't taken.
pub fn run(clips_dir: &Path, version: &str, cancel: &AtomicBool, said: &mut dyn FnMut(&str)) -> Pass {
    let pass = Pass::default();
    let client = match Client::new(crate::site(), version) {
        Ok(client) => client,
        Err(e) => return pass.ended(format!("couldn't start ({e:#})")),
    };
    let Some(saved) = token::load(&client.origin()) else { return pass };
    let account = match client.account(&saved) {
        Ok(account) => account,
        Err(e) if matches!(e.downcast_ref::<ApiError>(), Some(ApiError::SignedOut)) => return pass.ended("this PC isn't signed in to klipp.dev anymore"),
        Err(e) => return pass.ended(format!("klipp.dev couldn't be asked ({e:#})")),
    };
    let mode = Mode::parse(&account.backup);
    if mode == Mode::Off || !account.plan.auto_backup {
        return pass;
    }
    if account.status != "active" {
        return pass.ended("the account can't upload right now; its notifications on klipp.dev say why");
    }
    let mut ledger = match Ledger::load(clips_dir) {
        Ok(ledger) => ledger.for_account(&account.id),
        Err(e) => return pass.ended(format!("not uploading, so nothing goes up twice: {e:#}")),
    };
    let now = SystemTime::now();
    let since = match (account.backup_since, ledger.since) {
        (Some(since), _) => since,
        (None, Some(first_seen)) => first_seen,
        // The site doesn't say when it was turned on: it counts from this pass, and that is written down first.
        (None, None) => {
            let first_seen = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
            ledger.since = Some(first_seen);
            if let Err(e) = ledger.save(clips_dir) {
                return pass.ended(format!("not uploading yet: {e:#}"));
            }
            first_seen
        }
    };
    let clips = scan(clips_dir, now);
    upload_queue(&client, &saved, &account, clips_dir, &clips, mode, since, ledger, cancel, said)
}

#[allow(clippy::too_many_arguments)]
fn upload_queue(client: &Client, saved: &str, account: &Account, clips_dir: &Path, clips: &[Clip], mode: Mode, since: u64, mut ledger: Ledger, cancel: &AtomicBool, said: &mut dyn FnMut(&str)) -> Pass {
    let mut pass = Pass::default();
    let plan = &account.plan;
    let wanted = queue(clips, mode, since, &ledger, plan);
    if wanted.is_empty() {
        return pass;
    }
    // If the ledger can't be written, an uploaded clip would go up again next time: find that out first.
    if let Err(e) = ledger.save(clips_dir) {
        return pass.ended(format!("not uploading, so nothing goes up twice: {e:#}"));
    }
    let mut used = plan.used;
    let mut no_space = 0;
    let mut left = wanted.len() as u32;
    for clip in wanted {
        if cancel.load(Ordering::Relaxed) {
            pass.note = "stopped while a game is in front".into();
            break;
        }
        if pass.uploaded >= PASS_LIMIT {
            break;
        }
        match fit(plan, used, clip) {
            Fit::Yes => {}
            Fit::NoSpace => {
                no_space += 1;
                continue;
            }
            Fit::Never(why) => {
                said(&format!("{} stays on this PC only: {why}", clip.key));
                ledger.skipped.insert(clip.key.clone(), Skipped { why, plan: plan.id.clone(), bytes: clip.bytes });
                let _ = ledger.save(clips_dir);
                pass.skipped += 1;
                left -= 1;
                continue;
            }
        }
        let thumb = thumbnail(clips_dir, clip);
        let new = NewUpload { title: clip.title.clone(), game: clip.game.clone(), tags: clip.tags.clone(), map: clip.map.clone(), visibility: "private".into(), backup: true, marked: false };
        match upload(client, saved, &clip.path, thumb.as_deref(), &new, cancel, &mut |_| {}) {
            Ok(done) => {
                let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
                ledger.skipped.remove(&clip.key);
                ledger.done.insert(clip.key.clone(), Done { id: done.id.clone(), at, bytes: clip.bytes });
                if let Err(e) = ledger.save(clips_dir) {
                    // Unremembered, it would go up again: take it back and stop.
                    let _ = client.delete_clip(saved, &done.id);
                    pass.note = format!("stopped, so nothing goes up twice: {e:#}");
                    break;
                }
                used = used.saturating_add(clip.bytes);
                pass.uploaded += 1;
                pass.uploaded_bytes += clip.bytes;
                left -= 1;
                said(&format!("{} is on klipp.dev ({})", clip.key, size(clip.bytes)));
            }
            Err(e) => {
                match e.downcast_ref::<ApiError>() {
                    Some(ApiError::Rejected(why)) => {
                        said(&format!("{} stays on this PC only: {why}", clip.key));
                        ledger.skipped.insert(clip.key.clone(), Skipped { why: why.clone(), plan: plan.id.clone(), bytes: clip.bytes });
                        let _ = ledger.save(clips_dir);
                        pass.skipped += 1;
                        left -= 1;
                        continue;
                    }
                    Some(ApiError::SignedOut) => pass.note = "this PC isn't signed in to klipp.dev anymore".into(),
                    Some(ApiError::TooMany(_)) => pass.note = "klipp.dev asked to slow down; the rest goes up later".into(),
                    Some(ApiError::Refused(why)) => pass.note = format!("klipp.dev didn't take the upload: {why}"),
                    _ if e.is::<Cancelled>() => pass.note = "stopped while a game is in front".into(),
                    _ => pass.note = format!("an upload didn't go through ({e:#}); it is tried again later"),
                }
                break;
            }
        }
    }
    pass.waiting = left;
    if pass.note.is_empty() && no_space > 0 {
        pass.note = format!("the account is full ({} of {}): {} wait until there is room", size(used), size(plan.storage), count(no_space));
    }
    pass
}

/// The clip's saved thumbnail, when it is one the site takes.
fn thumbnail(clips_dir: &Path, clip: &Clip) -> Option<Vec<u8>> {
    let (folder, stem) = clip.key.split_once('/')?;
    let path = meta_file(clips_dir, folder, stem, "thumbs", "jpg");
    let len = fs::metadata(&path).ok()?.len();
    if !(100..=THUMB_MAX).contains(&len) {
        return None;
    }
    fs::read(&path).ok()
}
