// A short's project (Content Creator Mode): the game and the
// camera as layers on a 1080×1920 canvas, each with keyframes for where its
// frame is and which part of its picture shows. The geometry here is the
// renderer's (short.rs), so the preview and the file agree.

export const W = 1080;
export const H = 1920;

/** Where a layer is at time `t` (clip seconds): its frame on the canvas, its
 * zoom into the picture (1 = the picture just covers the frame) and which
 * point of the picture is in the middle (0–1 across and down). */
export interface Key {
  t: number;
  x: number;
  y: number;
  w: number;
  h: number;
  zoom: number;
  cx: number;
  cy: number;
}

export type Shape = "rect" | "rounded" | "circle";

export interface Layer {
  visible: boolean;
  shape: Shape;
  radius: number;
  /** At least one, sorted by time. One key: the layer stays put. */
  keys: Key[];
}

export interface Project {
  version: 1;
  /** The part of the clip, in seconds. */
  in: number;
  out: number;
  layout: LayoutId;
  background: "blur" | "black";
  /** The camera shows this many seconds later (earlier if negative), to line it up with your voice. */
  camShift: number;
  game: Layer;
  camera: Layer;
  /** The badge's centre on the canvas, and its size. */
  socials: { show: boolean; x: number; y: number; scale: number };
  fps: 30 | 60;
}

export type LayoutId = "split" | "corner" | "fit" | "game";

export const LAYOUTS: { id: LayoutId; name: string; hint: string; needsCamera: boolean }[] = [
  { id: "split", name: "Split", hint: "You on top, the game below", needsCamera: true },
  { id: "corner", name: "Corner cam", hint: "The game full screen, you in a corner", needsCamera: true },
  { id: "fit", name: "Whole game", hint: "All of the game, blurred around it", needsCamera: false },
  { id: "game", name: "Game only", hint: "The game full screen", needsCamera: false },
];

/** The layer's place at `t`: eased in and out between keyframes (zoom in
 * proportion, so zooming in feels even), held before the first and after the last. */
export function at(keys: Key[], t: number): Key {
  if (keys.length === 0) return { t, x: 0, y: 0, w: W, h: H, zoom: 1, cx: 0.5, cy: 0.5 };
  const first = keys[0];
  const last = keys[keys.length - 1];
  if (keys.length === 1 || t <= first.t) return { ...first, t };
  if (t >= last.t) return { ...last, t };
  let i = 0;
  while (i < keys.length - 2 && t >= keys[i + 1].t) i++;
  const a = keys[i];
  const b = keys[i + 1];
  const u = Math.min(1, Math.max(0, (t - a.t) / Math.max(b.t - a.t, 1e-9)));
  const e = u * u * (3 - 2 * u);
  const mix = (p: number, q: number) => p + (q - p) * e;
  return {
    t,
    x: mix(a.x, b.x),
    y: mix(a.y, b.y),
    w: mix(a.w, b.w),
    h: mix(a.h, b.h),
    zoom: Math.exp(mix(Math.log(Math.max(a.zoom, 1e-3)), Math.log(Math.max(b.zoom, 1e-3)))),
    cx: mix(a.cx, b.cx),
    cy: mix(a.cy, b.cy),
  };
}

/** Where the picture lands: canvas point (X, Y) in the frame shows picture
 * point (sx0 + (X − x) / scale, sy0 + (Y − y) / scale). */
export interface View {
  x: number;
  y: number;
  w: number;
  h: number;
  scale: number;
  sx0: number;
  sy0: number;
}

export function view(k: Key, srcW: number, srcH: number): View {
  const w = Math.max(k.w, 1);
  const h = Math.max(k.h, 1);
  const scale = Math.max(w / srcW, h / srcH) * Math.max(k.zoom, 1);
  const vw = w / scale;
  const vh = h / scale;
  const clamp = (v: number, hi: number) => Math.min(Math.max(v, 0), Math.max(hi, 0));
  return { x: k.x, y: k.y, w, h, scale, sx0: clamp(k.cx * srcW - vw / 2, srcW - vw), sy0: clamp(k.cy * srcH - vh / 2, srcH - vh) };
}

/** The picture point at the middle of the frame, after the view keeps it inside the picture. */
export function centre(v: View, srcW: number, srcH: number) {
  return { cx: (v.sx0 + v.w / v.scale / 2) / srcW, cy: (v.sy0 + v.h / v.scale / 2) / srcH };
}

const key = (t: number, x: number, y: number, w: number, h: number, zoom = 1): Key => ({ t, x, y, w, h, zoom, cx: 0.5, cy: 0.5 });

