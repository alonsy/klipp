//! Auto-backup (Plus and Pro, chosen on klipp.dev): clips go to the person's
//! Klipp account by themselves, privately. The work is klipp-cloud's
//! (`klipp_cloud::backup`); this decides when. A pass runs a few minutes
//! after the recorder starts and then every ten minutes, only when no game
//! has been in front for a while and no clip is being saved, on its own
//! low-priority thread. A game coming to the front stops the upload in
//! progress, and it starts over in a later pass: a match never shares the
//! connection with a backup. Without a sign-in on this PC a pass is one look
//! in Windows Credential Manager and nothing else.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use klipp_cloud::backup::{self, Pass};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentThread, ReleaseMutex, SetThreadPriority, THREAD_PRIORITY_LOWEST};
use windows::core::w;

use crate::games::DESKTOP;
use crate::log::{info, warning};
use crate::settings::Storage;

/// The first pass, this long after the recorder starts.
const FIRST_AFTER: Duration = Duration::from_secs(150);
/// Then this often.
const EVERY: Duration = Duration::from_secs(600);
/// Quitting waits this long for an upload to let go.
const STOP_WAIT: Duration = Duration::from_secs(4);

pub struct Backup {
    next: Instant,
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    /// How the last pass ended, so the log says it once and not every ten minutes.
    last_note: Arc<Mutex<String>>,
}

impl Backup {
    pub fn new() -> Self {
        Self { next: Instant::now() + FIRST_AFTER, running: Arc::default(), cancel: Arc::default(), last_note: Arc::default() }
    }

    /// Starts a pass on its own thread if it's time, and stops the one that
    /// is running when a game comes to the front. `game`: what was in front
    /// lately ("Desktop" for none); `saving`: a clip is on its way.
    pub fn tick(&mut self, storage: &Storage, game: &str, saving: bool) {
        if self.running.load(Ordering::Relaxed) {
            if game != DESKTOP {
                self.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }
        if !self.due(game, saving, Instant::now()) {
            return;
        }
        let dir = storage.clips_dir();
        let (running, cancel, last_note) = (self.running.clone(), self.cancel.clone(), self.last_note.clone());
        cancel.store(false, Ordering::Relaxed);
        running.store(true, Ordering::Relaxed);
        let spawned = std::thread::Builder::new().name("backup".into()).spawn(move || {
            // Reading clips and sending them stays behind everything else this PC does.
            let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST) };
            if let Some(pass) = pass(&dir, &cancel) {
                report(&pass, &mut last_note.lock().unwrap());
            }
            running.store(false, Ordering::Relaxed);
        });
        if let Err(e) = spawned {
            self.running.store(false, Ordering::Relaxed);
            warning!("backup: couldn't start: {e}");
        }
    }

    fn due(&mut self, game: &str, saving: bool, now: Instant) -> bool {
        if now < self.next || game != DESKTOP || saving {
            return false;
        }
        self.next = now + EVERY;
        true
    }

    /// Klipp is quitting: lets go of the upload in progress (it starts over next time).
    pub fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let asked = Instant::now();
        while self.running.load(Ordering::Relaxed) && asked.elapsed() < STOP_WAIT {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Held during a pass, so two never run at once (the recorder's own and a
/// `--backup-now`): they would upload the same clips twice.
struct PassLock(HANDLE);

impl PassLock {
    fn take() -> Option<Self> {
        let handle = unsafe { CreateMutexW(None, true, w!("Local\\KlippBackupPass")) }.ok()?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return None;
        }
        Some(Self(handle))
    }
}

impl Drop for PassLock {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// One pass, with a log line for every clip. `None` when another pass is running.
fn pass(dir: &Path, cancel: &AtomicBool) -> Option<Pass> {
    let _lock = PassLock::take()?;
    Some(backup::run(dir, env!("CARGO_PKG_VERSION"), cancel, &mut |line| info!("backup: {line}")))
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// What a pass did, in a line; nothing when it did nothing.
fn summary(pass: &Pass) -> Option<String> {
    if pass.uploaded == 0 && pass.skipped == 0 {
        return None;
    }
    let clips = |n: u32| if n == 1 { "1 clip".to_string() } else { format!("{n} clips") };
    let mut parts = vec![format!("{} uploaded ({})", clips(pass.uploaded), megabytes(pass.uploaded_bytes))];
    if pass.skipped > 0 {
        parts.push(format!("{} the plan doesn't take", clips(pass.skipped)));
    }
    if pass.waiting > 0 {
        parts.push(format!("{} still to go", clips(pass.waiting)));
    }
    Some(parts.join(", "))
}

fn report(pass: &Pass, last_note: &mut String) {
    if let Some(line) = summary(pass) {
        info!("backup: {line}");
    }
    if pass.note != *last_note {
        if !pass.note.is_empty() {
            info!("backup: {}", pass.note);
        }
        last_note.clone_from(&pass.note);
    }
}

/// `--backup-now`: one pass right away, whatever is in front, saying what it did.
pub fn run_now(dir: &Path) {
    let cancel = AtomicBool::new(false);
    match pass(dir, &cancel) {
        None => info!("backup: another pass is running"),
        Some(pass) => {
            if summary(&pass).is_none() && pass.note.is_empty() {
                info!("backup: nothing to upload (nobody signed in, auto-backup off, or everything is on klipp.dev already)");
            }
            report(&pass, &mut String::new());
        }
    }
}
