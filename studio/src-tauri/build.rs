use std::{env, fs, path::PathBuf};

fn main() {
    tauri_build::build();
    copy_ffmpeg();
}

/// The studio runs ffmpeg.exe (same LGPL build as the recorder's DLLs) for
/// thumbnails and exports, so it and its DLLs go next to the executable.
fn copy_ffmpeg() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let bin = manifest.join("../../vendor/ffmpeg/bin");
    println!("cargo:rerun-if-changed={}", bin.display());
    // OUT_DIR is target/<profile>/build/<package>-<hash>/out.
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let profile_dir = out.ancestors().nth(3).expect("unexpected OUT_DIR layout");
    let Ok(entries) = fs::read_dir(&bin) else {
        println!("cargo:warning=FFmpeg not found in {}; see docs/BUILDING.md", bin.display());
        return;
    };
    for entry in entries.flatten() {
        let src = entry.path();
        let name = entry.file_name();
        let wanted = src.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")) || name == "ffmpeg.exe" || name == "ffprobe.exe";
        if !wanted {
            continue;
        }
        let dest = profile_dir.join(&name);
        let same_size = fs::metadata(&dest).ok().map(|m| m.len()) == fs::metadata(&src).ok().map(|m| m.len());
        if !same_size {
            fs::copy(&src, &dest).expect("copying FFmpeg");
        }
    }
}
