//! The hidden `.klipp` folder inside the clips folder, where everything Klipp
//! knows about a clip lives, so the clips folder itself only shows clips:
//! `.klipp\info\<game>\<clip>.json` and `.klipp\thumbs\<game>\<clip>.jpg`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value, json};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
use windows::core::HSTRING;

pub const META_DIR: &str = ".klipp";

/// `<clips>\.klipp\<kind>\<game>\<stem>.<ext>` for a clip at `<clips>\<game>\<stem>.mp4`.
fn meta_file(clips_dir: &Path, clip: &Path, kind: &str, ext: &str) -> PathBuf {
    // Clips outside the clips folder (it was changed) keep their own `.klipp`
    // next to their game folder, like the recorder wrote it.
    let (root, rel) = match clip.strip_prefix(clips_dir) {
        Ok(rel) => (clips_dir.to_path_buf(), rel.to_path_buf()),
        Err(_) => {
            let game_dir = clip.parent().unwrap_or(Path::new(""));
            let root = game_dir.parent().unwrap_or(game_dir).to_path_buf();
            let rel = clip.strip_prefix(&root).map(Path::to_path_buf).unwrap_or_else(|_| clip.file_name().unwrap_or_default().into());
            (root, rel)
        }
    };
    root.join(META_DIR).join(kind).join(rel).with_extension(ext)
}

pub fn info_path(clips_dir: &Path, clip: &Path) -> PathBuf {
    meta_file(clips_dir, clip, "info", "json")
}

pub fn thumb_path(clips_dir: &Path, clip: &Path) -> PathBuf {
    meta_file(clips_dir, clip, "thumbs", "jpg")
}

/// Creates the folder a meta file goes in, keeping `.klipp` hidden.
pub fn prepare(clips_dir: &Path, file: &Path) -> Result<()> {
    let root = clips_dir.join(META_DIR);
    let new_root = !root.exists();
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    if new_root && root.exists() {
        let _ = unsafe { SetFileAttributesW(&HSTRING::from(root.as_os_str()), FILE_ATTRIBUTE_HIDDEN) };
    }
    Ok(())
}

/// The clip's info. Tolerates a UTF-8 BOM, which some editors add.
pub fn read_info(clips_dir: &Path, clip: &Path) -> Option<Value> {
    let text = fs::read_to_string(info_path(clips_dir, clip)).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// Changes some fields of the clip's info (creating it if needed), keeping the rest.
pub fn update_info(clips_dir: &Path, clip: &Path, change: impl FnOnce(&mut Value)) -> Result<()> {
    let path = info_path(clips_dir, clip);
    let mut info = read_info(clips_dir, clip).filter(Value::is_object).unwrap_or_else(|| json!({ "version": 2 }));
    change(&mut info);
    prepare(clips_dir, &path)?;
    let part = path.with_extension("json.part");
    fs::write(&part, serde_json::to_string_pretty(&info)?)?;
    fs::rename(&part, &path)?;
    Ok(())
}

/// Moves info files the old way (`<clip>.json` next to the clip) into `.klipp`.
pub fn adopt_sidecars(clips_dir: &Path, game_dir: &Path) {
    for entry in fs::read_dir(game_dir).into_iter().flatten().flatten() {
        let json = entry.path();
        if !json.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
            continue;
        }
        let clip = json.with_extension("mp4");
        if !clip.exists() {
            continue;
        }
        let dest = info_path(clips_dir, &clip);
        if prepare(clips_dir, &dest).is_ok() && !dest.exists() {
            let _ = fs::rename(&json, &dest);
        }
    }
}
