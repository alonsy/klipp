//! Uploading one clip. The file goes straight to storage in pieces, through
//! addresses the site signs for this one upload; the site only hears how big
//! the file is and, at the end, that every piece arrived. It then checks the
//! file itself before the clip goes live.

use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::api::{ApiError, Client, DonePart, NewUpload, PartUrl, Ticket, Uploaded};
use crate::http::{Body, Cancelled};

/// A thumbnail the site will take: a JPEG of at most 1 MB.
const THUMB_MAX: usize = 1024 * 1024;
/// How often one piece is tried, and the waits in between.
const WAITS: [Duration; 3] = [Duration::from_secs(1), Duration::from_secs(3), Duration::from_secs(8)];

/// How far an upload is, in bytes of the clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub sent: u64,
    pub total: u64,
}

/// Where each piece of a file of `len` bytes starts and how long it is.
fn pieces(len: u64, part_size: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < len {
        let size = part_size.min(len - at);
        out.push((at, size));
        at += size;
    }
    out
}

/// Waits, unless the upload is cancelled meanwhile.
fn pause(wait: Duration, cancel: &AtomicBool) -> Result<()> {
    let step = Duration::from_millis(100);
    let mut left = wait;
    while !left.is_zero() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        std::thread::sleep(step.min(left));
        left = left.saturating_sub(step);
    }
    Ok(())
}

/// Uploads `file` as a new clip of the account behind `token` and returns its page.
///
/// `thumb` is a JPEG to show before the clip plays. `cancel` stops the upload
/// from another thread; a stopped or failed upload leaves nothing on the site.
/// `progress` hears about every 256 KB sent.
pub fn upload(client: &Client, token: &str, file: &Path, thumb: Option<&[u8]>, clip: &NewUpload, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Uploaded> {
    let mut source = File::open(file).with_context(|| format!("opening {}", file.display()))?;
    let len = source.metadata().context("reading the file's size")?.len();
    if len < 1024 {
        bail!("that file is too small to be a clip");
    }
    let thumb = thumb.filter(|jpeg| (100..=THUMB_MAX).contains(&jpeg.len()) && jpeg.starts_with(&[0xFF, 0xD8]));
    let ticket = client.start_upload(token, len, thumb.map(|t| t.len() as u64), clip)?;

    match send(client, token, &mut source, len, thumb, &ticket, cancel, progress) {
        Ok(done) => Ok(done),
        Err(error) => {
            // The site already dropped an upload it refused at the end; anything else is dropped here,
            // so a stopped upload doesn't hold space or count as one still running.
            if !matches!(error.downcast_ref::<ApiError>(), Some(ApiError::Refused(_) | ApiError::Rejected(_) | ApiError::SignedOut)) {
                let _ = client.delete_clip(token, &ticket.clip_id);
            }
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn send(client: &Client, token: &str, source: &mut File, len: u64, thumb: Option<&[u8]>, ticket: &Ticket, cancel: &AtomicBool, progress: &mut dyn FnMut(Progress)) -> Result<Uploaded> {
    let plan = pieces(len, ticket.part_size.max(1));
    if plan.len() != ticket.parts.len() || plan.is_empty() {
        bail!("klipp.dev planned the upload differently than Klipp expected");
    }
    let mut done = Vec::with_capacity(plan.len());
    let mut sent_before = 0u64;
    for (index, (offset, size)) in plan.iter().copied().enumerate() {
        let number = index as u32 + 1;
        let mut address = ticket.parts.iter().find(|p| p.part_number == number).map(|p| p.url.clone()).context("klipp.dev left a piece of the upload without an address")?;
        let mut refreshed = false;
        let mut attempt = 0;
        let etag = loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Cancelled.into());
            }
            let url = client.base().join(&address)?;
            let result = client.session().send("PUT", &url, &[], Body::File { file: source, offset, len: size }, &mut |bytes| {
                progress(Progress { sent: sent_before + bytes, total: len });
                !cancel.load(Ordering::Relaxed)
            });
            match result {
                Ok(answer) if (200..300).contains(&answer.status) => match answer.etag {
                    Some(etag) => break etag,
                    None => bail!("storage took a piece of the upload without confirming it"),
                },
                // The signed address stopped working (they last a few hours): ask for a fresh one, once.
                Ok(answer) if matches!(answer.status, 400 | 401 | 403) && !refreshed => {
                    refreshed = true;
                    let fresh: Vec<PartUrl> = client.part_urls(token, &ticket.clip_id, &[number])?;
                    address = fresh.into_iter().find(|p| p.part_number == number).map(|p| p.url).context("klipp.dev gave no fresh address for a piece of the upload")?;
                }
                Ok(answer) if answer.status < 500 => bail!("storage refused a piece of the upload ({})", answer.status),
                Err(error) if error.is::<Cancelled>() => return Err(error),
                // A dropped connection or a storage hiccup: wait and try this piece again.
                other => {
                    let Some(wait) = WAITS.get(attempt) else {
                        return Err(match other {
                            Err(error) => error.context("the upload kept failing; check your connection and try again"),
                            Ok(answer) => anyhow::anyhow!("storage kept failing ({}); try again in a moment", answer.status),
                        });
                    };
                    attempt += 1;
                    progress(Progress { sent: sent_before, total: len });
                    pause(*wait, cancel)?;
                }
            }
        };
        sent_before += size;
        done.push(DonePart { part_number: number, etag });
    }

    // The picture is a nicety: without it the clip page shows the player's first frame.
    if let (Some(jpeg), Some(address)) = (thumb, &ticket.thumb_url) {
        let url = client.base().join(address)?;
        let sent = client.session().send("PUT", &url, &[("Content-Type", "image/jpeg")], Body::Bytes(jpeg), &mut |_| !cancel.load(Ordering::Relaxed));
        if matches!(&sent, Err(error) if error.is::<Cancelled>()) {
            return Err(Cancelled.into());
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    progress(Progress { sent: len, total: len });
    client.finish_upload(token, &ticket.clip_id, &done)
}
