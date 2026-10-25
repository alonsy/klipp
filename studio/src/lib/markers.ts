import type { ClipEvent } from "./api";
import { eventName, eventRank, weaponName } from "./format";

export interface Marker {
  at: number;
  /** The most notable event at this moment; its icon is shown. */
  kind: string;
  /** Everything that happened here, for tooltips ("Headshot · AK-47"). */
  all: string[];
}

/** Events at the same moment (a kill that is also the 2K) share one marker. */
export function mergeMarkers(events: ClipEvent[]): Marker[] {
  const out: Marker[] = [];
  for (const e of [...events].sort((a, b) => a.atSecs - b.atSecs)) {
    const last = out[out.length - 1];
    const name = e.detail ? `${eventName(e.kind)} · ${weaponName(e.detail)}` : eventName(e.kind);
    if (last && e.atSecs - last.at < 0.3) {
      if (!last.all.includes(name)) last.all.push(name);
      if (eventRank(e.kind) > eventRank(last.kind)) last.kind = e.kind;
    } else {
      out.push({ at: e.atSecs, kind: e.kind, all: [name] });
    }
  }
  return out;
}

export const isHot = (kind: string) => /^\d+k$|^ace$|^penta$/.test(kind);
