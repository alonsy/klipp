<script lang="ts">
  // The short editor (Content Creator Mode): a clip and its
  // camera on a 9:16 canvas. Drag to move, Alt+drag to pan inside, scroll to
  // zoom, corners to resize; once a layer has two keyframes, any change at the
  // playhead makes another. The preview uses the renderer's geometry.
  import { onMount, untrack } from "svelte";
  import * as api from "../api";
  import type { CameraTrack, Clip } from "../api";
  import { TrackMixer } from "../audio";
  import { clipTitle, size, timecode } from "../format";
  import Icon from "../Icon.svelte";
  import { DUR, enter } from "../motion";
  import { openClip, openPage, prefs, setClipShort, toast, ui } from "../state.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import WindowControls from "../WindowControls.svelte";
  import * as S from "./short";
  import type { Key, Layer, LayoutId, Project, Shape } from "./short";
  import { drawBadge, lines } from "./socials";

  let { clip, from = "creator" }: { clip: Clip; from?: "creator" | "editor" } = $props();

  // The editor is made again for each clip ({#key}), so the starting values are all it needs.
  const initial = untrack(() => ({
    path: clip.path,
    edit: clip.edit ?? {},
    tracks: [...clip.tracks],
    short: clip.short,
    duration: clip.durationSecs ?? 0,
  }));

  type Target = "game" | "camera" | "socials";
  type LayerId = "game" | "camera";

  let game: HTMLVideoElement;
  let cam = $state<HTMLVideoElement | null>(null);
  let bg = $state<HTMLCanvasElement | null>(null);
  let canvasEl = $state<HTMLDivElement | null>(null);
  let track = $state<CameraTrack | null>(null);
  let trackChecked = $state(false);
  let gameSize = $state<{ w: number; h: number } | null>(null);
  let camSize = $state({ w: 1920, h: 1080 });
  let length = $state(initial.duration);
  let project = $state<Project | null>(null);
  let current = $state(0);
  let playing = $state(false);
  let selected = $state<Target>("game");
  /** What dragging inside a layer does (Alt does the other). */
  let dragModes = $state<Record<LayerId, "move" | "pan">>({ game: "pan", camera: "move" });
  let safeZones = $state(false);
  let stageW = $state(0);
  let stageH = $state(0);
  let k = $derived(Math.max(0.05, Math.min((stageW - 40) / S.W, (stageH - 32) / S.H)));

  let exportId = $state<number | null>(null);
  let exportFraction = $state(0);
  let exported = $state<{ path: string; bytes: number } | null>(null);

  let mixer: TrackMixer | null = null;

  onMount(() => {
    let alive = true;
    api
      .cameraTrack(initial.path)
      .then((t) => {
        if (!alive) return;
        track = t;
        if (t) camSize = { w: t.width || 1920, h: t.height || 1080 };
      })
      .catch((e) => toast(`Couldn't read the camera track: ${e}`))
      .finally(() => (trackChecked = true));

    const m = new TrackMixer();
    m.load(initial.path)
      .then((ok) => {
        if (!alive) return m.close();
        if (!ok) {
          m.close();
          game.muted = false;
          return;
        }
        mixer = m;
        levels();
        if (!game.paused) m.play(game.currentTime);
      })
      .catch(() => {
        m.close();
        if (alive) game.muted = false;
      });

    let frame = 0;
    const tick = () => {
      if (game && project) {
        if (!game.paused) {
          current = game.currentTime;
          if (current >= project.out - 0.01) seek(project.in, true);
          mixer?.follow(game.currentTime);
        }
        followCamera();
        drawBackground();
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    const unlisten = api.onShort({
      progress: (id, f) => id === exportId && (exportFraction = f),
      done: (id, path, bytes) => {
        if (id !== exportId) return;
        exportId = null;
        exported = { path, bytes };
        if (prefs.copyAfterExport) {
          api.copyToClipboard(path).then(
            () => toast("Short saved and copied: paste it into Discord or anywhere."),
            () => toast("Short saved."),
          );
        } else {
          toast("Short saved.", { label: "Show", run: () => api.reveal(path) });
        }
      },
      failed: (id, error) => {
        if (id !== exportId) return;
        exportId = null;
        if (error !== "cancelled") toast(`The short failed: ${error}`);
      },
    });
    return () => {
      alive = false;
      cancelAnimationFrame(frame);
      unlisten.then((off) => off());
      save();
      mixer?.close();
      mixer = null;
      if (exportId != null) api.cancelShort(exportId);
    };
  });

  /** The clip's track levels as the editor left them (what the short's audio uses). */
  function levels() {
    if (!mixer) return;
    const e = initial.edit;
    const master = e.master ?? 1;
    if (mixer.separate) initial.tracks.slice(1).forEach((_, i) => mixer!.setVolume(i, e.muted?.[i] ? 0 : (e.volumes?.[i] ?? 1)));
    else mixer.setVolume(0, 1);
    mixer.setMaster(master);
    mixer.setMonitor(prefs.previewVolume);
  }

  function loaded() {
    length = game.duration;
    gameSize = { w: game.videoWidth || 1920, h: game.videoHeight || 1080 };
  }

  // The project, once the game's size and whether there's a camera are known.
  $effect(() => {
    if (project || !gameSize || !trackChecked) return;
    untrack(() => {
      const start = Math.max(0, initial.edit.in ?? 0);
      const end = Math.min(initial.edit.out ?? length, length || Infinity);
      const p = S.loadProject(initial.short) ?? S.newProject(start, end, !!track, gameSize!.w, gameSize!.h, 60);
      if (!track) p.camera.visible = false;
      project = p;
      selected = track && p.camera.visible ? "camera" : "game";
      seek(p.in);
    });
  });

  // Picture placement at the playhead.
  let gameView = $derived(project && gameSize ? S.view(S.at(project.game.keys, current), gameSize.w, gameSize.h) : null);
  let camView = $derived(project && track ? S.view(S.at(project.camera.keys, current), camSize.w, camSize.h) : null);
  let badge = $derived(project?.socials.show ? drawBadge(prefs.socials, project.socials.scale) : null);
  let badgeUrl = $derived(badge?.toDataURL() ?? null);
  let badgeBox = $derived(project && badge ? { x: project.socials.x - badge.width / 2, y: project.socials.y - badge.height / 2, w: badge.width, h: badge.height } : null);
  let hasSocials = $derived(lines(prefs.socials).length > 0);

  function layerOf(id: LayerId): Layer {
    return id === "game" ? project!.game : project!.camera;
  }

  function srcOf(id: LayerId) {
    return id === "game" ? gameSize! : camSize;
  }

  function viewOf(id: LayerId) {
    return id === "game" ? gameView : camView;
  }

  /** CSS radius of a layer's frame, as the renderer cuts it. */
  function radius(layer: Layer, v: S.View) {
    if (layer.shape === "circle") return Math.min(v.w, v.h) / 2;
    if (layer.shape === "rounded") return Math.min(layer.radius, v.w / 2, v.h / 2);
    return 0;
  }

  const px = (n: number) => `${(n * k).toFixed(2)}px`;

  function frameStyle(layer: Layer, v: S.View) {
    return `left:${px(v.x)};top:${px(v.y)};width:${px(v.w)};height:${px(v.h)};border-radius:${px(radius(layer, v))}`;
  }

  function pictureStyle(v: S.View, src: { w: number; h: number }) {
    return `left:${px(-v.sx0 * v.scale)};top:${px(-v.sy0 * v.scale)};width:${px(src.w * v.scale)};height:${px(src.h * v.scale)}`;
  }

  // Playback.

  /** Where in the camera's own file the picture for clip time `t` is. */
  function camTime(t: number) {
    return t - (project?.camShift ?? 0) - (track?.offset ?? 0);
  }

  function followCamera() {
    if (!cam || !track) return;
    const want = camTime(game.currentTime);
    const dur = cam.duration || Infinity;
    if (want < 0 || want >= dur) {
      if (!cam.paused) cam.pause();
      const hold = Math.min(Math.max(want, 0), Math.max(0, dur - 0.05));
      if (Math.abs(cam.currentTime - hold) > 0.05) cam.currentTime = hold;
      return;
    }
    if (Math.abs(cam.currentTime - want) > (game.paused ? 0.02 : 0.12)) cam.currentTime = want;
    if (!game.paused && cam.paused) cam.play().catch(() => {});
    if (game.paused && !cam.paused) cam.pause();
  }

  function drawBackground() {
    if (!bg || !gameSize || project?.background !== "blur") return;
    const ctx = bg.getContext("2d");
    if (!ctx || game.readyState < 2) return;
    const v = S.view({ t: 0, x: 0, y: 0, w: S.W, h: S.H, zoom: 1, cx: 0.5, cy: 0.5 }, gameSize.w, gameSize.h);
    ctx.drawImage(game, v.sx0, v.sy0, S.W / v.scale, S.H / v.scale, 0, 0, bg.width, bg.height);
  }

  function play() {
    if (!project) return;
    if (game.currentTime >= project.out - 0.05 || game.currentTime < project.in - 0.05) seek(project.in);
    game.play().catch(() => {});
  }

  function pause() {
    game.pause();
  }

  function toggle() {
    if (game.paused) play();
    else pause();
  }

  function seek(t: number, keepPlaying = false) {
    const to = Math.min(Math.max(t, 0), length || t);
    game.currentTime = to;
    current = to;
    if (!keepPlaying) followCamera();
  }

  // Editing: everything goes through here, so it can be undone and is saved.

  // Projects as JSON, for undo and redo.
  let history = $state<string[]>([]);
  let future = $state<string[]>([]);
  let lastWheel = 0;

  function checkpoint() {
    if (!project) return;
    history.push(JSON.stringify(project));
    if (history.length > 80) history.shift();
    future = [];
  }

  function undo() {
    if (!project || history.length === 0) return;
    future.push(JSON.stringify(project));
    project = JSON.parse(history.pop()!);
    changed();
  }

  function redo() {
    if (!project || future.length === 0) return;
    history.push(JSON.stringify(project));
    project = JSON.parse(future.pop()!);
    changed();
  }

  let saveTimer = 0;
  function changed() {
    clearTimeout(saveTimer);
    saveTimer = window.setTimeout(save, 700);
  }

  function save() {
    clearTimeout(saveTimer);
    if (!project) return;
    const snapshot = $state.snapshot(project);
    // The library's clip keeps it too, so opening the short again starts from here.
    setClipShort(initial.path, snapshot);
    api.saveShort(initial.path, snapshot).catch((e) => toast(`Couldn't save the short: ${e}`));
  }

  function change(fn: () => void) {
    if (!project) return;
    checkpoint();
    fn();
    changed();
  }

  function applyLayout(id: LayoutId) {
    change(() => {
      const p = project!;
      const l = S.layout(id, p.in, gameSize!.w, gameSize!.h);
      p.game = l.game;
      p.camera = l.camera;
      p.background = l.background;
      p.socials = { ...l.socials, show: p.socials.show, scale: p.socials.scale };
      p.layout = id;
      if (!track) p.camera.visible = false;
      selected = track && p.camera.visible ? "camera" : "game";
    });
  }

  const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);

  /** Zooms a layer's picture by `factor`, keeping `point` (canvas) where it is (the frame's middle by default). */
  function zoomBy(id: LayerId, factor: number, point?: { x: number; y: number }) {
    const layer = layerOf(id);
    const src = srcOf(id);
    const key = S.at(layer.keys, current);
    const v = S.view(key, src.w, src.h);
    const zoom = clamp(key.zoom * factor, 1, 6);
    const p = point ?? { x: v.x + v.w / 2, y: v.y + v.h / 2 };
    const ix = v.sx0 + (p.x - v.x) / v.scale;
    const iy = v.sy0 + (p.y - v.y) / v.scale;
    const s1 = Math.max(v.w / src.w, v.h / src.h) * zoom;
    const cx = (ix - (p.x - v.x) / s1 + v.w / s1 / 2) / src.w;
    const cy = (iy - (p.y - v.y) / s1 + v.h / s1 / 2) / src.h;
    S.edit(layer, current, { zoom, cx: clamp(cx, 0, 1), cy: clamp(cy, 0, 1) });
    changed();
  }

  function setZoom(id: LayerId, zoom: number) {
    // One undo step per slider drag.
    if (Date.now() - lastWheel > 600) checkpoint();
    lastWheel = Date.now();
    const key = S.at(layerOf(id).keys, current);
    zoomBy(id, zoom / key.zoom);
  }

  function setShape(shape: Shape) {
    change(() => {
      const layer = project!.camera;
      layer.shape = shape;
      // A circle is round in a square frame.
      if (shape === "circle") {
        for (const key of layer.keys) {
          const s = Math.min(key.w, key.h);
          key.x += (key.w - s) / 2;
          key.y += (key.h - s) / 2;
          key.w = key.h = s;
        }
      }
    });
  }

  /** A quick zoom at the playhead: as it is now, then `factor` closer (or back to 1) a moment later. */
  function punch(id: LayerId, factor: number | null) {
    const t = current;
    const end = Math.min(t + 0.35, project!.out);
    if (end - t < 0.1) return;
    change(() => {
      const layer = layerOf(id);
      const now = S.at(layer.keys, t);
      // Keys inside the move would fight it.
      layer.keys = layer.keys.filter((key) => key.t <= t + S.KEY_SNAP || key.t >= end - S.KEY_SNAP);
      S.addKey(layer, t);
      const target: Key = { ...now, t: end, zoom: factor == null ? 1 : clamp(now.zoom * factor, 1, 6) };
      const there = layer.keys.findIndex((key) => Math.abs(key.t - end) < S.KEY_SNAP);
      if (there >= 0) layer.keys[there] = target;
      else S.insertKey(layer, target);
    });
  }

  // The canvas: pick, move, pan, resize, zoom.

  interface Drag {
    target: Target;
    mode: "move" | "pan" | "resize";
    corner?: string;
    start: { x: number; y: number };
    key?: Key;
    view?: S.View;
    socials?: { x: number; y: number };
    moved: boolean;
  }
  let drag: Drag | null = null;

  function toCanvas(e: { clientX: number; clientY: number }) {
    const r = canvasEl!.getBoundingClientRect();
    return { x: (e.clientX - r.left) / k, y: (e.clientY - r.top) / k };
  }

  function inside(p: { x: number; y: number }, b: { x: number; y: number; w: number; h: number } | null) {
    return !!b && p.x >= b.x && p.x <= b.x + b.w && p.y >= b.y && p.y <= b.y + b.h;
  }

  function hit(p: { x: number; y: number }): Target | null {
    if (badgeBox && inside(p, badgeBox)) return "socials";
    if (track && project?.camera.visible && inside(p, camView)) return "camera";
    if (project?.game.visible && inside(p, gameView)) return "game";
    return null;
  }

  function down(e: PointerEvent) {
    if (!project || e.button !== 0) return;
    const p = toCanvas(e);
    const corner = (e.target as HTMLElement).dataset.corner;
    const target = corner ? selected : hit(p);
    if (!target) return;
    selected = target;
    if (!game.paused) pause();
    checkpoint();
    if (target === "socials") {
      drag = { target, mode: "move", start: p, socials: { x: project.socials.x, y: project.socials.y }, moved: false };
    } else {
      const flip = (m: "move" | "pan") => (m === "move" ? "pan" : "move");
      const mode = corner ? "resize" : e.altKey ? flip(dragModes[target]) : dragModes[target];
      drag = { target, mode, corner, start: p, key: S.at(layerOf(target).keys, current), view: viewOf(target)!, moved: false };
    }
    canvasEl!.setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function move(e: PointerEvent) {
    if (!drag || !project) return;
    const p = toCanvas(e);
    const dx = p.x - drag.start.x;
    const dy = p.y - drag.start.y;
    if (!drag.moved && Math.hypot(dx, dy) * k < 2) return;
    drag.moved = true;
    if (drag.target === "socials") {
      let x = drag.socials!.x + dx;
      x += S.snap([x], [S.W / 2]);
      project.socials.x = Math.round(x);
      project.socials.y = Math.round(clamp(drag.socials!.y + dy, 0, S.H));
      changed();
      return;
    }
    const id = drag.target;
    const layer = layerOf(id);
    const k0 = drag.key!;
    if (drag.mode === "move") {
      const other = id === "game" ? camView : gameView;
      const ys = other ? [other.y, other.y + other.h] : [];
      const xs = other ? [other.x, other.x + other.w] : [];
      let x = k0.x + dx;
      let y = k0.y + dy;
      x += S.snap([x, x + k0.w / 2, x + k0.w], [0, S.W / 2, S.W, ...xs]);
      y += S.snap([y, y + k0.h / 2, y + k0.h], [0, S.H / 2, S.H, ...ys]);
      S.edit(layer, current, { x: Math.round(x), y: Math.round(y) });
    } else if (drag.mode === "pan") {
      const src = srcOf(id);
      const v = drag.view!;
      const c = S.centre(v, src.w, src.h);
      S.edit(layer, current, { cx: clamp(c.cx - dx / (v.scale * src.w), 0, 1), cy: clamp(c.cy - dy / (v.scale * src.h), 0, 1) });
    } else {
      const c = drag.corner!;
      let w = c.includes("e") ? k0.w + dx : c.includes("w") ? k0.w - dx : k0.w;
      let h = c.includes("s") ? k0.h + dy : c.includes("n") ? k0.h - dy : k0.h;
      if (e.shiftKey || layer.shape === "circle") {
        const s = Math.max(w / k0.w, h / k0.h);
        w = k0.w * s;
        h = k0.h * s;
      }
      w = Math.max(80, w);
      h = Math.max(80, h);
      const x = c.includes("w") ? k0.x + k0.w - w : k0.x;
      const y = c.includes("n") ? k0.y + k0.h - h : k0.y;
      S.edit(layer, current, { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) });
    }
    changed();
  }

  function up(e: PointerEvent) {
    if (drag && !drag.moved) history.pop();
    drag = null;
    if (canvasEl?.hasPointerCapture(e.pointerId)) canvasEl.releasePointerCapture(e.pointerId);
  }

  // Not passive, so scrolling over the canvas zooms instead of scrolling the page.
  $effect(() => {
    const el = canvasEl;
    if (!el) return;
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  });

  function wheel(e: WheelEvent) {
    if (!project) return;
    e.preventDefault();
    const p = toCanvas(e);
    const target = hit(p) ?? selected;
    if (Date.now() - lastWheel > 600) checkpoint();
    lastWheel = Date.now();
    if (target === "socials") {
      project.socials.scale = clamp(project.socials.scale * Math.exp(-e.deltaY * 0.001), 0.5, 2.2);
      changed();
      return;
    }
    selected = target;
    zoomBy(target, Math.exp(-e.deltaY * 0.0015), p);
  }

  // Keyframes of the selected layer.

  let selLayer = $derived(project && selected !== "socials" ? layerOf(selected) : null);
  let keyHere = $derived(!!selLayer && selLayer.keys.length > 1 && selLayer.keys.some((key) => Math.abs(key.t - current) < S.KEY_SNAP));
  let selKey = $derived(selLayer ? S.at(selLayer.keys, current) : null);

  function jumpKey(dir: 1 | -1) {
    if (!selLayer) return;
    const times = selLayer.keys.map((key) => key.t);
    const t = dir > 0 ? times.find((x) => x > current + 0.01) : [...times].reverse().find((x) => x < current - 0.01);
    if (t != null) seek(t);
  }

  // The timeline.

  let lane = $state<HTMLDivElement | null>(null);
  const pct = (t: number) => (length > 0 ? (clamp(t, 0, length) / length) * 100 : 0);
  let ticks = $derived.by(() => {
    const step = length > 40 ? 5 : length > 15 ? 2 : 1;
    return Array.from({ length: Math.floor(length / step) + 1 }, (_, i) => i * step);
  });

  function laneTime(e: PointerEvent) {
    const r = lane!.getBoundingClientRect();
    return clamp((e.clientX - r.left) / r.width, 0, 1) * length;
  }

  interface LaneDrag {
    kind: "seek" | "in" | "out" | "key";
    layer?: LayerId;
    index?: number;
    moved: boolean;
  }
  let laneDrag: LaneDrag | null = null;

  function laneDown(e: PointerEvent) {
    if (!project || e.button !== 0) return;
    const el = e.target as HTMLElement;
    const trim = el.dataset.trim as "in" | "out" | undefined;
    const keyAt = el.dataset.key;
    if (!game.paused && !trim) pause();
    if (trim) {
      checkpoint();
      laneDrag = { kind: trim, moved: false };
    } else if (keyAt) {
      const [layer, index] = keyAt.split(":");
      checkpoint();
      laneDrag = { kind: "key", layer: layer as LayerId, index: Number(index), moved: false };
      selected = layer as LayerId;
    } else {
      laneDrag = { kind: "seek", moved: false };
      seek(laneTime(e));
    }
    lane!.setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function laneMove(e: PointerEvent) {
    if (!laneDrag || !project) return;
    const t = laneTime(e);
    laneDrag.moved = true;
    if (laneDrag.kind === "seek") seek(t);
    else if (laneDrag.kind === "in") {
      project.in = Math.round(clamp(t, 0, project.out - 1) * 100) / 100;
      seek(project.in);
      changed();
    } else if (laneDrag.kind === "out") {
      project.out = Math.round(clamp(t, project.in + 1, length) * 100) / 100;
      seek(Math.max(project.in, project.out - 1.5));
      changed();
    } else {
      const keys = layerOf(laneDrag.layer!).keys;
      const i = laneDrag.index!;
      const lo = i > 0 ? keys[i - 1].t + 0.06 : 0;
      const hi = i < keys.length - 1 ? keys[i + 1].t - 0.06 : length;
      keys[i].t = Math.round(clamp(t, lo, hi) * 100) / 100;
      seek(keys[i].t);
      changed();
    }
  }

  function laneUp(e: PointerEvent) {
    if (laneDrag?.kind === "key" && !laneDrag.moved) {
      history.pop();
      seek(layerOf(laneDrag.layer!).keys[laneDrag.index!].t);
    }
    laneDrag = null;
    if (lane?.hasPointerCapture(e.pointerId)) lane.releasePointerCapture(e.pointerId);
  }

  // Export.

  async function render() {
    if (!project || exportId != null) return;
    save();
    exported = null;
    const p = $state.snapshot(project) as Project;
    let b: api.ShortRequest["badge"] = null;
    const c = p.socials.show ? drawBadge(prefs.socials, p.socials.scale) : null;
    if (c) {
      const data = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
      b = { x: p.socials.x - c.width / 2, y: p.socials.y - c.height / 2, width: c.width, height: c.height, rgba: Array.from(data) };
    }
    const e = initial.edit;
    const master = e.master ?? 1;
    const separate = initial.tracks.length > 1 ? initial.tracks.slice(1) : [];
    try {
      exportFraction = 0;
      exportId = await api.startShort({
        path: initial.path,
        start: p.in,
        end: p.out,
        background: p.background,
        camShift: p.camShift,
        game: p.game,
        camera: track && p.camera.visible ? p.camera : null,
        badge: b,
        volumes: separate.map((_, i) => (e.muted?.[i] ? 0 : (e.volumes?.[i] ?? 1)) * master),
        mixVolume: master,
        fps: p.fps,
      });
    } catch (err) {
      toast(`Couldn't start the short: ${err}`);
    }
  }

  function back() {
    save();
    if (from === "editor") {
      ui.short = null;
      openClip(clip);
    } else {
      openPage("creator");
    }
  }

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    if (target.tagName === "INPUT" && (target as HTMLInputElement).type !== "range") return;
    if (target.tagName === "SELECT" || ui.closeNotice) return;
    if (e.ctrlKey && (e.key === "z" || e.key === "Z")) {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
      return;
    }
    if (e.ctrlKey && (e.key === "y" || e.key === "Y")) {
      e.preventDefault();
      redo();
      return;
    }
    if (e.ctrlKey || e.altKey) return;
    switch (e.key) {
      case " ":
        e.preventDefault();
        toggle();
        break;
      case "k":
      case "K":
        if (selLayer) change(() => S.addKey(selLayer!, current));
        break;
      case "Delete":
      case "Backspace":
        if (selLayer && keyHere) change(() => S.removeKey(selLayer!, current));
        break;
      case "ArrowLeft":
        e.preventDefault();
        seek(current - (e.shiftKey ? 1 : 1 / 30));
        break;
      case "ArrowRight":
        e.preventDefault();
        seek(current + (e.shiftKey ? 1 : 1 / 30));
        break;
      case "Escape":
        back();
        break;
    }
  }

  let shortLength = $derived(project ? project.out - project.in : 0);
  let layerIds = $derived<LayerId[]>(track ? ["game", "camera"] : ["game"]);
  let layerOptions = $derived<[Target, string][]>([["game", "Game"], ...(track ? [["camera", "Camera"] as [Target, string]] : []), ...(hasSocials ? [["socials", "Socials"] as [Target, string]] : [])]);
</script>

<svelte:window onkeydown={onKey} />

<section class="short">
  <header data-tauri-drag-region>
    <button class="ghost icon" onclick={back} title="Back (Esc)" aria-label="Back"><Icon name="back" size={18} /></button>
    <div class="title" data-tauri-drag-region>
      <h1>Short</h1>
      <span class="tag">Experimental</span>
      <span class="sub">{clipTitle(clip)} · 1080×1920</span>
    </div>
    <div class="actions">
      <button class="ghost icon" onclick={undo} disabled={history.length === 0} title="Undo (Ctrl+Z)" aria-label="Undo"><Icon name="undo" size={17} /></button>
      <button class="ghost icon redo" onclick={redo} disabled={future.length === 0} title="Redo (Ctrl+Y)" aria-label="Redo"><Icon name="undo" size={17} /></button>
      {#if exportId != null}
        <div class="progress" title="Making the short" in:enter={{ x: 8, y: 0, duration: DUR.base }}>
          <div class="bar"><div class="fill" style="width: {(exportFraction * 100).toFixed(1)}%"></div></div>
          <span class="mono">{Math.round(exportFraction * 100)}%</span>
          <button class="ghost icon" onclick={() => exportId != null && api.cancelShort(exportId)} title="Cancel" aria-label="Cancel"><Icon name="x" size={15} /></button>
        </div>
      {:else}
        {#if exported}
          <button class="ghost" in:enter={{ x: 8, y: 0, duration: DUR.base }} onclick={() => exported && api.reveal(exported.path)} title={exported.path}><Icon name="folder" size={16} />Show <span class="mono size">{size(exported.bytes)}</span></button>
          <button class="ghost" in:enter={{ x: 8, y: 0, duration: DUR.base }} onclick={() => exported && api.copyToClipboard(exported.path).then(() => toast("Copied: paste it anywhere."), (e) => toast(`Couldn't copy: ${e}`))}><Icon name="copy" size={16} />Copy</button>
        {/if}
        <button class="primary" onclick={render} disabled={!project}><Icon name="export" size={16} />Export</button>
      {/if}
    </div>
    <WindowControls />
  </header>

  <div class="body">
    <div class="stage" bind:clientWidth={stageW} bind:clientHeight={stageH}>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="canvas"
        class:loading={!project}
        bind:this={canvasEl}
        style="width: {px(S.W)}; height: {px(S.H)}"
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointercancel={up}
        ondblclick={toggle}
      >
        {#if project?.background === "blur"}<canvas class="bg" bind:this={bg} width={S.W / 8} height={S.H / 8} style="--blur: {px(48)}"></canvas>{/if}
        <div class="layer" class:hidden={!project?.game.visible} style={project && gameView ? frameStyle(project.game, gameView) : "visibility:hidden"}>
          <video
            bind:this={game}
            src={api.fileUrl(initial.path)}
            preload="auto"
            muted
            style={gameView && gameSize ? pictureStyle(gameView, gameSize) : ""}
            onloadedmetadata={loaded}
            onplay={() => {
              playing = true;
              mixer?.play(game.currentTime);
            }}
            onpause={() => {
              playing = false;
              mixer?.pause();
            }}
            onseeked={() => {
              current = game.currentTime;
              if (!game.paused) mixer?.play(game.currentTime);
            }}
          ></video>
        </div>
        {#if track}
          <div class="layer" class:hidden={!project?.camera.visible} style={project && camView ? frameStyle(project.camera, camView) : "visibility:hidden"}>
            <video bind:this={cam} src={api.fileUrl(track.path)} preload="auto" muted style={camView ? pictureStyle(camView, camSize) : ""}></video>
          </div>
        {/if}
        {#if badgeUrl && badgeBox}
          <img class="badge-img" src={badgeUrl} alt="" draggable="false" style="left:{px(badgeBox.x)};top:{px(badgeBox.y)};width:{px(badgeBox.w)}" />
        {/if}
        {#if safeZones}
          <div class="zone" style="left:0;top:0;width:{px(S.W)};height:{px(150)}"><span>Top tabs</span></div>
          <div class="zone" style="left:{px(S.W - 150)};top:{px(820)};width:{px(150)};height:{px(760)}"><span>Buttons</span></div>
          <div class="zone" style="left:0;top:{px(S.H - 420)};width:{px(S.W - 150)};height:{px(420)}"><span>Caption and name</span></div>
        {/if}
        {#if project && selected !== "socials" && viewOf(selected) && layerOf(selected).visible}
          {@const v = viewOf(selected)!}
          <div class="select" style="left:{px(v.x)};top:{px(v.y)};width:{px(v.w)};height:{px(v.h)}">
            {#each ["nw", "ne", "sw", "se"] as c (c)}<span class="corner {c}" data-corner={c}></span>{/each}
          </div>
        {:else if selected === "socials" && badgeBox}
          <div class="select soft" style="left:{px(badgeBox.x)};top:{px(badgeBox.y)};width:{px(badgeBox.w)};height:{px(badgeBox.h)}"></div>
        {/if}
      </div>
    </div>

    <aside class="side">
      {#if project}
        <div class="section">
          <h3>Layout</h3>
          <div class="layouts">
            {#each S.LAYOUTS as l (l.id)}
              <button class="layout" class:on={project.layout === l.id} disabled={l.needsCamera && !track} onclick={() => applyLayout(l.id)} title={l.needsCamera && !track ? "This clip has no camera" : l.hint}>
                <span class="mini {l.id}"><span class="g"></span><span class="c"></span></span>
                {l.name}
              </button>
            {/each}
          </div>
        </div>

        <div class="section">
          <Segmented label="Layer" value={selected} options={layerOptions} onchange={(v) => (selected = v)} />
          {#key selected}
            <div class="props" in:enter={{ x: 8, y: 0, duration: DUR.base }}>
              {#if selected === "socials"}
                <div class="row">
                  <span>Visible</span>
                  <Toggle checked={project.socials.show} label="Show my socials" onchange={(v) => change(() => (project!.socials.show = v))} />
                </div>
                <div class="row">
                  <span>Size</span>
                  <Slider min={0.5} max={2.2} step={0.05} value={project.socials.scale} label="Socials size" oninput={(v) => (project!.socials.scale = v)} onchange={() => changed()} />
                </div>
                <button
                  class="link"
                  onclick={() => {
                    ui.creatorTab = "socials";
                    openPage("creator");
                  }}>Change names</button
                >
              {:else if selLayer && selKey}
                {#if selected === "camera"}
                  <div class="row">
                    <span>Visible</span>
                    <Toggle checked={selLayer.visible} label="Show the camera" onchange={(v) => change(() => (project!.camera.visible = v))} />
                  </div>
                {/if}
                <div class="row">
                  <span>Zoom</span>
                  <Slider min={1} max={4} step={0.01} value={selKey.zoom} label="Zoom" oninput={(v) => setZoom(selected as LayerId, v)} onchange={() => {}} />
                  <span class="value mono">{selKey.zoom.toFixed(2)}×</span>
                </div>
                <div class="row">
                  <span>Drag</span>
                  <Segmented
                    label="Dragging"
                    value={dragModes[selected as LayerId]}
                    options={[["move", "Move"], ["pan", "Pan inside"]]}
                    onchange={(v) => (dragModes[selected as LayerId] = v)}
                  />
                </div>
                {#if selected === "camera"}
                  <div class="row">
                    <span>Shape</span>
                    <Segmented label="Shape" value={selLayer.shape} options={[["rect", "Square"], ["rounded", "Rounded"], ["circle", "Circle"]]} onchange={setShape} />
                  </div>
                  {#if selLayer.shape === "rounded"}
                    <div class="row">
                      <span>Corners</span>
                      <Slider min={8} max={160} step={1} value={selLayer.radius} label="Corner size" oninput={(v) => (project!.camera.radius = v)} onchange={() => changed()} />
                    </div>
                  {/if}
                  <div class="row" title="If your mouth moves before your voice, nudge the camera later. After it, earlier.">
                    <span>Sync</span>
                    <Slider min={-0.5} max={0.5} step={0.01} value={project.camShift} label="Camera sync" oninput={(v) => (project!.camShift = v)} onchange={() => changed()} />
                    <span class="value mono">{project.camShift > 0 ? "+" : ""}{project.camShift.toFixed(2)} s</span>
                  </div>
                {/if}

                <div class="motion">
                  <h3>Motion{#if selLayer.keys.length > 1}<span>{selLayer.keys.length} keyframes</span>{/if}</h3>
                  <p class="note">
                    {#if selLayer.keys.length > 1}Move the playhead, then drag or zoom: Klipp adds a keyframe there.
                    {:else}Stays still. Add a keyframe to start moving it.{/if}
                  </p>
                  <div class="buttons">
                    <button onclick={() => change(() => S.addKey(selLayer!, current))} disabled={keyHere} title="Add a keyframe at the playhead (K)"><Icon name="keyframe" size={15} />Keyframe</button>
                    {#if keyHere}
                      <button class="ghost" onclick={() => change(() => S.removeKey(selLayer!, current))} title="Remove the keyframe at the playhead (Delete)"><Icon name="x" size={14} />Remove</button>
                    {/if}
                    <button class="ghost icon" onclick={() => jumpKey(-1)} disabled={selLayer.keys.length < 2} title="Previous keyframe" aria-label="Previous keyframe"><Icon name="prevFrame" size={15} /></button>
                    <button class="ghost icon" onclick={() => jumpKey(1)} disabled={selLayer.keys.length < 2} title="Next keyframe" aria-label="Next keyframe"><Icon name="nextFrame" size={15} /></button>
                  </div>
                  <div class="buttons">
                    <button onclick={() => punch(selected as LayerId, 1.6)} title="Zooms in quickly at the playhead"><Icon name="zoomIn" size={15} />Punch in</button>
                    <button onclick={() => punch(selected as LayerId, null)} title="Zooms back out quickly at the playhead">Zoom out</button>
                    {#if selLayer.keys.length > 1}
                      <button class="ghost" onclick={() => change(() => S.clearKeys(selLayer!, current))} title="Keep it as it is at the playhead, for the whole short">Stop moving</button>
                    {/if}
                  </div>
                </div>
              {/if}
            </div>
          {/key}
        </div>

        <div class="section">
          <h3>Look</h3>
          <div class="props">
            <div class="row">
              <span>Background</span>
              <Segmented label="Background" value={project.background} options={[["blur", "Blurred"], ["black", "Black"]]} onchange={(v) => change(() => (project!.background = v))} />
            </div>
            <div class="row">
              <span>Frame rate</span>
              <Segmented label="Frame rate" value={project.fps} options={[[30, "30"], [60, "60"]]} onchange={(v) => change(() => (project!.fps = v === 30 ? 30 : 60))} />
            </div>
            <div class="row" title="Where the apps' buttons go">
              <span>Safe zones</span>
              <Toggle checked={safeZones} label="Show where the apps' buttons go" onchange={(v) => (safeZones = v)} />
            </div>
          </div>
        </div>
      {:else}
        <div class="loading-side" aria-label="Loading the clip">
          {#each [60, 100, 80, 100] as w, i (i)}<span class="skeleton text" style:width="{w}%"></span>{/each}
        </div>
      {/if}
    </aside>
  </div>

  {#if project}
    <div class="timeline">
      <div class="transport">
        <div class="t-left">
          <span class="mono time">{timecode(current)}</span>
          <span class="mono length" class:long={shortLength > 60} title={shortLength > 60 ? "Over 60 s: YouTube may not treat it as a Short" : "The short's length"}>{timecode(shortLength)}</span>
        </div>
        <button class="play" class:playing onclick={toggle} aria-label={playing ? "Pause" : "Play"} title="Play / pause (Space)">
          <span class="glyph to-play"><Icon name="play" size={18} /></span>
          <span class="glyph to-pause"><Icon name="pause" size={18} /></span>
        </button>
        <p class="hint">
          {#if selected === "socials"}Drag to move · Scroll to resize
          {:else}Drag to {dragModes[selected] === "move" ? "move" : "pan"} · Alt to {dragModes[selected] === "move" ? "pan" : "move"} · Scroll to zoom · Corners resize{/if}
        </p>
      </div>
      <div class="names">
        <span>Trim</span>
        <span class:on={selected === "game"}>Game</span>
        {#if track}<span class:on={selected === "camera"}>Camera</span>{/if}
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="lane" bind:this={lane} onpointerdown={laneDown} onpointermove={laneMove} onpointerup={laneUp} onpointercancel={laneUp}>
        <div class="ruler">
          {#each ticks as t (t)}<span class="tick" style="left: {pct(t)}%"><span class="mono">{t}s</span></span>{/each}
        </div>
        <div class="trim">
          <div class="out-left" style="width: {pct(project.in)}%"></div>
          <div class="kept" class:trimmed={project.in > 0.01 || project.out < length - 0.01} style="left: {pct(project.in)}%; width: {pct(project.out) - pct(project.in)}%">
            <span class="handle in" data-trim="in"></span>
            <span class="handle out" data-trim="out"></span>
          </div>
        </div>
        {#each layerIds as id (id)}
          {@const layer = layerOf(id)}
          <div class="keys" class:on={selected === id}>
            {#if layer.keys.length > 1}
              <div class="path" style="left: {pct(layer.keys[0].t)}%; width: {pct(layer.keys[layer.keys.length - 1].t) - pct(layer.keys[0].t)}%"></div>
              {#each layer.keys as key, i (i)}
                <span class="diamond" class:here={Math.abs(key.t - current) < S.KEY_SNAP} style="left: {pct(key.t)}%" data-key="{id}:{i}" title="{timecode(key.t)} · drag to move"></span>
              {/each}
            {/if}
          </div>
        {/each}
        <div class="playhead" style="left: {pct(current)}%"></div>
      </div>
    </div>
  {/if}
</section>

<style>
  .short {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
  }

  header {
    position: relative;
    z-index: 30;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    height: 44px;
    padding-left: var(--sp-2);
  }

  /* Window controls sit in the corner, like a title bar's. */
  header > :global(.controls) {
    align-self: flex-start;
    margin-left: var(--sp-2);
  }

  .title {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex: 1;
    min-width: 0;
    margin-left: var(--sp-1);
  }

  /* So the title drags the window too. */
  .title > * {
    pointer-events: none;
  }

  h1 {
    margin: 0;
    flex-shrink: 0;
    font-family: var(--font-display);
    font-size: var(--fs-base);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  /* As Field's badge. */
  .tag {
    flex-shrink: 0;
    padding: 1px 6px;
    border-radius: var(--r-xs);
    background: var(--surface-3);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 500;
  }

  .sub {
    min-width: 0;
    margin-left: var(--sp-1);
    color: var(--faint);
    font-size: var(--fs-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .actions {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: var(--sp-1);
  }

  .primary {
    gap: 6px;
    margin-left: var(--sp-1);
  }

  .redo :global(.icon) {
    transform: scaleX(-1);
  }

  .size {
    color: var(--muted);
    font-size: var(--fs-xs);
  }

  .progress {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-left: var(--sp-2);
    font-size: var(--fs-sm);
  }

  .bar {
    width: 120px;
    height: 4px;
    border-radius: var(--r-full);
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transition: width var(--dur-fast) var(--ease-out);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 320px;
  }

  .stage {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 0;
    min-height: 0;
    background: var(--stage);
    overflow: hidden;
  }

  /* Black, whatever the theme: it's what the short has behind the picture. */
  .canvas {
    position: relative;
    flex-shrink: 0;
    overflow: hidden;
    background: #000;
    border-radius: var(--r-xs);
    box-shadow: var(--shadow-3);
    cursor: grab;
    touch-action: none;
    transition: opacity var(--dur) var(--ease-out);
  }

  .canvas.loading {
    opacity: 0.4;
  }

  .bg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    filter: blur(var(--blur)) brightness(0.55) saturate(0.8);
    transform: scale(1.08);
  }

  .layer {
    position: absolute;
    overflow: hidden;
  }

  .layer.hidden {
    visibility: hidden;
  }

  .layer video {
    position: absolute;
    max-width: none;
    object-fit: fill;
    pointer-events: none;
  }

  .badge-img {
    position: absolute;
    pointer-events: none;
    max-width: none;
  }

  .zone {
    position: absolute;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    padding: var(--sp-1);
    background: color-mix(in srgb, var(--danger) 22%, transparent);
    outline: 1px dashed color-mix(in srgb, var(--danger) 70%, transparent);
    outline-offset: -1px;
    color: var(--text);
    font-size: var(--fs-2xs);
    pointer-events: none;
    animation: fade var(--dur) var(--ease-out) backwards;
  }

  .select {
    position: absolute;
    outline: 2px solid var(--accent);
    pointer-events: none;
  }

  .select.soft {
    outline-style: dashed;
  }

  .corner {
    position: absolute;
    width: 12px;
    height: 12px;
    margin: -6px;
    border-radius: 50%;
    background: var(--text);
    box-shadow: var(--shadow-1);
    pointer-events: auto;
  }

  /* A bigger place to catch it than to see it. */
  .corner::after {
    content: "";
    position: absolute;
    inset: -6px;
  }

  .corner.nw {
    left: 0;
    top: 0;
    cursor: nwse-resize;
  }

  .corner.ne {
    right: 0;
    top: 0;
    cursor: nesw-resize;
  }

  .corner.sw {
    left: 0;
    bottom: 0;
    cursor: nesw-resize;
  }

  .corner.se {
    right: 0;
    bottom: 0;
    cursor: nwse-resize;
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: var(--sp-5);
    min-height: 0;
    padding: var(--sp-3) var(--sp-4) var(--sp-4);
    overflow-y: auto;
  }

  .section,
  .props {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-width: 0;
  }

  .props {
    gap: var(--sp-1);
  }

  .section :global(.segmented) {
    display: grid;
    width: 100%;
  }

  h3 {
    display: flex;
    justify-content: space-between;
    margin: 0;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--faint);
  }

  h3 span {
    font-weight: 400;
  }

  .layouts {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--sp-1);
    margin-inline: calc(-1 * var(--sp-1));
  }

  .layout {
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding: var(--sp-2) 2px 6px;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-xs);
    white-space: nowrap;
  }

  .layout:hover {
    background: var(--hover);
    color: var(--text);
  }

  .layout.on {
    background: var(--surface-2);
    color: var(--text);
  }

  .mini {
    position: relative;
    width: 24px;
    height: 42px;
    border-radius: var(--r-xs);
    background: var(--stage);
    overflow: hidden;
  }

  .mini .g,
  .mini .c {
    position: absolute;
  }

  .mini .g {
    background: var(--faint);
  }

  .mini .c {
    background: var(--accent);
  }

  .mini.split .c {
    inset: 0 0 60% 0;
  }

  .mini.split .g {
    inset: 40% 0 0 0;
  }

  .mini.corner .g {
    inset: 0;
  }

  .mini.corner .c {
    left: 2px;
    top: 4px;
    width: 10px;
    height: 10px;
    border-radius: 2px;
  }

  .mini.fit .g {
    left: 0;
    right: 0;
    top: 36%;
    height: 28%;
  }

  .mini.fit .c {
    left: 15%;
    right: 15%;
    top: 14%;
    height: 16%;
    border-radius: 2px;
  }

  .mini.game .g {
    inset: 0;
  }

  .mini.game .c {
    display: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 36px;
    font-size: var(--fs-control);
  }

  .row > span:first-child {
    width: 72px;
    flex-shrink: 0;
    color: var(--muted);
  }

  .row :global(.slider) {
    flex: 1;
    min-width: 0;
  }

  .row :global(.segmented) {
    flex: 1;
    min-width: 0;
  }

  /* A narrow panel: the words have to fit in equal parts. */
  .section :global(.segmented button) {
    min-width: 0;
    padding-inline: var(--sp-1);
  }

  .row :global(.toggle) {
    margin-left: auto;
  }

  .value {
    min-width: 48px;
    text-align: right;
    font-size: var(--fs-sm);
  }

  .note {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }

  .link {
    align-self: flex-start;
    padding: 0;
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .link:hover {
    background: transparent;
    color: var(--text);
    text-decoration: underline;
  }

  .motion {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin-top: var(--sp-4);
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
  }

  .buttons button {
    gap: 6px;
    padding: 6px 10px;
    font-size: var(--fs-sm);
  }

  .buttons button.icon {
    width: 30px;
    height: 30px;
    padding: 0;
  }

  .loading-side {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }

  .timeline {
    flex-shrink: 0;
    display: grid;
    grid-template-columns: 126px minmax(0, 1fr);
    grid-template-rows: auto auto;
    column-gap: var(--sp-2);
    padding: 0 18px var(--sp-2) 16px;
  }

  .transport {
    grid-column: 1 / 3;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: var(--sp-3);
    height: 52px;
  }

  .t-left {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }

  .play {
    position: relative;
    width: 38px;
    height: 38px;
    padding: 0;
    border-radius: 50%;
    background: var(--text);
    color: var(--on-accent);
  }

  .play:hover {
    background: var(--text);
    transform: scale(1.05);
  }

  .play:active {
    transform: scale(0.94);
  }

  /* Both shapes are there; one turns into the other. */
  .glyph {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    transition:
      opacity var(--dur-fast) var(--ease-out),
      transform var(--dur) var(--ease-out);
  }

  .to-pause,
  .playing .to-play {
    opacity: 0;
    transform: scale(0.6);
  }

  .playing .to-pause {
    opacity: 1;
    transform: none;
  }

  .time {
    font-size: var(--fs-base);
    font-weight: 500;
  }

  .length {
    color: var(--faint);
    font-size: var(--fs-xs);
  }

  .length.long {
    color: var(--warn);
  }

  .hint {
    justify-self: end;
    min-width: 0;
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .names {
    display: grid;
    grid-auto-rows: 26px;
    padding-top: 20px;
    color: var(--muted);
    font-size: var(--fs-sm);
    font-weight: 550;
  }

  .names span {
    display: flex;
    align-items: center;
    transition: color var(--dur-fast);
  }

  .names .on {
    color: var(--text);
  }

  .lane {
    position: relative;
    display: grid;
    grid-auto-rows: 26px;
    padding-top: 20px;
    cursor: pointer;
    touch-action: none;
  }

  .ruler {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 18px;
  }

  .tick {
    position: absolute;
    bottom: 0;
    width: 1px;
    height: 5px;
    background: var(--line-strong);
  }

  .tick span {
    position: absolute;
    left: 0;
    bottom: 6px;
    transform: translateX(-50%);
    color: var(--faint);
    font-size: var(--fs-2xs);
    white-space: nowrap;
  }

  .trim {
    position: relative;
    margin: 2px 0;
    border-radius: var(--r-xs);
    background: var(--surface);
  }

  .out-left {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
  }

  /* Nothing cut yet: quiet, as in the editor. Once trimmed, the kept part is marked,
     a line along its top and bottom and the handles closing the sides. */
  .kept {
    position: absolute;
    top: 0;
    bottom: 0;
    transition: background var(--dur) var(--ease-out);
  }

  .kept.trimmed {
    background: var(--accent-soft);
    box-shadow:
      inset 0 2px 0 var(--accent),
      inset 0 -2px 0 var(--accent);
  }

  .handle {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 10px;
    background: var(--surface-3);
    cursor: ew-resize;
    transition: background var(--dur-fast);
  }

  .kept.trimmed .handle,
  .handle:hover {
    background: var(--accent);
  }

  /* A bigger place to catch it than to see it. */
  .handle::before {
    content: "";
    position: absolute;
    inset: 0 -6px;
  }

  .handle::after {
    content: "";
    position: absolute;
    left: 4px;
    top: 50%;
    width: 2px;
    height: 10px;
    margin-top: -5px;
    border-radius: 1px;
    background: var(--muted);
    pointer-events: none;
    transition: background var(--dur-fast);
  }

  .kept.trimmed .handle::after,
  .handle:hover::after {
    background: var(--on-accent);
  }

  .handle.in {
    left: -10px;
    border-radius: var(--r-xs) 0 0 var(--r-xs);
  }

  .handle.out {
    right: -10px;
    border-radius: 0 var(--r-xs) var(--r-xs) 0;
  }

  .keys {
    position: relative;
    margin: 2px 0;
    border-radius: var(--r-xs);
    background: var(--surface);
    transition: background var(--dur-fast);
  }

  .keys.on {
    background: var(--surface-2);
  }

  .path {
    position: absolute;
    top: 50%;
    height: 2px;
    margin-top: -1px;
    background: color-mix(in srgb, var(--accent) 50%, transparent);
  }

  .diamond {
    position: absolute;
    top: 50%;
    width: 11px;
    height: 11px;
    margin: -5.5px 0 0 -5.5px;
    transform: rotate(45deg);
    border-radius: 2px;
    background: var(--text);
    box-shadow: 0 0 0 2px var(--bg);
    cursor: ew-resize;
    transition: background var(--dur-fast);
  }

  .diamond.here {
    background: var(--accent);
  }

  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
    pointer-events: none;
  }

  .playhead::before {
    content: "";
    position: absolute;
    top: 2px;
    left: -4px;
    width: 10px;
    height: 13px;
    border-radius: 3px 3px 5px 5px;
    background: var(--text);
  }

  @media (max-height: 700px) {
    .transport {
      height: 46px;
    }

    .play {
      width: 34px;
      height: 34px;
    }
  }

  @media (max-width: 1040px) {
    .sub {
      display: none;
    }
  }
</style>
