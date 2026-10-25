// Calls into Klipp's Rust side.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Update } from "@tauri-apps/plugin-updater";

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
  /** From the recorder's watchdog: video restarting, a still picture it can't fix, or can't keep up. */
  problem: "stopped" | "blind" | "slow" | null;
  /** Another app holds the save key. */
  hotkeyTaken: boolean;
  /** Recording at a lower frame rate than set, because the graphics card couldn't keep up. */
  /** `learned`: earlier sessions made it the rate to start at (see the recorder's tuning). */
  reduced: { to: number; from: number; learned?: boolean } | null;
  /** The check after an update lowered the video settings ("1440p at 120 fps" → "1080p at 60 fps"). */
  adjusted: { from: string; to: string; version: string } | null;
}
export const recorderStatus = () => invoke<RecorderStatus>("recorder_status");

/** One track's level in the level check. `off`: why it records nothing. */
export interface TrackLevel {
  track: string;
  db: number;
  off: "mixer" | "no discord" | "no device" | null;
}
/** Runs the level check (12 s); reports arrive through `onAudioLevels`. */
export const audioLevels = () => invoke<void>("audio_levels");
export function onAudioLevels(handler: (tracks: TrackLevel[]) => void): Promise<UnlistenFn> {
  return listen<{ tracks: TrackLevel[] }>("audio-levels", (e) => handler(e.payload.tracks));
}

/** Setup's key test: a running recorder lends its key first, then up to 10 s for a press. */
export type HotkeyTest = "works" | "taken" | "nothing" | "invalid";
export const borrowHotkey = () => invoke<void>("borrow_hotkey");
export const testHotkey = (key: string) => invoke<HotkeyTest>("test_hotkey", { key });

/** A screen Klipp can record; `id` is what `video.monitor` stores. */
export interface MonitorInfo {
  id: string;
  name: string;
  width: number;
  height: number;
  refreshHz: number;
  primary: boolean;
}
export const monitors = () => invoke<MonitorInfo[]>("monitors");

/** One of the PC's own drives, and the folder Klipp would make there. */
export interface Drive {
  letter: string;
  label: string;
  freeGb: number;
  totalGb: number;
  /** The Windows drive (clips go in Videos\Klipp there). */
  system: boolean;
  folder: string;
}
export const drives = () => invoke<Drive[]>("drives");
export const createFolder = (path: string) => invoke<void>("create_folder", { path });

/** What `klipp-recorder --probe` found, plus free space where clips go. */
export interface SystemCheck {
  gpus: { name: string; vendor: "nvidia" | "amd" | "intel" | "other"; vramMb: number }[];
  monitors: MonitorInfo[];
  monitor: { width: number; height: number; refreshHz: number } | null;
  recordingGpu: string | null;
  recordingVendor: "nvidia" | "amd" | "intel" | null;
  /** "NVENC", "AMD AMF" or "Intel Quick Sync". */
  encoder: string | null;
  encoders: { codec: "av1" | "h264"; width: number; height: number; ok: boolean; error: string | null }[];
  ramMb: number;
  /** Has a battery: setup suggests Alt+F8, which needs no Fn on laptop keyboards. */
  laptop: boolean;
  cs2: boolean;
  /** The recording path measured at each candidate, best first (older recorders leave it out). */
  performance?: { height: number; fps: number; codec: string; busy: number; smooth: boolean }[];
  /** The best candidate that runs smoothly with room left for a game. */
  recommended?: { height: number; fps: number } | null;
  /** League of Legends is installed; Rocket League has run on this PC. */
  league?: boolean;
  rocketLeague?: boolean;
  error: string | null;
  clipsDir: string;
  freeGb: number | null;
}

/** "Copy details": the PC, setup's last check, key settings and the end of the log. */
export const supportDetails = () => invoke<string>("support_details");

/** Normal releases ("production"), or test builds too ("test"); updates.json. */
export interface UpdateChannel {
  channel: "production" | "test";
  /** Test channel: the recorder sends its log to the developer. */
  sendLogs: boolean;
  /** Look for updates by itself (klipp.dev, with the version and a random install number). */
  checks: boolean;
  /** This copy is a test build ("0.3.5-test.1"). */
  testBuild: boolean;
}
export const updateChannel = () => invoke<UpdateChannel>("update_channel");
export const setUpdateChannel = (u: Pick<UpdateChannel, "channel" | "sendLogs" | "checks">) => invoke<void>("set_update_channel", u);

/** The update the chosen channel offers, ready to download and install. `whenOff`: check even with checks off (asked for). */
export async function checkUpdate(whenOff = false): Promise<Update | null> {
  const { channel, checks } = await updateChannel();
  if (!checks && !whenOff) return null;
  const metadata = await invoke<ConstructorParameters<typeof Update>[0] | null>("check_update", { channel });
  return metadata ? new Update(metadata) : null;
}

/** Plays the save sound (or the error sound) once, as a real save would. */
export const playSound = (device: string, volume: number, error = false) => invoke<void>("play_sound", { device, volume, error });

/** Flashes an on-screen indicator now (Settings → General → On screen). */
export const previewIndicator = (kind: "on" | "saved" | "failed") => invoke<void>("preview_indicator", { kind });

/** CS2 is running (updating now would clear the replay buffer mid-game). */
export const gameRunning = () => invoke<boolean>("game_running");

/** Free space where clips are saved, in GB. */
export const freeSpace = () => invoke<number | null>("free_space");

export interface MovePlan {
  clips: number;
  bytes: number;
  sameDrive: boolean;
  freeBytes: number | null;
  /** There's room for them there. */
  room: boolean;
  /** Why the folder can't be used, if it can't. */
  problem: string | null;
}
export interface MoveReport {
  moved: number;
  /** Clips that stayed in the old folder (open elsewhere, or already there). */
  left: string[];
}
export const planMove = (to: string) => invoke<MovePlan>("plan_move", { to });
export const moveClips = (from: string, to: string) => invoke<MoveReport>("move_clips", { from, to });
export function onMoveProgress(handler: (done: number, total: number) => void): Promise<UnlistenFn> {
  return listen<{ done: number; total: number }>("move-progress", (e) => handler(e.payload.done, e.payload.total));
}

/** Puts text on the clipboard; false if Windows refused. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
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
