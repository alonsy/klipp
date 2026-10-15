// Groups clips into play sessions, so a library reads like a history:
// "Mirage · Competitive — Today 18:40–19:55".
//
// Walking the clips in time order, a clip starts a new session when
//   - it's from a different game than the session,
//   - more than SESSION_GAP passed since the session's last clip, or
//   - both it and the session know their map and the maps differ
//     (the next match started, even right after the last one).
// Clips that don't know their map (saved in a menu, or by a game we don't
// read) simply join the session they fall in.
import type { Clip } from "./api";

export const SESSION_GAP_MS = 30 * 60 * 1000;

export interface Session {
  /** Stable while clips are added to it: its first clip's path. */
  key: string;
  game: string;
  map: string;
  mode: string;
  start: number;
  end: number;
  /** In the order they're shown. */
  clips: Clip[];
}

export function groupSessions(clips: Clip[], newestFirst: boolean): Session[] {
  const ordered = [...clips].sort((a, b) => a.created - b.created);
  const out: Session[] = [];
  let cur: Session | null = null;
  for (const c of ordered) {
    const mapChanged = cur && cur.map && c.map && cur.map !== c.map;
    if (!cur || cur.game !== c.game || c.created - cur.end > SESSION_GAP_MS || mapChanged) {
      cur = { key: c.path, game: c.game, map: c.map, mode: c.mode, start: c.created, end: c.created, clips: [] };
      out.push(cur);
    }
    cur.clips.push(c);
    cur.end = c.created;
    if (!cur.map && c.map) {
      cur.map = c.map;
      cur.mode = c.mode;
    }
  }
  if (newestFirst) {
    out.reverse();
    for (const s of out) s.clips.reverse();
  }
  return out;
}
