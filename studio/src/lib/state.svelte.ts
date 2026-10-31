// App-wide state shared by the sidebar, clips page, editor and settings.
import * as api from "./api";
import type { Arrival, Clip } from "./api";
import { emptySocials } from "./creator/socials";

export type Kind = "all" | "auto" | "manual" | "favorites";
export type Sort = "newest" | "oldest" | "longest" | "largest";
export type Page = "clips" | "settings" | "setup" | "pctest" | "creator";

export const library = $state({
  clips: [] as Clip[],
  loading: true,
  error: "",
  clipsDir: "",
  settingsPath: "",
  logDir: "",
});

export const filters = $state({
  /** A game folder, or null for every folder. */
  game: null as string | null,
  kind: "all" as Kind,
  /** Clips with this event (e.g. "2k", "headshot"). */
  event: null as string | null,
  search: "",
  sort: "newest" as Sort,
});

export const ui = $state({
  page: "clips" as Page,
  /** Which settings section is open. */
  section: "general",
  /** The clip open in the editor, if any. */
  clip: null as Clip | null,
  /** The clip being made into a short (Content Creation Center), if any, and where it was opened from. */
  short: null as Clip | null,
  shortFrom: "creator" as "creator" | "editor",
  /** What the Content Creation page shows. */
  creatorTab: "clips" as "clips" | "camera" | "socials",
  closeNotice: false,
  /** Setup was opened from Settings (it can be cancelled), not on a fresh install. */
  setupRerun: false,
  /** The clip whose thumbnail grows into the player and back (see `isHero`). */
  hero: null as string | null,
});

/** For the shell transition: this clip's picture carries over between library and editor. */
export const isHero = (path: string) => ui.hero === path;

/**
 * Thumbnails made after the list was read aren't in `clip.thumb` yet. The editor
 * needs them at once, for the picture the hero grows into.
 */
export const shownThumbs = new Map<string, string>();

/** The studio's own preferences, with defaults. */
export const prefs = $state({
  closeNotice: true,
  accent: "#ff5a2e",
  groupSessions: true,
  /** The editor's listening volume (not the clip's levels). */
  previewVolume: 1,
  /** Put exported files on the clipboard when they're done. */
  copyAfterExport: true,
  /** Leave the voice tracks (chat, microphone) out of exports. */
  exportWithoutVoices: false,
  /** The version whose settings change (the check after an update) was already told. */
  adjustToldFor: "",
  /** Settings → Audio shows every track (Multi Device Track Mode). */
  audioMultiDevice: false,
  /** Settings → Audio switched to Sonar's tracks by itself once. */
  sonarSwitched: false,
  /** The version the PC test screen was shown for after an update (it shows once). */
  pcTestShownFor: "",
  /** Content Creator Mode: your handles, and which ones shorts show. */
  socials: emptySocials(),
  /** The Content Creation Center lists only clips with your camera. */
  creatorCameraOnly: true,
});

/** Clips on their way, and clips that just landed (briefly highlighted). */
export const incoming = $state({ list: [] as Arrival[], fresh: [] as string[] });

/** A new arrivals list from the recorder. When one disappears, its clip has landed. */
export async function setArrivals(list: Arrival[]) {
  const landed = incoming.list.some((a) => !list.some((b) => b.id === a.id));
  incoming.list = list;
  if (!landed) return;
  const before = new Set(library.clips.map((c) => c.path));
  await reload();
  const fresh = library.clips.filter((c) => !before.has(c.path)).map((c) => c.path);
  if (fresh.length === 0) return;
  incoming.fresh = fresh;
  setTimeout(() => {
    if (incoming.fresh === fresh) incoming.fresh = [];
  }, 2600);
}

export const recorder = $state({
  running: true,
  autostart: false,
  hotkey: "F8",
  lengthSecs: 60,
  /** Manual clipping on: the hotkey works and Klipp records all the time. */
  manual: true,
  /** From the recorder's watchdog: "stopped" (video restarting), "blind" (a still picture) or "slow" (can't keep up). */
  problem: null as "stopped" | "blind" | "slow" | null,
  /** Another app holds the save key. */
  hotkeyTaken: false,
  /** Recording at a lower frame rate than set until the recorder restarts (the card couldn't keep up). */
  reduced: null as { to: number; from: number; learned?: boolean } | null,
  /** Restart the recorder if it stops while this window is open (not before setup is done). */
  autoRestart: false,
});

