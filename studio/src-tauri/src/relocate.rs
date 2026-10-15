//! Changing the clips folder and taking the clips along: game folders,
//! exports and Klipp's hidden notes (`.klipp`: info, thumbnails, trims and
//! levels). On the same drive each file is renamed, which is instant. To
//! another drive each file is copied, and the original is deleted only once
//! the copy is complete. A clip open in another program stays where it is,
//! with its notes, and is reported.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::export::EXPORTS_DIR;
use crate::meta::{self, META_DIR};

/// What moving would involve, for the question before it starts.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub clips: usize,
    pub bytes: u64,
    pub same_drive: bool,
    /// Free space where the clips would go (another drive only).
    pub free_bytes: Option<u64>,
    /// There's room for them there (always, on the same drive).
    pub room: bool,
    /// Why this folder can't be used, if it can't.
    pub problem: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub moved: usize,
    /// Clips that stayed in the old folder (open in another program, or a
    /// clip with the same name already in the new one).
    pub left: Vec<String>,
}

/// Room to spare on the new drive beyond the clips themselves.
const MARGIN: u64 = 512 * 1024 * 1024;

pub fn plan(from: &Path, to: &Path) -> Plan {
    let clips = clip_files(from);
    let bytes: u64 = files(from).iter().map(|f| size(f)).sum();
    let same_drive = drive(from) == drive(to);
    let free_bytes = if same_drive { None } else { crate::setup::free_gb(to).map(|gb| (gb * 1e9) as u64) };
    let problem = if same_path(from, to) {
        Some("That's already the clips folder.".into())
    } else if is_inside(to, from) {
        Some("The new folder is inside the current clips folder. Pick a folder outside it.".into())
    } else if is_inside(from, to) {
        Some("The current clips folder is inside the new one. Pick another folder.".into())
    } else {
        None
    };
    let room = same_drive || free_bytes.is_none_or(|free| free >= bytes + MARGIN);
    Plan { clips: clips.len(), bytes, same_drive, free_bytes, room, problem }
}

/// Moves everything; `progress` gets (bytes done, bytes in all) every so often.
pub fn run(from: &Path, to: &Path, progress: impl Fn(u64, u64)) -> Result<Report> {
    fs::create_dir_all(to).with_context(|| format!("can't create {}", to.display()))?;
    let total: u64 = files(from).iter().map(|f| size(f)).sum();
    let mut done = 0u64;
    let mut last_report = Instant::now();
    let mut report_progress = |done: u64| {
        if last_report.elapsed() > Duration::from_millis(100) {
            last_report = Instant::now();
            progress(done, total);
        }
    };
    let mut report = Report { moved: 0, left: Vec::new() };

    // Each clip together with its notes, so a clip that can't move keeps them.
    for clip in clip_files(from) {
        let rel = clip.strip_prefix(from).unwrap_or(&clip).to_path_buf();
        let dest = to.join(&rel);
        let clip_size = size(&clip);
        let moved = !dest.exists() && move_file(&clip, &dest)?;
        if moved {
            report.moved += 1;
            for (old, new) in [
                (meta::info_path(from, &clip), meta::info_path(to, &dest)),
                (meta::thumb_path(from, &clip), meta::thumb_path(to, &dest)),
            ] {
                if old.exists() && !new.exists() {
                    meta::prepare(to, &new)?;
                    done += size(&old);
                    move_file(&old, &new)?;
                }
            }
        } else {
            report.left.push(rel.to_string_lossy().into_owned());
        }
        done += clip_size;
        report_progress(done);
    }

    // Then the rest: exports, and notes that don't belong to a moved clip
    // (those of clips that stayed are skipped, they stay with their clip).
    let stayed: Vec<PathBuf> = report.left.iter().map(|rel| from.join(rel)).collect();
    let keep: Vec<PathBuf> = stayed.iter().flat_map(|c| [meta::info_path(from, c), meta::thumb_path(from, c)]).collect();
    for file in files(from) {
        if stayed.contains(&file) || keep.contains(&file) {
            continue;
        }
        let rel = file.strip_prefix(from).unwrap_or(&file).to_path_buf();
        let dest = to.join(&rel);
        if !dest.exists() {
            if rel.starts_with(META_DIR) {
                meta::prepare(to, &dest)?;
            }
            done += size(&file);
            move_file(&file, &dest)?;
        }
        report_progress(done);
    }
    progress(total, total);
    remove_empty_dirs(from);
    Ok(report)
}

/// Moves one file. False if it's in use (or otherwise can't be moved) and was
/// left alone; an error only when copying itself fails (e.g. the disk is full).
fn move_file(src: &Path, dest: &Path) -> Result<bool> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    match fs::rename(src, dest) {
        Ok(()) => Ok(true),
        // ERROR_NOT_SAME_DEVICE: another drive.
        Err(e) if e.raw_os_error() == Some(17) => copy_across(src, dest),
        // In use, or not allowed: leave it where it is.
        Err(_) => Ok(false),
    }
}

/// To another drive: copy to `<dest>.part`, check it, delete the original,
/// then give the copy its name. False (nothing changed) if the original is in use.
fn copy_across(src: &Path, dest: &Path) -> Result<bool> {
    let part = dest.with_extension(format!("{}.part", dest.extension().and_then(|e| e.to_str()).unwrap_or("")));
    fs::copy(src, &part).with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
    if size(&part) != size(src) {
        let _ = fs::remove_file(&part);
        anyhow::bail!("the copy of {} came out incomplete", src.display());
    }
    if fs::remove_file(src).is_err() {
        // Open in another program: keep the original, drop the copy.
        let _ = fs::remove_file(&part);
        return Ok(false);
    }
    fs::rename(&part, dest)?;
    Ok(true)
}

/// Clips: MP4s in the game folders and loose in the clips folder (not exports).
fn clip_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !name.starts_with('.') && name != EXPORTS_DIR {
                out.extend(walk(&path).into_iter().filter(|p| is_mp4(p)));
            }
        } else if is_mp4(&path) {
            out.push(path);
        }
    }
    out
}

/// Every file Klipp keeps in the clips folder. The pending-delete `.trash`
/// stays (Undo still finds it there), and so do the user's own files at the top.
fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name == META_DIR || !name.starts_with('.') {
                out.extend(walk(&path));
            }
        } else if is_mp4(&path) {
            out.push(path);
        }
    }
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn remove_empty_dirs(root: &Path) {
    fn prune(dir: &Path) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                prune(&entry.path());
            }
        }
        let _ = fs::remove_dir(dir); // only succeeds when empty
    }
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() && (name == META_DIR || !name.starts_with('.')) {
            prune(&entry.path());
        }
    }
}

fn is_mp4(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
}

fn size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn drive(path: &Path) -> String {
    match path.components().next() {
        Some(Component::Prefix(p)) => p.as_os_str().to_string_lossy().to_uppercase(),
        _ => String::new(),
    }
}

fn normalized(path: &Path) -> String {
    let s = path.to_string_lossy().replace('/', "\\").to_lowercase();
    s.trim_end_matches('\\').to_string()
}

fn same_path(a: &Path, b: &Path) -> bool {
    normalized(a) == normalized(b)
}

/// `inner` is somewhere below `outer`.
fn is_inside(inner: &Path, outer: &Path) -> bool {
    let (inner, outer) = (normalized(inner), normalized(outer));
    inner.len() > outer.len() && inner.starts_with(&outer) && inner[outer.len()..].starts_with('\\')
}
