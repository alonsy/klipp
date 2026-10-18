// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Klipp: the window you open (clip library, editor, settings). The recorder
//! runs as its own process in the tray; opening this starts it if needed,
//! and closing this leaves it recording.

mod arrivals;
mod clipboard;
mod config;
mod export;
mod ffmpeg;
mod library;
mod lifecycle;
mod meta;
mod prefs;
mod relocate;
mod settings;
mod setup;
mod support;
mod trash;

use std::os::windows::process::CommandExt;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};

use crate::config::Config;
use crate::export::{ExportRequest, Exports};
use crate::library::Clip;
use crate::trash::Trash;

struct App {
    config: Config,
    trash: Trash,
    exports: Exports,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Paths {
    clips_dir: String,
    settings_path: String,
    log_dir: String,
}

/// What Klipp was opened for: `klipp.exe <clip.mp4>` opens the clip in the
/// editor, `klipp.exe --settings` opens settings, `klipp.exe --update` (the
/// tray's "Update to Klipp x.y.z") checks for the update right away.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct OpenRequest {
    clip: Option<String>,
    page: Option<String>,
}

impl OpenRequest {
    fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut r = Self::default();
        for a in args {
            if a == "--settings" {
                r.page = Some("settings".into());
            } else if a == "--update" {
                r.page = Some("update".into());
            } else if a.to_lowercase().ends_with(".mp4") && PathBuf::from(&a).is_file() {
                r.clip = Some(a);
            }
        }
        r
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecorderStatus {
    running: bool,
    autostart: bool,
    /// What the recorder's watchdog reports: "stopped" (video restarting),
    /// "blind" (the screen shows a still picture) or "slow" (can't keep up
    /// with the video settings).
    problem: Option<String>,
    /// Another app holds the save key, so pressing it does nothing.
    hotkey_taken: bool,
}

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Runs blocking work off the main thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
fn paths() -> Paths {
    // Read fresh: the clips folder can be changed in Settings.
    let config = config::load();
    Paths {
        clips_dir: config.clips_dir.to_string_lossy().into_owned(),
        settings_path: config.settings_path.to_string_lossy().into_owned(),
        log_dir: config.log_dir.to_string_lossy().into_owned(),
    }
}

#[tauri::command]
fn startup_request() -> OpenRequest {
    OpenRequest::from_args(std::env::args().skip(1))
}

#[tauri::command]
async fn list_clips(handle: AppHandle) -> CmdResult<Vec<Clip>> {
    // Read fresh: the clips folder can be changed in Settings.
    let dir = config::load().clips_dir;
    let _ = handle.asset_protocol_scope().allow_directory(&dir, true);
    blocking(move || Ok(library::list(&dir))).await
}

#[tauri::command]
async fn thumbnail(path: String) -> CmdResult<String> {
    let dir = config::load().clips_dir;
    blocking(move || ffmpeg::thumbnail(&dir, &PathBuf::from(path))).await.map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn set_favorite(paths: Vec<String>, favorite: bool) -> CmdResult<()> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    library::set_favorite(&config::load().clips_dir, &paths, favorite).map_err(err)
}

/// Remembers trim points and track levels (`null` forgets them).
#[tauri::command]
fn save_edit(path: String, edit: Value) -> CmdResult<()> {
    library::save_edit(&config::load().clips_dir, &PathBuf::from(path), edit).map_err(err)
}

#[tauri::command]
fn delete_clips(app: State<App>, paths: Vec<String>) -> CmdResult<u64> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    app.trash.delete(&config::load().clips_dir, &paths).map_err(err)
}

#[tauri::command]
fn undo_delete(app: State<App>, id: u64) -> CmdResult<()> {
    app.trash.undo(id).map_err(err)
}

#[tauri::command]
fn finalize_delete(app: State<App>, id: u64) -> CmdResult<()> {
    app.trash.finalize(id).map_err(err)
}

