//! Deletes old clips by itself (Settings → Storage, off by default): clips
//! older than `storage.delete_after_days`, and the oldest ones while all clips
//! together take more than `storage.max_gb`. Favorites are never deleted
//! (they still count toward the total). Runs a couple of minutes after the
//! recorder starts and then about hourly, only between games and never while
//! a clip is being saved, on its own thread so the hotkey stays responsive.
//! Deleted clips go to the Recycle Bin; their info and thumbnail in `.klipp`
//! are removed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW};
use windows::core::PCWSTR;

use crate::games::DESKTOP;
use crate::log::{info, warning};
use crate::save::META_DIR;
use crate::settings::Storage;

/// The first run, this long after the recorder starts (or the settings change).
const FIRST_AFTER: Duration = Duration::from_secs(120);
/// Then about this often.
const EVERY: Duration = Duration::from_secs(3600);
/// Clips modified this recently are left alone (being saved, trimmed or squeezed).
const RECENT: Duration = Duration::from_secs(600);
/// The window's exports: not clips, never deleted (the studio's `export::EXPORTS_DIR`).
const EXPORTS_DIR: &str = "Exports";
/// `max_gb` counts in the same GB the window shows sizes in.
const GB: u64 = 1_000_000_000;
const DAY: i64 = 86_400;

/// A clip as the selection sees it.
#[derive(Clone, Debug)]
pub struct Clip {
    pub path: PathBuf,
    /// When it was recorded: local time in seconds, only compared with `now`.
    pub time: i64,
    pub bytes: u64,
    pub favorite: bool,
    /// Modified in the last few minutes: counts toward the total, never deleted.
    pub recent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Older than `delete_after_days`.
    Age,
    /// The oldest while the clips took more than `max_gb`.
    Size,
}

/// Which clips go and why. `now` is in the same local seconds as `Clip::time`;
/// `days` 0 is no age limit, `max_bytes` 0 no size limit.
pub fn select(clips: &[Clip], now: i64, days: u32, max_bytes: u64) -> Vec<(usize, Why)> {
    let mut chosen: Vec<Option<Why>> = vec![None; clips.len()];
    let deletable = |c: &Clip| !c.favorite && !c.recent;
    if days > 0 {
        let cutoff = now - days as i64 * DAY;
        for (i, c) in clips.iter().enumerate() {
            if deletable(c) && c.time < cutoff {
                chosen[i] = Some(Why::Age);
            }
        }
    }
    if max_bytes > 0 {
        let mut total: u64 = clips.iter().zip(&chosen).filter(|(_, w)| w.is_none()).map(|(c, _)| c.bytes).sum();
        let mut oldest: Vec<usize> = (0..clips.len()).filter(|&i| chosen[i].is_none() && deletable(&clips[i])).collect();
        oldest.sort_by_key(|&i| clips[i].time);
        for i in oldest {
            if total <= max_bytes {
                break;
            }
            chosen[i] = Some(Why::Size);
            total -= clips[i].bytes;
        }
    }
    chosen.into_iter().enumerate().filter_map(|(i, w)| w.map(|w| (i, w))).collect()
}

/// Decides when to run, from the recorder's games timer.
pub struct Cleanup {
    next: Instant,
    /// The storage settings last seen; a change runs it again soon.
    seen: Option<Storage>,
    running: Arc<AtomicBool>,
}

impl Cleanup {
    pub fn new() -> Self {
        Self { next: Instant::now() + FIRST_AFTER, seen: None, running: Arc::default() }
    }

    /// Starts a run on its own thread if it's time. `game`: what was in front
    /// lately ("Desktop" for none); `saving`: a clip is on its way.
    pub fn tick(&mut self, storage: &Storage, game: &str, saving: bool) {
        if !self.due(storage, game, saving, Instant::now()) {
            return;
        }
        let (dir, days, max_gb) = (storage.clips_dir(), storage.delete_after_days, storage.max_gb);
        let running = self.running.clone();
        running.store(true, Ordering::Relaxed);
        let spawned = std::thread::Builder::new().name("cleanup".into()).spawn(move || {
            run(&dir, days, max_gb);
            running.store(false, Ordering::Relaxed);
        });
        if let Err(e) = spawned {
            self.running.store(false, Ordering::Relaxed);
            warning!("cleanup: couldn't start: {e}");
        }
    }

    fn due(&mut self, storage: &Storage, game: &str, saving: bool, now: Instant) -> bool {
        if self.seen.as_ref() != Some(storage) {
            // Turned on or changed: apply it soon rather than within the hour.
            if self.seen.is_some() {
                self.next = self.next.min(now + FIRST_AFTER);
            }
            self.seen = Some(storage.clone());
        }
        let limits = storage.delete_after_days > 0 || storage.max_gb > 0;
        if !storage.auto_delete || !limits || now < self.next || game != DESKTOP || saving || self.running.load(Ordering::Relaxed) {
            return false;
        }
        self.next = now + EVERY;
        true
    }
}

