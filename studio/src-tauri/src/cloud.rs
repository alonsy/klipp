//! The window's side of the account on klipp.dev (the work is in the
//! klipp-cloud crate): signing this PC in with a code approved in the browser,
//! showing who is signed in, and uploading an exported clip for a link.
//! Nothing here runs unless the person asks for it; Klipp works without it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use klipp_cloud::{Account, ApiError, Cancelled, Client, LinkPoll, NewUpload, token};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::ffmpeg;

#[derive(Default)]
pub struct Cloud {
    next: AtomicU64,
    /// Uploads in progress, each with the flag that stops it.
    uploads: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    /// The sign-in being waited on, if any; setting its flag ends the wait.
    linking: Mutex<Option<Arc<AtomicBool>>>,
}

fn client() -> Result<Client> {
    Client::new(klipp_cloud::site(), env!("CARGO_PKG_VERSION"))
}

fn signed_out(error: &anyhow::Error) -> bool {
    matches!(error.downcast_ref::<ApiError>(), Some(ApiError::SignedOut))
}

/// What the Account page shows.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// A sign-in is saved on this PC.
    signed_in: bool,
    /// Who it is and what their plan allows; missing when klipp.dev couldn't be reached.
    account: Option<Account>,
    /// Why the account couldn't be read, when signed in.
    error: Option<String>,
    /// Clips from this PC's clips folder that auto-backup put on the account.
    backed_up: usize,
}

/// Who is signed in on this PC. A sign-in the site no longer accepts is forgotten.
pub fn status(clips_dir: &Path) -> Result<Status> {
    let client = client()?;
    let origin = client.origin();
    let Some(saved) = token::load(&origin) else {
        return Ok(Status { signed_in: false, account: None, error: None, backed_up: 0 });
    };
    let backed_up = klipp_cloud::backup::Ledger::load(clips_dir).map(|ledger| ledger.count()).unwrap_or(0);
    Ok(match client.account(&saved) {
        Ok(account) => Status { signed_in: true, account: Some(account), error: None, backed_up },
        Err(e) if signed_out(&e) => {
            let _ = token::forget(&origin);
            Status { signed_in: false, account: None, error: None, backed_up: 0 }
        }
        Err(e) => Status { signed_in: true, account: None, error: Some(format!("{e:#}")), backed_up },
    })
}

/// The code to show while the browser is open on the approval page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkInfo {
    code: String,
    /// The page that was opened, in case the browser didn't come up.
    url: String,
    expires_in: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LinkEvent {
    /// "linked", "denied", "expired", "cancelled" or "failed".
    state: &'static str,
    handle: Option<String>,
    error: Option<String>,
}

