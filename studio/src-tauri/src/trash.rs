//! Deleting with undo.
//!
//! Deleted clips first move into a hidden `.trash` folder (instant, same
//! drive), so "Undo" just moves them back. When the undo window passes, they
//! move back to where they were and go to the Recycle Bin from there, so the
//! Recycle Bin's "Restore" puts them in the right folder. A manifest in each
//! batch folder lets a crash be cleaned up on the next start.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW};
use windows::core::{HSTRING, PCWSTR};

use crate::meta;

pub const TRASH_DIR: &str = ".trash";
const MANIFEST: &str = "manifest.json";

/// (original path, where it is now)
type Moves = Vec<(PathBuf, PathBuf)>;

pub struct Trash {
    /// Where leftovers are looked for at startup (the clips folder's .trash).
    root: PathBuf,
    /// Batch id → (its folder, the moves to undo).
    batches: Mutex<HashMap<u64, (PathBuf, Moves)>>,
    next: AtomicU64,
}

impl Trash {
    pub fn new(clips_dir: &Path) -> Self {
        let first = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_millis() as u64);
        Self { root: clips_dir.join(TRASH_DIR), batches: Mutex::new(HashMap::new()), next: AtomicU64::new(first) }
    }

    /// Moves the clips (with their info and thumbnails) out of the library.
    /// Returns the batch id for `undo` / `finalize`.
    pub fn delete(&self, clips_dir: &Path, clips: &[PathBuf]) -> Result<u64> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        // Stage next to the clips themselves (`<clips>\<game>\clip.mp4` → `<clips>\.trash`),
        // so moving is a rename on the same drive even if the clips folder changed.
        let root = clips
            .first()
            .and_then(|c| c.parent()?.parent())
            .map(|dir| dir.join(TRASH_DIR))
            .unwrap_or_else(|| self.root.clone());
        let dir = root.join(id.to_string());
        fs::create_dir_all(&dir)?;
        unsafe {
            let _ = SetFileAttributesW(&HSTRING::from(root.as_os_str()), FILE_ATTRIBUTE_HIDDEN);
        }
        let mut moves = Moves::new();
        for (i, clip) in clips.iter().enumerate() {
            for file in [clip.clone(), meta::info_path(clips_dir, clip), meta::thumb_path(clips_dir, clip)] {
                if !file.exists() {
                    continue;
                }
                let dest = dir.join(format!("{i}_{}", file.file_name().context("no file name")?.to_string_lossy()));
                fs::rename(&file, &dest).with_context(|| format!("moving {}", file.display()))?;
                moves.push((file, dest));
            }
        }
        fs::write(dir.join(MANIFEST), serde_json::to_string(&moves)?)?;
        self.batches.lock().unwrap().insert(id, (dir, moves));
        Ok(id)
    }

    pub fn undo(&self, id: u64) -> Result<()> {
        let (dir, moves) = self.batches.lock().unwrap().remove(&id).context("that delete can no longer be undone")?;
        restore(&moves)?;
        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    /// Sends a batch to the Recycle Bin.
    pub fn finalize(&self, id: u64) -> Result<()> {
        let Some((dir, moves)) = self.batches.lock().unwrap().remove(&id) else { return Ok(()) };
        finalize_batch(&dir, &moves)
    }

    /// Finalizes every batch still waiting (e.g. when the window closes).
    pub fn finalize_all(&self) {
        let ids: Vec<u64> = self.batches.lock().unwrap().keys().copied().collect();
        for id in ids {
            let _ = self.finalize(id);
        }
    }

    /// Finishes batches left behind by a crash.
    pub fn recover(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else { return };
        for entry in entries.flatten() {
            let dir = entry.path();
            let moves: Option<Moves> = fs::read_to_string(dir.join(MANIFEST)).ok().and_then(|t| serde_json::from_str(&t).ok());
            if let Some(moves) = moves {
                let _ = finalize_batch(&dir, &moves);
            }
        }
    }
}

fn restore(moves: &Moves) -> Result<()> {
    for (original, now) in moves {
        if now.exists() {
            fs::rename(now, original).with_context(|| format!("restoring {}", original.display()))?;
        }
    }
    Ok(())
}

fn finalize_batch(dir: &Path, moves: &Moves) -> Result<()> {
    restore(moves)?;
    let originals: Vec<&PathBuf> = moves.iter().map(|(o, _)| o).filter(|p| p.exists()).collect();
    recycle(&originals)?;
    let _ = fs::remove_dir_all(dir);
    Ok(())
}

fn recycle(paths: &[&PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    // A list of paths, each null-terminated, ending with an extra null.
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(p.as_os_str().encode_wide_owned());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let result = unsafe { SHFileOperationW(&mut op) };
    anyhow::ensure!(result == 0, "moving to the Recycle Bin failed (code {result})");
    Ok(())
}

trait EncodeWideOwned {
    fn encode_wide_owned(&self) -> Vec<u16>;
}

impl EncodeWideOwned for std::ffi::OsStr {
    fn encode_wide_owned(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect()
    }
}
