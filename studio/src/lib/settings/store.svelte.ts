// The recorder's settings as the settings page edits them. Every change is
// written to settings.toml straight away; the recorder applies it within a second.
import * as api from "../api";
import type { AudioDevice, Settings } from "../api";
import { refreshRecorder, reload, toast } from "../state.svelte";

export const store = $state({
  s: null as Settings | null,
  devices: [] as AudioDevice[],
  error: "",
});

export async function load() {
  try {
    store.s = await api.readSettings();
    store.error = "";
  } catch (e) {
    store.error = String(e);
  }
  store.devices = await api.audioDevices().catch(() => []);
}

/** Saves one setting, e.g. set(["auto_clip", "cs2", "headshot"], true). */
export async function set(key: string[], value: unknown) {
  await setMany([{ key, value }]);
}

/** Saves several settings in one write, so recording restarts once (a video preset). */
export async function setMany(changes: { key: string[]; value: unknown }[]) {
  if (store.s) {
    for (const { key, value } of changes) {
      let t = store.s;
      for (const k of key.slice(0, -1)) t = t[k] ??= {};
      t[key[key.length - 1]] = value;
    }
  }
  try {
    await api.writeSettings(changes);
    if (changes.some((c) => c.key[0] === "storage" && c.key[1] === "clips_dir")) setTimeout(reload, 300);
    if (changes.some((c) => c.key[0] === "replay")) setTimeout(refreshRecorder, 1500);
  } catch (e) {
    toast(`Couldn't save: ${e}`);
  }
}

export function devicesOf(kind: string): string[] {
  return store.devices.filter((d) => (kind === "capture" ? d.kind === "recording" : d.kind === "playback")).map((d) => d.name);
}

/** Device options for a source, keeping its current value even if it isn't plugged in. */
export function deviceOptions(kind: string, current: string): string[] {
  const names = devicesOf(kind);
  return !current || names.some((n) => n.toLowerCase().includes(current.toLowerCase())) ? names : [current, ...names];
}

/** The full device name a saved (partial) name stands for ("" is the Windows default). */
export function matchDevice(kind: string, current: string): string {
  if (!current) return "";
  return deviceOptions(kind, current).find((n) => n.toLowerCase().includes(current.toLowerCase())) ?? current;
}