let wasRunning = false;
let lastRestart = 0;
let adjustTold = false;

export async function loadPrefs() {
  try {
    Object.assign(prefs, await api.getPrefs());
  } catch {
    // Defaults.
  }
  applyAccent();
}

export async function setPref<K extends keyof typeof prefs>(key: K, value: (typeof prefs)[K]) {
  prefs[key] = value;
  if (key === "accent") applyAccent();
  await api.setPref(key, value).catch((e) => toast(`Couldn't save: ${e}`));
}

function applyAccent() {
  document.documentElement.style.setProperty("--accent", prefs.accent);
}

export async function refreshRecorder() {
  try {
    const [status, settings] = await Promise.all([api.recorderStatus(), api.readSettings()]);
    // It was recording and stopped without "Quit Klipp" (which closes this
    // window too): it crashed or was killed, so start it again (once a minute at most).
    if (!status.running && wasRunning && recorder.autoRestart && Date.now() - lastRestart > 60_000) {
      lastRestart = Date.now();
      api.startRecorder().then(
        () => toast("Klipp's recorder stopped unexpectedly, so it was started again."),
        (e) => toast(`Klipp's recorder stopped and couldn't start again: ${e}`),
      );
    }
    wasRunning = status.running;
    recorder.running = status.running;
    recorder.autostart = status.autostart;
    recorder.problem = status.problem;
    recorder.hotkeyTaken = status.hotkeyTaken;
    recorder.reduced = status.reduced;
    tellAdjusted(status.adjusted);
    recorder.hotkey = settings.replay?.hotkey ?? recorder.hotkey;
    recorder.lengthSecs = settings.replay?.length_secs ?? recorder.lengthSecs;
    recorder.manual = settings.replay?.manual ?? true;
  } catch {
    // Keep what we had.
  }
}

/** Once per version: the recorder's check after the update lowered the video settings. */
function tellAdjusted(a: api.RecorderStatus["adjusted"]) {
  if (!a || adjustTold || prefs.adjustToldFor === a.version) return;
  adjustTold = true;
  setPref("adjustToldFor", a.version);
  toast(
    `After the update, Klipp tested your graphics card: ${a.from} wasn't smooth, so it records ${a.to} now.`,
    { label: "Video settings", run: () => openPage("settings", "video") },
    12000,
  );
}

export async function reload() {
  try {
    const [clips, paths] = await Promise.all([api.listClips(), api.paths()]);
    library.clips = clips;
    library.clipsDir = paths.clipsDir;
    library.settingsPath = paths.settingsPath;
    library.logDir = paths.logDir;
    library.error = "";
    // Keep the open clip's object current (favorite, saved edits).
    if (ui.clip) ui.clip = clips.find((c) => c.path === ui.clip!.path) ?? ui.clip;
  } catch (e) {
    library.error = String(e);
  } finally {
    library.loading = false;
  }
}

/** Where the library was scrolled to and which clip was opened from it, so
 * coming back from the editor lands in the same place. Not reactive: the
 * library reads it once when it's shown again. */
export const libraryView = { scrollTop: 0, opened: null as string | null };

export function openClip(c: Clip) {
  libraryView.opened = c.path;
  ui.hero = c.path;
  ui.clip = c;
}

/** Opens the short editor (Content Creation Center) for a clip. */
export function openShort(c: Clip, from: "creator" | "editor" = "creator") {
  ui.clip = null;
  ui.shortFrom = from;
  ui.short = c;
}

/** A clip's saved trim and levels, for the next time it's opened. */
export function setClipEdit(path: string, edit: Clip["edit"]) {
  for (const c of [ui.clip, ui.short, ...library.clips]) if (c?.path === path) c.edit = edit;
}

