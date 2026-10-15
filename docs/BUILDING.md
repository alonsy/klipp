# Building Klipp

Klipp is two programs that install together:

- `klipp-recorder.exe` lives in the tray and keeps the replay buffer (Rust).
- `klipp.exe` is the window: library, editor and settings (Tauri with Svelte and TypeScript). Opening it starts the recorder if it isn't running.

## 1. Tools

```powershell
# Rust (for your user, no admin needed)
winget install -e --id Rustlang.Rustup

# Node.js 22
winget install -e --id OpenJS.NodeJS.LTS

# The MSVC compiler, Windows SDK and libclang. Needs admin.
winget install -e --id Microsoft.VisualStudio.2022.BuildTools --override "--passive --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --add Microsoft.VisualStudio.Component.VC.Llvm.Clang"
```

If the Build Tools are somewhere other than the default folder, update `LIBCLANG_PATH` in [.cargo/config.toml](../.cargo/config.toml).

## 2. FFmpeg

```powershell
.\scripts\fetch-ffmpeg.ps1
```

This downloads the **LGPL** shared build of FFmpeg 8.1 into `vendor/ffmpeg/` and checks its SHA-256. Klipp links against it and ships its DLLs (plus `ffmpeg.exe` for thumbnails and exports) next to the programs. Only LGPL builds are used, so Klipp stays MIT.

## 3. Install

```powershell
.\scripts\install.ps1            # build everything and install for your user
.\scripts\install.ps1 -NoBuild   # install what's already built
```

It installs to `%LOCALAPPDATA%\Programs\Klipp`, adds Klipp to the Start Menu and starts recording. Run it again to update.

## 4. Working on it

```powershell
# The window's interface
cd studio
npm install
npm run build        # into studio/dist, which is built into klipp.exe
npm run check        # type-check the Svelte and TypeScript code
cd ..

# Both programs
cargo build --release
.\target\release\klipp.exe
```

For hot reload while working on the interface, build `klipp` with `--no-default-features` and run `npm run dev` in `studio` (or use `npx tauri dev`).

Useful while working on the recorder:

```powershell
.\target\release\klipp-recorder.exe --list-audio          # device names for Settings → Audio
.\target\release\klipp-recorder.exe --settings my.toml    # use another settings file
```

## Where things are

| | |
|---|---|
| Settings | `%APPDATA%\Klipp\settings.toml` (commented, applied while running) |
| Logs | `%LOCALAPPDATA%\Klipp\recorder.log` |
| Clips | `Videos\Klipp\<game>\`, with Klipp's notes in a hidden `.klipp` folder |

## Layout

| Folder | What's in it |
|---|---|
| `crates/klipp-media` | FFmpeg wrappers (encoding and MP4 writing); the only place with FFmpeg's unsafe calls |
| `crates/klipp-recorder` | Screen capture, audio, the replay buffer, game integrations, the tray |
| `studio/src-tauri` | The window's Rust side: library, exports, settings file, talking to the recorder |
| `studio/src` | The window's interface (Svelte 5) |
| `assets` | The logo, icons and sounds |