/// One run: finds what goes, recycles it and logs one line.
fn run(dir: &Path, days: u32, max_gb: u32) {
    // SHFileOperation wants COM on the calling thread.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let now = local_now();
    let clips = scan(dir, now - unix_now());
    let picked = select(&clips, now, days, max_gb as u64 * GB);
    let (mut age, mut size, mut failed) = ((0, 0u64), (0, 0u64), 0);
    for &(i, why) in &picked {
        let clip = &clips[i];
        if !recycle(&clip.path) {
            failed += 1;
            continue;
        }
        for meta in [meta_file(dir, &clip.path, "info", "json"), meta_file(dir, &clip.path, "thumbs", "jpg")] {
            let _ = fs::remove_file(meta);
        }
        let n = if why == Why::Age { &mut age } else { &mut size };
        n.0 += 1;
        n.1 += clip.bytes;
    }
    let total: u64 = clips.iter().map(|c| c.bytes).sum::<u64>() - age.1 - size.1;
    let left = format!("{} left, {}", count(clips.len() - age.0 - size.0), gb(total));
    let mut parts = Vec::new();
    if age.0 > 0 {
        parts.push(format!("{} older than {days} days ({})", count(age.0), gb(age.1)));
    }
    if size.0 > 0 {
        parts.push(format!("{} to stay under {max_gb} GB ({})", count(size.0), gb(size.1)));
    }
    if parts.is_empty() {
        info!("cleanup: nothing to delete ({left})");
    } else {
        info!("cleanup: deleted {} to the Recycle Bin; {left}", parts.join(" and "));
    }
    if failed > 0 {
        warning!("cleanup: {} couldn't be deleted (open in another app?)", count(failed));
    }
}

fn count(n: usize) -> String {
    if n == 1 { "1 clip".into() } else { format!("{n} clips") }
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GB as f64)
}

/// Every clip in the game folders (not exports, not `.klipp` or `.trash`).
/// `offset`: local minus UTC seconds, for clips whose name has no date.
fn scan(dir: &Path, offset: i64) -> Vec<Clip> {
    let mut clips = Vec::new();
    for game in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = game.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.eq_ignore_ascii_case(EXPORTS_DIR) || !game.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for file in fs::read_dir(game.path()).into_iter().flatten().flatten() {
            let path = file.path();
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4")) {
                continue;
            }
            let Ok(meta) = file.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().ok();
            // A time in the future counts as recent too.
            let recent = modified.and_then(|m| m.elapsed().ok()).is_none_or(|age| age < RECENT);
            let from_name = path.file_stem().and_then(|s| name_time(&s.to_string_lossy()));
            let from_file = modified.and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64 + offset);
            let Some(time) = from_name.or(from_file) else { continue };
            // Only clips Klipp saved (they have its info): never another
            // app's videos in a clips folder that's shared, like Videos.
            let info = meta_file(dir, &path, "info", "json");
            if !info.exists() {
                continue;
            }
            let favorite = favorite(&info);
            clips.push(Clip { path, time, bytes: meta.len(), favorite, recent });
        }
    }
    clips
}

/// `<clips>\.klipp\<kind>\<game>\<stem>.<ext>` (see save.rs).
fn meta_file(dir: &Path, clip: &Path, kind: &str, ext: &str) -> PathBuf {
    let game = clip.parent().and_then(Path::file_name).unwrap_or_default();
    let stem = clip.file_stem().unwrap_or_default();
    dir.join(META_DIR).join(kind).join(game).join(stem).with_extension(ext)
}

/// The info file says `"favorite": true`. Tolerates a UTF-8 BOM.
fn favorite(info: &Path) -> bool {
    let Ok(text) = fs::read_to_string(info) else { return false };
    serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')).is_ok_and(|v| v["favorite"] == true)
}

/// Sends one file to the Recycle Bin, without asking or showing anything.
fn recycle(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    // A list of paths, each null-terminated, ending with an extra null.
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.extend([0, 0]);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let result = unsafe { SHFileOperationW(&mut op) };
    result == 0 && !op.fAnyOperationsAborted.as_bool() && !path.exists()
}

/// Days since 1970-01-01 of a calendar date (Howard Hinnant's days_from_civil).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A local date and time as seconds, as if it were UTC (only compared with each other).
fn local_secs(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    days_from_civil(y, mo, d) * DAY + h * 3600 + mi * 60 + s
}

/// "2026-10-13_21-29-01_2k" → when it was recorded (local seconds).
fn name_time(stem: &str) -> Option<i64> {
    let b = stem.as_bytes();
    if b.len() < 19 || !matches!(b.get(19), None | Some(b'_')) {
        return None;
    }
    let num = |from: usize, len: usize| -> Option<i64> {
        let s = stem.get(from..from + len)?;
        s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten()
    };
    let seps = [(4, b'-'), (7, b'-'), (10, b'_'), (13, b'-'), (16, b'-')];
    if seps.iter().any(|&(i, c)| b[i] != c) {
        return None;
    }
    let (y, mo, d, h, mi, s) = (num(0, 4)?, num(5, 2)?, num(8, 2)?, num(11, 2)?, num(14, 2)?, num(17, 2)?);
    let valid = (1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && s < 61;
    valid.then(|| local_secs(y, mo, d, h, mi, s))
}

fn local_now() -> i64 {
    let t = unsafe { GetLocalTime() };
    local_secs(t.wYear as i64, t.wMonth as i64, t.wDay as i64, t.wHour as i64, t.wMinute as i64, t.wSecond as i64)
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}