/// Every audio track of the clip (Mix first) as files the editor can decode.
#[tauri::command]
async fn audio_tracks(app: State<'_, App>, path: String) -> CmdResult<Vec<String>> {
    let cache = app.config.cache_dir.clone();
    blocking(move || ffmpeg::extract_tracks(&PathBuf::from(path), &cache))
        .await
        .map(|v| v.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
}

/// Raw bytes of a file in the studio's cache (extracted audio tracks), sent as
/// binary rather than JSON.
#[tauri::command]
fn read_cached(app: State<App>, path: String) -> CmdResult<tauri::ipc::Response> {
    let path = PathBuf::from(path);
    if !path.starts_with(&app.config.cache_dir) {
        return Err("only cached files can be read".into());
    }
    std::fs::read(&path).map(tauri::ipc::Response::new).map_err(|e| e.to_string())
}

/// Starts an export; progress and the result arrive as events.
#[tauri::command]
fn start_export(handle: AppHandle, app: State<App>, request: ExportRequest) -> u64 {
    app.exports.start(handle, config::load().clips_dir, request)
}

#[tauri::command]
fn cancel_export(app: State<App>, id: u64) {
    app.exports.cancel(id);
}

#[tauri::command]
fn copy_to_clipboard(path: String) -> CmdResult<()> {
    clipboard::copy_file(&PathBuf::from(path)).map_err(err)
}

#[tauri::command]
fn read_settings(app: State<App>) -> CmdResult<Value> {
    settings::read(&app.config.settings_path).map_err(err)
}

#[tauri::command]
fn write_settings(app: State<App>, changes: Vec<settings::Change>) -> CmdResult<()> {
    settings::write(&app.config.settings_path, &changes).map_err(err)
}

#[tauri::command]
async fn audio_devices() -> CmdResult<Vec<settings::Device>> {
    blocking(settings::audio_devices).await
}

#[tauri::command]
fn get_prefs(app: State<App>) -> serde_json::Map<String, Value> {
    prefs::read(&app.config.prefs_path)
}

#[tauri::command]
fn set_pref(app: State<App>, key: String, value: Value) -> CmdResult<()> {
    prefs::set(&app.config.prefs_path, &key, value).map_err(err)
}

/// A fresh install that still needs the setup screens.
#[tauri::command]
fn setup_needed(app: State<App>) -> bool {
    setup::needed(&app.config.settings_path)
}

/// What this PC can record, for the setup screens.
#[tauri::command]
async fn system_check() -> CmdResult<Value> {
    let config = config::load();
    blocking(move || setup::system_check(&config.clips_dir, &config.log_dir)).await
}

/// The PC's drives, for setup's "Save clips on".
#[tauri::command]
fn drives() -> Vec<Value> {
    setup::drives()
}

/// Makes the clips folder setup chose, so it's there before the first clip.
#[tauri::command]
fn create_folder(path: String) -> CmdResult<()> {
    std::fs::create_dir_all(&path).map_err(|e| format!("couldn't create {path}: {e}"))
}

/// The monitors Klipp can record, main display first (Settings → Video).
#[tauri::command]
async fn monitors() -> CmdResult<Value> {
    blocking(setup::monitors).await
}

/// "Copy details": the PC, setup's last check, key settings and the log's end.
#[tauri::command]
async fn support_details(handle: AppHandle) -> CmdResult<String> {
    let version = handle.package_info().version.to_string();
    blocking(move || Ok(support::details(&version, &config::load()))).await
}

/// The ▶ buttons: the save (or error) sound once, as a real save would play it.
#[tauri::command]
fn play_sound(device: String, volume: f64, error: bool) -> CmdResult<()> {
    lifecycle::play_sound(&device, volume, error).map_err(err)
}

/// Flashes an on-screen indicator (Settings → General → On screen).
#[tauri::command]
fn preview_indicator(kind: String) -> CmdResult<()> {
    lifecycle::preview_indicator(&kind).map_err(err)
}

/// A game Klipp records is running (updating then would cost the replay buffer).
#[tauri::command]
fn game_running() -> bool {
    lifecycle::process_running("cs2.exe")
}

/// Free space where clips are saved, in GB.
#[tauri::command]
fn free_space() -> Option<f64> {
    setup::free_gb(&config::load().clips_dir)
}

/// What moving the clips to `to` would involve.
#[tauri::command]
async fn plan_move(to: String) -> CmdResult<relocate::Plan> {
    let from = config::load().clips_dir;
    blocking(move || Ok(relocate::plan(&from, &PathBuf::from(to)))).await
}

/// Moves the clips from `from` (the old clips folder) to `to`; progress
/// arrives as "move-progress" events.
#[tauri::command]
async fn move_clips(handle: AppHandle, from: String, to: String) -> CmdResult<relocate::Report> {
    let to = PathBuf::from(to);
    let _ = handle.asset_protocol_scope().allow_directory(&to, true);
    blocking(move || {
        relocate::run(&PathBuf::from(from), &to, |done, total| {
            let _ = handle.emit("move-progress", serde_json::json!({ "done": done, "total": total }));
        })
    })
    .await
}

/// Clips on their way (see `arrivals`).
#[tauri::command]
fn arriving() -> Value {
    arrivals::read()
}

#[tauri::command]
fn recorder_status() -> RecorderStatus {
    let running = lifecycle::recorder_running();
    let log_dir = config::load().log_dir;
    // The recorder's reports, which only count while it runs.
    let report = |name: &str| {
        running
            .then(|| std::fs::read(log_dir.join(name)).ok())
            .flatten()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .unwrap_or_default()
    };
    let problem = report("health.json")["problem"].as_str().map(String::from);
    let hotkey_taken = report("hotkey.json")["works"] == false;
    RecorderStatus { running, autostart: lifecycle::autostart_enabled(), problem, hotkey_taken }
}

/// Setup's Test button, step 1: a running recorder lends its save key.
#[tauri::command]
async fn borrow_hotkey() -> CmdResult<()> {
    blocking(|| {
        lifecycle::borrow_hotkey();
        Ok(())
    })
    .await
}

/// Settings → Audio's level check: every track's level for 12 s, as
/// "audio-levels" events, the way the recorder hears them.
#[tauri::command]
async fn audio_levels(handle: AppHandle) -> CmdResult<()> {
    blocking(move || {
        lifecycle::audio_levels(12, |report| {
            let _ = handle.emit("audio-levels", report);
        })
    })
    .await
}

/// Step 2: waits up to 10 s for a press of `key` (see `lifecycle::test_hotkey`).
#[tauri::command]
async fn test_hotkey(key: String) -> CmdResult<String> {
    blocking(move || lifecycle::test_hotkey(&key)).await
}

#[tauri::command]
fn start_recorder() -> CmdResult<()> {
    lifecycle::ensure_recorder().map_err(err)
}

#[tauri::command]
fn set_autostart(enabled: bool) -> CmdResult<()> {
    lifecycle::set_autostart(enabled).map_err(err)
}

/// Closes the window after the "still recording" notice (the recorder keeps going).
#[tauri::command]
fn close_window(window: tauri::Window, app: State<App>, dont_show_again: bool) -> CmdResult<()> {
    if dont_show_again {
        prefs::set(&app.config.prefs_path, "closeNotice", Value::Bool(false)).map_err(err)?;
    }
    window.destroy().map_err(|e| e.to_string())
}

/// Opens Explorer with the file selected (or a folder opened).
#[tauri::command]
fn reveal(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    let mut cmd = std::process::Command::new("explorer.exe");
    if p.is_dir() {
        cmd.arg(&p);
    } else {
        cmd.raw_arg(format!("/select,\"{}\"", p.display()));
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Opens a file with its default app (e.g. the settings file in a text editor).
#[tauri::command]
fn open_file(path: String) -> CmdResult<()> {
    std::process::Command::new("explorer.exe").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Windows draws a 1 px border around the window (it has no frame of its own),
/// in the accent color if you chose that in Windows settings. Use the app's
/// own edge color instead, so the window stays one neutral surface.
fn neutral_border(window: &tauri::WebviewWindow) {
    use windows::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DwmSetWindowAttribute};
    let Ok(raw) = window.hwnd() else { return };
    let hwnd = windows::Win32::Foundation::HWND(raw.0 as _);
    // COLORREF is 0x00BBGGRR. The border is the raised surface (#3b3b39); the
    // 1 px of Windows' own title area left at the top is the page (#1e1e1d).
    for (attribute, color) in [(DWMWA_BORDER_COLOR, 0x0039_3b3b_u32), (DWMWA_CAPTION_COLOR, 0x001d_1e1e_u32)] {
        let _ = unsafe { DwmSetWindowAttribute(hwnd, attribute, &color as *const u32 as *const _, 4) };
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn main() {
    let config = config::load();
    // On a fresh install, recording starts when setup finishes, with its choices.
    if !setup::needed(&config.settings_path)
        && let Err(e) = lifecycle::ensure_recorder()
    {
        eprintln!("couldn't start the recorder: {e:#}");
    }
    let trash = Trash::new(&config.clips_dir);
    trash.recover();
    ffmpeg::trim_cache(&config.cache_dir);
    let (clips_dir, cache_dir) = (config.clips_dir.clone(), config.cache_dir.clone());

    let app = tauri::Builder::default()
        // Opening Klipp again (Start menu, tray) focuses this window instead
        // and passes on what it was opened for.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            show_main(app);
            let _ = app.emit("open-request", OpenRequest::from_args(args.into_iter().skip(1)));
        }))
        // New versions from GitHub releases, signed with Klipp's key (see tauri.conf.json).
        .plugin(tauri_plugin_updater::Builder::new().build())
        // The Windows folder picker (Settings → Storage).
        .plugin(tauri_plugin_dialog::init())
        .manage(App { config, trash, exports: Exports::default() })
        .setup(move |app| {
            // Let the page load videos and thumbnails from these folders only.
            let scope = app.asset_protocol_scope();
            scope.allow_directory(&clips_dir, true)?;
            scope.allow_directory(&cache_dir, true)?;
            let handle = app.handle().clone();
            lifecycle::on_quit_request(move || handle.exit(0))?;
            arrivals::watch(app.handle().clone());
            if let Some(w) = app.get_webview_window("main") {
                neutral_border(&w);
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The first time, explain that closing the window doesn't stop recording.
            WindowEvent::CloseRequested { api, .. } => {
                let app = window.state::<App>();
                let notice = prefs::read(&app.config.prefs_path).get("closeNotice").and_then(Value::as_bool).unwrap_or(true);
                if notice && lifecycle::recorder_running() {
                    api.prevent_close();
                    let _ = window.emit("close-requested", ());
                }
            }
            WindowEvent::Destroyed => window.state::<App>().trash.finalize_all(),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            paths,
            startup_request,
            list_clips,
            thumbnail,
            set_favorite,
            save_edit,
            delete_clips,
            undo_delete,
            finalize_delete,
            reveal,
            open_file,
            audio_tracks,
            read_cached,
            start_export,
            cancel_export,
            copy_to_clipboard,
            read_settings,
            write_settings,
            audio_devices,
            get_prefs,
            set_pref,
            recorder_status,
            borrow_hotkey,
            test_hotkey,
            audio_levels,
            setup_needed,
            system_check,
            arriving,
            start_recorder,
            set_autostart,
            close_window,
            support_details,
            play_sound,
            game_running,
            free_space,
            plan_move,
            move_clips,
            monitors,
            drives,
            create_folder,
            preview_indicator
        ])
        .build(tauri::generate_context!())
        .expect("Klipp failed to start");
    app.run(|app, event| {
        if let RunEvent::Exit = event {
            app.state::<App>().trash.finalize_all();
        }
    });
}