impl Cloud {
    /// Starts signing in: asks the site for a code, opens the approval page
    /// in the browser, and waits in the background. How it ends arrives as a
    /// `cloud-link` event.
    pub fn link(&self, app: AppHandle) -> Result<LinkInfo> {
        self.cancel_link();
        let client = client()?;
        let start = client.link_start(&klipp_cloud::device_name()).map_err(|e| match e.downcast_ref::<ApiError>() {
            // A copy of the site without accounts (before they open): say so plainly.
            Some(ApiError::Site(404)) => anyhow::anyhow!("Klipp accounts aren't open yet."),
            _ => e,
        })?;
        // Only the site's own approval page is opened, whatever the answer says.
        let origin = client.origin();
        if !start.verification_uri_complete.starts_with(&format!("{origin}/link?code=")) || start.verification_uri_complete.chars().any(|c| c.is_control() || c == ' ' || c == '"') {
            bail!("klipp.dev sent an address Klipp doesn't recognise");
        }
        let _ = std::process::Command::new("explorer.exe").arg(&start.verification_uri_complete).spawn();

        let stop = Arc::new(AtomicBool::new(false));
        *self.linking.lock().unwrap() = Some(stop.clone());
        let info = LinkInfo { code: start.user_code.clone(), url: start.verification_uri_complete.clone(), expires_in: start.expires_in };
        std::thread::spawn(move || {
            let end = |state: &'static str, handle: Option<String>, error: Option<String>| {
                let _ = app.emit("cloud-link", LinkEvent { state, handle, error });
            };
            let until = Instant::now() + Duration::from_secs(start.expires_in.clamp(60, 1800));
            let mut wait = Duration::from_secs(start.interval.clamp(2, 30));
            let mut failures = 0;
            loop {
                let slept = Instant::now();
                while slept.elapsed() < wait {
                    if stop.load(Ordering::Relaxed) {
                        return end("cancelled", None, None);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                if Instant::now() > until {
                    return end("expired", None, None);
                }
                match client.link_poll(&start.device_code) {
                    Ok(LinkPoll::Pending) => failures = 0,
                    Ok(LinkPoll::SlowDown) => wait += Duration::from_secs(5),
                    Ok(LinkPoll::Denied) => return end("denied", None, None),
                    Ok(LinkPoll::Expired) => return end("expired", None, None),
                    Ok(LinkPoll::Linked { token: new, handle }) => {
                        // Cancelled in the same moment it was approved: don't keep a sign-in nobody is waiting for.
                        if stop.load(Ordering::Relaxed) {
                            let _ = client.sign_out(&new);
                            return end("cancelled", None, None);
                        }
                        return match token::save(&origin, &new) {
                            Ok(()) => end("linked", Some(handle), None),
                            Err(e) => {
                                let _ = client.sign_out(&new);
                                end("failed", None, Some(format!("{e:#}")))
                            }
                        };
                    }
                    // A dropped connection while waiting isn't the end; a site that keeps failing is.
                    Err(e) => {
                        failures += 1;
                        if failures >= 5 {
                            return end("failed", None, Some(format!("{e:#}")));
                        }
                    }
                }
            }
        });
        Ok(info)
    }

    pub fn cancel_link(&self) {
        if let Some(stop) = self.linking.lock().unwrap().take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// An upload is running (an update would cut it off).
    pub fn busy(&self) -> bool {
        !self.uploads.lock().unwrap().is_empty()
    }

    pub fn cancel_upload(&self, id: u64) {
        if let Some(stop) = self.uploads.lock().unwrap().get(&id) {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Stops every upload (the window is closing).
    pub fn cancel_all(&self) {
        for stop in self.uploads.lock().unwrap().values() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Uploads a clip from the clips folder; progress and the result arrive
    /// as `upload-progress`, `upload-done` and `upload-failed` events.
    pub fn upload(&self, app: AppHandle, clips_dir: PathBuf, cache_dir: PathBuf, request: UploadRequest) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let stop = Arc::new(AtomicBool::new(false));
        self.uploads.lock().unwrap().insert(id, stop.clone());
        let uploads = self.uploads.clone();
        std::thread::spawn(move || {
            let mut last = -1i64;
            let result = run_upload(&clips_dir, &cache_dir, &request, &stop, &mut |fraction| {
                // A step per half percent is plenty for a progress ring.
                let step = (fraction * 200.0) as i64;
                if step != last {
                    last = step;
                    let _ = app.emit("upload-progress", UploadProgress { id, fraction });
                }
            });
            uploads.lock().unwrap().remove(&id);
            let _ = match result {
                Ok(done) => app.emit("upload-done", UploadDone { id, clip_id: done.id, url: done.url, visibility: done.visibility }),
                Err(e) => app.emit("upload-failed", UploadFailed { id, cancelled: e.is::<Cancelled>(), signed_out: signed_out(&e), error: format!("{e:#}") }),
            };
        });
        id
    }
}

/// Signs this PC out: ends the token on the site and removes it here. False
/// when the site couldn't be told (it is removed here either way, and the
/// person can remove the PC under their account's devices).
pub fn sign_out() -> Result<bool> {
    let client = client()?;
    let origin = client.origin();
    let told = match token::load(&origin) {
        Some(saved) => client.sign_out(&saved).is_ok(),
        None => true,
    };
    token::forget(&origin)?;
    Ok(told)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRequest {
    path: String,
    title: String,
    /// The game as Klipp names it ("Counter-Strike 2").
    game: Option<String>,
    /// Moments in the clip, e.g. "ace", "headshot".
    #[serde(default)]
    tags: Vec<String>,
    map: Option<String>,
    /// "public", "unlisted" or "private".
    visibility: String,
    /// How long the file plays, when the window knows.
    duration_secs: Option<f64>,
    /// The export burned the Klipp tab into this file (see `export`).
    #[serde(default)]
    marked: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UploadProgress {
    id: u64,
    fraction: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UploadDone {
    id: u64,
    clip_id: String,
    url: String,
    visibility: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UploadFailed {
    id: u64,
    error: String,
    cancelled: bool,
    signed_out: bool,
}

fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{mb:.0} MB") }
}

/// What the plan would refuse, said before anything is sent. (The site checks
/// again on its side; this only saves an upload that can't succeed.)
fn plan_problem(account: &Account, bytes: u64, duration_secs: Option<f64>) -> Option<String> {
    let plan = &account.plan;
    let name = match plan.id.as_str() {
        "plus" => "Plus",
        "pro" => "Pro",
        _ => "Free",
    };
    if account.status != "active" {
        return Some("This account can't upload right now. See your notifications on klipp.dev.".into());
    }
    if bytes > plan.max_file {
        return Some(format!("Clips can be up to {} on {name}. This one is {}.", megabytes(plan.max_file), megabytes(bytes)));
    }
    if plan.used.saturating_add(bytes) > plan.storage {
        return Some(format!("Not enough space on your account: {} of {} used. Delete some clips on klipp.dev or pick a bigger plan.", megabytes(plan.used), megabytes(plan.storage)));
    }
    match duration_secs {
        Some(secs) if secs > f64::from(plan.max_seconds) + 2.0 => Some(format!("{name} uploads go up to {} minutes. Trim this clip or pick a bigger plan.", plan.max_seconds / 60)),
        _ => None,
    }
}

/// The file to upload, which must be one of Klipp's own MP4s: nothing outside the clips folder leaves this PC.
fn clip_file(clips_dir: &Path, path: &str) -> Result<PathBuf> {
    let file = std::fs::canonicalize(path).context("that clip isn't there anymore")?;
    let root = std::fs::canonicalize(clips_dir).context("the clips folder isn't there")?;
    let is_mp4 = file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("mp4"));
    if !file.starts_with(&root) || !is_mp4 || !file.is_file() {
        bail!("only clips from your clips folder can be uploaded");
    }
    Ok(file)
}

fn run_upload(clips_dir: &Path, cache_dir: &Path, request: &UploadRequest, stop: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<klipp_cloud::Uploaded> {
    let file = clip_file(clips_dir, &request.path)?;
    let client = client()?;
    let origin = client.origin();
    let saved = token::load(&origin).ok_or(ApiError::SignedOut)?;
    let account = client.account(&saved).inspect_err(|e| {
        if signed_out(e) {
            let _ = token::forget(&origin);
        }
    })?;
    let bytes = std::fs::metadata(&file)?.len();
    if let Some(problem) = plan_problem(&account, bytes, request.duration_secs) {
        return Err(ApiError::Refused(problem).into());
    }
    // A picture for the clip's page and for link previews; the upload goes on without one.
    let poster = ffmpeg::poster(&file, cache_dir, request.duration_secs.map(|d| (d / 2.0).min(3.0)).unwrap_or(1.0)).ok();
    let clip = NewUpload {
        title: request.title.trim().chars().take(100).collect(),
        game: request.game.clone().filter(|g| !g.is_empty() && g != "Desktop"),
        tags: request.tags.iter().take(8).cloned().collect(),
        map: request.map.clone().filter(|m| !m.is_empty()),
        visibility: request.visibility.clone(),
        backup: false,
        marked: request.marked,
    };
    klipp_cloud::upload(&client, &saved, &file, poster.as_deref(), &clip, stop, &mut |p| progress(p.sent as f64 / p.total.max(1) as f64)).inspect_err(|e| {
        if signed_out(e) {
            let _ = token::forget(&origin);
        }
    })
}
