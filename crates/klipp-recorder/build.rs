//! Gives the .exe the Klipp icon and copies the FFmpeg DLLs (and ffmpeg.exe,
//! which makes thumbnails) next to it so it runs without PATH changes.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=klipp.rc");
    println!("cargo:rerun-if-changed=klipp.ico");
    embed_resource::compile("klipp.rc", embed_resource::NONE).manifest_optional().expect("compiling the icon resource");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let bin = manifest.join("../../vendor/ffmpeg/bin");
    println!("cargo:rerun-if-changed={}", bin.display());

    // OUT_DIR is target/<profile>/build/<package>-<hash>/out.
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let profile_dir = out.ancestors().nth(3).expect("unexpected OUT_DIR layout");

    let Ok(entries) = fs::read_dir(&bin) else {
        println!("cargo:warning=FFmpeg DLLs not found in {}; see docs/BUILDING.md", bin.display());
        return;
    };
    for entry in entries.flatten() {
        let src = entry.path();
        let wanted = src.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")) || entry.file_name() == "ffmpeg.exe";
        if !wanted {
            continue;
        }
        let dest = profile_dir.join(entry.file_name());
        let same_size = fs::metadata(&dest).ok().map(|m| m.len()) == fs::metadata(&src).ok().map(|m| m.len());
        if !same_size {
            fs::copy(&src, &dest).expect("copying FFmpeg DLL");
        }
    }
}