/** A clip's saved short, for the next time it's opened. */
export function setClipShort(path: string, short: Clip["short"]) {
  for (const c of [ui.short, ...library.clips]) if (c?.path === path) c.short = short;
}

export function openPage(page: Page, section?: string) {
  ui.clip = null;
  ui.short = null;
  ui.page = page;
  if (section) ui.section = section;
}

/** Everything but the event filter, which the toolbar's chips are built from. */
export function inScope(c: Clip): boolean {
  if (filters.game && c.game !== filters.game) return false;
  if (filters.kind === "favorites" && !c.favorite) return false;
  if (filters.kind === "auto" && c.kind !== "auto") return false;
  if (filters.kind === "manual" && c.kind === "auto") return false;
  const q = filters.search.trim().toLowerCase();
  if (q && !`${c.name} ${c.game} ${c.label ?? ""} ${c.map}`.toLowerCase().includes(q)) return false;
  return true;
}

export function matches(c: Clip): boolean {
  if (!inScope(c)) return false;
  if (filters.event && c.label !== filters.event && !c.events.some((e) => e.kind === filters.event)) return false;
  return true;
}

export function sorted(clips: Clip[]): Clip[] {
  const by: Record<Sort, (a: Clip, b: Clip) => number> = {
    newest: (a, b) => b.created - a.created,
    oldest: (a, b) => a.created - b.created,
    longest: (a, b) => (b.durationSecs ?? 0) - (a.durationSecs ?? 0),
    largest: (a, b) => b.sizeBytes - a.sizeBytes,
  };
  return [...clips].sort(by[filters.sort]);
}

// Toasts, optionally with an action (e.g. Undo).
export interface Toast {
  id: number;
  text: string;
  tone: "info" | "error";
  action?: { label: string; run: () => void };
}

export const toasts = $state<Toast[]>([]);
let nextToast = 1;

/** Shows a toast for `ms`; `onExpire` runs if it times out without its action being used. */
export function toast(text: string, action?: Toast["action"], ms = 6000, onExpire?: () => void) {
  const id = nextToast++;
  let acted = false;
  const tone = /fail|couldn't|error/i.test(text) ? "error" : "info";
  const t: Toast = {
    id,
    text,
    tone,
    action: action && {
      label: action.label,
      run: () => {
        acted = true;
        dismiss(id);
        action.run();
      },
    },
  };
  toasts.push(t);
  setTimeout(() => {
    if (toasts.some((x) => x.id === id)) dismiss(id);
    if (!acted) onExpire?.();
  }, ms);
}

export function dismiss(id: number) {
  const i = toasts.findIndex((t) => t.id === id);
  if (i >= 0) toasts.splice(i, 1);
}

/** Deletes clips with an Undo toast; they go to the Recycle Bin once it closes. */
export async function deleteClips(paths: string[]) {
  if (paths.length === 0) return;
  try {
    const id = await api.deleteClips(paths);
    const gone = new Set(paths);
    library.clips = library.clips.filter((c) => !gone.has(c.path));
    if (ui.clip && gone.has(ui.clip.path)) ui.clip = null;
    const what = paths.length === 1 ? "1 clip" : `${paths.length} clips`;
    toast(
      `Deleted ${what}`,
      {
        label: "Undo",
        run: async () => {
          await api.undoDelete(id).catch((e) => toast(`Undo failed: ${e}`));
          await reload();
        },
      },
      8000,
      () => api.finalizeDelete(id).catch((e) => toast(`Moving to the Recycle Bin failed: ${e}`)),
    );
  } catch (e) {
    toast(`Delete failed: ${e}`);
    await reload();
  }
}

export async function setFavorite(paths: string[], favorite: boolean) {
  try {
    await api.setFavorite(paths, favorite);
    const set = new Set(paths);
    for (const c of library.clips) if (set.has(c.path)) c.favorite = favorite;
    if (ui.clip && set.has(ui.clip.path)) ui.clip.favorite = favorite;
  } catch (e) {
    toast(`Couldn't update favorites: ${e}`);
  }
}
