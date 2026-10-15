// Calls into Klipp's Rust side.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ClipEvent {
  kind: string;
  atSecs: number;
  detail: string;
}

/** Trim points and track levels the editor remembers per clip. */
export interface Edit {
  in?: number;
  out?: number;
  /** Per separate track (Game, Chat, Mic…). */
  volumes?: number[];
  muted?: boolean[];
  master?: number;
}

export interface Clip {
  path: string;
  name: string;
  game: string;
  kind: "manual" | "auto" | "";
  label: string | null;
  created: number;
  durationSecs: number | null;
  sizeBytes: number;
  favorite: boolean;
  tracks: string[];
  events: ClipEvent[];
  map: string;
  mode: string;
  thumb: string | null;
  edit: Edit | null;
}

export interface Paths {
  clipsDir: string;
  settingsPath: string;
  logDir: string;
}

export interface OpenRequest {
  clip: string | null;
  page: string | null;
}

export const paths = () => invoke<Paths>("paths");
/** What Klipp was opened for (a clip, or settings). */
export const startupRequest = () => invoke<OpenRequest>("startup_request");
export const listClips = () => invoke<Clip[]>("list_clips");
export const setFavorite = (paths: string[], favorite: boolean) => invoke<void>("set_favorite", { paths, favorite });
export const saveEdit = (path: string, edit: Edit | null) => invoke<void>("save_edit", { path, edit });
export const deleteClips = (paths: string[]) => invoke<number>("delete_clips", { paths });
export const undoDelete = (id: number) => invoke<void>("undo_delete", { id });
export const finalizeDelete = (id: number) => invoke<void>("finalize_delete", { id });
export const reveal = (path: string) => invoke<void>("reveal", { path });
export const openFile = (path: string) => invoke<void>("open_file", { path });

/** URL the page can load a local file from. */
export const fileUrl = (path: string) => convertFileSrc(path);

/** Files of every audio track (Mix first). */
export const audioTracks = (path: string) => invoke<string[]>("audio_tracks", { path });
export const readCached = (path: string) => invoke<ArrayBuffer>("read_cached", { path });
export const copyToClipboard = (path: string) => invoke<void>("copy_to_clipboard", { path });

/** The recorder's settings (defaults filled in), shaped like settings.toml. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Settings = Record<string, any>;
export interface SettingChange {
  key: string[];
  value: unknown;
}
export interface AudioDevice {
  name: string;
  kind: "playback" | "recording";
  isDefault: boolean;
}

export const readSettings = () => invoke<Settings>("read_settings");
export const writeSettings = (changes: SettingChange[]) => invoke<void>("write_settings", { changes });
export const audioDevices = () => invoke<AudioDevice[]>("audio_devices");

export const getPrefs = () => invoke<Record<string, unknown>>("get_prefs");
export const setPref = (key: string, value: unknown) => invoke<void>("set_pref", { key, value });

export interface RecorderStatus {
  running: boolean;
  autostart: boolean;
}
export const recorderStatus = () => invoke<RecorderStatus>("recorder_status");

/** What `klipp-recorder --probe` found, plus free space where clips go. */
export interface SystemCheck {
  gpus: { name: string; vendor: "nvidia" | "amd" | "intel" | "other"; vramMb: number }[];
  monitor: { width: number; height: number; refreshHz: number } | null;
  recordingGpu: string | null;
  encoders: { codec: "av1" | "h264"; width: number; height: number; ok: boolean; error: string | null }[];
  ramMb: number;
  cs2: boolean;
  error: string | null;
  clipsDir: string;
  freeGb: number | null;
}

/** A fresh install that still needs setup. */
export const setupNeeded = () => invoke<boolean>("setup_needed");
export const systemCheck = () => invoke<SystemCheck>("system_check");
export const startRecorder = () => invoke<void>("start_recorder");
export const setAutostart = (enabled: boolean) => invoke<void>("set_autostart", { enabled });
/** Closes the window (the recorder keeps going). */
export const closeWindow = (dontShowAgain: boolean) => invoke<void>("close_window", { dontShowAgain });

/** A clip the recorder is saving (or collecting presses for). */
export interface Arrival {
  id: number;
  kind: "manual" | "auto";
  game: string;
  /** What an auto-clip will be named after ("2k"), when known. */
  label: string | null;
  presses: number;
  since: number;
}

export const arriving = () => invoke<Arrival[]>("arriving");

export function onArrivals(handler: (list: Arrival[]) => void): Promise<UnlistenFn> {
  return listen<Arrival[]>("arrivals", (e) => handler(e.payload));
}

export function onOpenRequest(handler: (r: OpenRequest) => void): Promise<UnlistenFn> {
  return listen<OpenRequest>("open-request", (e) => handler(e.payload));
}

export function onCloseRequested(handler: () => void): Promise<UnlistenFn> {
  return listen("close-requested", () => handler());
}

export type ExportMode = "trim" | "quality" | "share";

export interface ExportRequest {
  path: string;
  start: number;
  end: number;
  mode: ExportMode;
  targetMb: number;
  /** Per separate track, 1 = unchanged, 0 = muted. */
  volumes: number[];
  mixVolume: number;
}

export const startExport = (request: ExportRequest) => invoke<number>("start_export", { request });
export const cancelExport = (id: number) => invoke<void>("cancel_export", { id });

export interface ExportEvents {
  progress: (id: number, fraction: number) => void;
  done: (id: number, path: string, sizeBytes: number) => void;
  failed: (id: number, error: string) => void;
}

/** Subscribes to export events; returns a function that unsubscribes. */
export async function onExport(handlers: ExportEvents): Promise<() => void> {
  const offs: UnlistenFn[] = await Promise.all([
    listen<{ id: number; fraction: number }>("export-progress", (e) => handlers.progress(e.payload.id, e.payload.fraction)),
    listen<{ id: number; path: string; sizeBytes: number }>("export-done", (e) =>
      handlers.done(e.payload.id, e.payload.path, e.payload.sizeBytes),
    ),
    listen<{ id: number; error: string }>("export-failed", (e) => handlers.failed(e.payload.id, e.payload.error)),
  ]);
  return () => offs.forEach((off) => off());
}

// Thumbnails the recorder didn't make (older clips, trimmed copies) are made
// on demand, a few at a time, so a fast scroll doesn't queue hundreds of jobs
// for cards that are already gone.
const thumbs = new Map<string, Promise<string>>();
const waiting: Array<() => void> = [];
let inFlight = 0;
const MAX_IN_FLIGHT = 2;

export function thumbnail(clip: Clip, stillWanted: () => boolean): Promise<string | null> {
  if (clip.thumb) return Promise.resolve(fileUrl(clip.thumb));
  const cached = thumbs.get(clip.path);
  if (cached) return cached;
  return new Promise((resolve) => {
    const run = () => {
      if (!stillWanted()) {
        resolve(null);
        next();
        return;
      }
      inFlight++;
      const p = invoke<string>("thumbnail", { path: clip.path }).then(fileUrl);
      thumbs.set(clip.path, p);
      p.then(resolve, () => {
        thumbs.delete(clip.path);
        resolve(null);
      }).finally(() => {
        inFlight--;
        next();
      });
    };
    if (inFlight < MAX_IN_FLIGHT) run();
    else waiting.push(run);
  });
}

function next() {
  while (inFlight < MAX_IN_FLIGHT && waiting.length > 0) waiting.shift()!();
}