/** A layout's layers and socials place, for a game picture of `gameW`×`gameH`. */
export function layout(id: LayoutId, t: number, gameW: number, gameH: number): Pick<Project, "game" | "camera" | "background" | "socials"> {
  const socials = { show: true, x: W / 2, y: 0, scale: 1 };
  switch (id) {
    case "split":
      return {
        background: "black",
        camera: { visible: true, shape: "rect", radius: 0, keys: [key(t, 0, 0, W, 768)] },
        game: { visible: true, shape: "rect", radius: 0, keys: [key(t, 0, 768, W, H - 768)] },
        socials: { ...socials, y: 768 },
      };
    case "corner":
      return {
        background: "black",
        camera: { visible: true, shape: "rounded", radius: 48, keys: [key(t, 48, 150, 430, 430)] },
        game: { visible: true, shape: "rect", radius: 0, keys: [key(t, 0, 0, W, H)] },
        // Under the camera, clear of it.
        socials: { ...socials, x: 48 + 215, y: 150 + 430 + 90 },
      };
    case "fit": {
      const h = Math.round((W * gameH) / gameW);
      const top = Math.round((H - h) / 2);
      return {
        background: "blur",
        camera: { visible: true, shape: "rounded", radius: 36, keys: [key(t, 165, top - 460, 750, 420)] },
        game: { visible: true, shape: "rect", radius: 0, keys: [key(t, 0, top, W, h)] },
        socials: { ...socials, y: top + h + 40 },
      };
    }
    case "game":
      return {
        background: "black",
        camera: { visible: false, shape: "rounded", radius: 48, keys: [key(t, 48, 150, 430, 430)] },
        game: { visible: true, shape: "rect", radius: 0, keys: [key(t, 0, 0, W, H)] },
        socials: { ...socials, y: 1380 },
      };
  }
}

export function newProject(start: number, end: number, hasCamera: boolean, gameW: number, gameH: number, fps: number): Project {
  const id: LayoutId = hasCamera ? "split" : "fit";
  return { version: 1, in: start, out: end, layout: id, camShift: 0, fps: fps >= 50 ? 60 : 30, ...layout(id, start, gameW, gameH) };
}

/** A saved project, if it's one this version understands. */
export function loadProject(saved: unknown): Project | null {
  const p = saved as Project | null;
  if (!p || p.version !== 1 || !p.game?.keys?.length || !p.camera?.keys?.length) return null;
  return p;
}

/** Keyframes closer than this to the playhead are the one being edited. */
export const KEY_SNAP = 0.05;

/** Changes the layer at `t`. With one key (no animation) that key changes;
 * with more, the key at `t` changes, or a new one is made there. */
export function edit(layer: Layer, t: number, patch: Partial<Key>) {
  if (layer.keys.length === 1) {
    Object.assign(layer.keys[0], patch);
    return;
  }
  const near = layer.keys.find((k) => Math.abs(k.t - t) < KEY_SNAP);
  if (near) {
    Object.assign(near, patch);
    return;
  }
  insertKey(layer, { ...at(layer.keys, t), ...patch, t });
}

export function insertKey(layer: Layer, k: Key) {
  const i = layer.keys.findIndex((x) => x.t > k.t);
  if (i < 0) layer.keys.push(k);
  else layer.keys.splice(i, 0, k);
}

/** Starts animating, or adds a keyframe at `t` holding where the layer is now. */
export function addKey(layer: Layer, t: number) {
  if (layer.keys.some((k) => Math.abs(k.t - t) < KEY_SNAP)) return;
  insertKey(layer, { ...at(layer.keys, t), t });
}

export function removeKey(layer: Layer, t: number) {
  if (layer.keys.length <= 1) return;
  const i = layer.keys.findIndex((k) => Math.abs(k.t - t) < KEY_SNAP);
  if (i >= 0) layer.keys.splice(i, 1);
}

/** Back to one key: the layer stays where it is at `t`. */
export function clearKeys(layer: Layer, t: number) {
  layer.keys = [{ ...at(layer.keys, t), t }];
}

/** Snaps an edge or centre within `range` canvas pixels of a guide. Returns the shift. */
export function snap(edges: number[], guides: number[], range = 14): number {
  let best = 0;
  let bestDist = range;
  for (const e of edges) {
    for (const g of guides) {
      const d = Math.abs(g - e);
      if (d < bestDist) {
        bestDist = d;
        best = g - e;
      }
    }
  }
  return best;
}
