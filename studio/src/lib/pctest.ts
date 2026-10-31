// The PC test: what recording costs this PC. `klipp-recorder --probe` (run
// by setup, or again from Settings) records a few seconds at a few settings
// and measures Klipp's own share of the graphics card, CPU and memory, next to
// how busy the PC was; the window keeps it as pc-test.json. From those runs,
// `predict` estimates any other video settings, and `recommendPreset` picks
// the preset this PC runs with plenty of room.

import type { PresetId, Screen, VideoChoice } from "./presets";

/** What one run of the recording path cost Klipp's own process. */
export interface PcUsage {
  /** How long it was measured. */
  seconds: number;
  /** Klipp's share of the card's 3D engine, video encoder and copy engine, %; null without GPU counters. */
  gpu3d: number | null;
  gpuEncode: number | null;
  gpuCopy: number | null;
  /** The whole card's busiest 3D engine meanwhile (every app), %. */
  card3d: number | null;
  /** CPU: % of the whole machine, and of one core. */
  cpu: number;
  cpuCore: number;
  /** Memory, MB: before it started; private bytes, in RAM (private working set), the buffer's part, private minus the buffer. */
  beforeMb: number;
  privateMb: number;
  workingSetMb: number;
  bufferMb: number;
  baseMb: number;
  /** Graphics memory the recording used, MB. */
  vramMb: number;
  /** The bitrate it made on the desktop, Mbit/s (games make far more). */
  mbps: number;
  /** Frames with something new on screen. */
  newFrames: number;
}

/** One candidate setting the probe recorded. */
export interface PcRun {
  width: number;
  height: number;
  fps: number;
  codec: "av1" | "h264";
  /** "light" with `lightEncoder`: as Performance+ records. */
  capture: "auto" | "light" | "wgc" | "duplication";
  lightEncoder: boolean;
  frames: number;
  late: number;
  dropped: number;
  sendMs: number;
  sendMaxMs: number;
  convertMs: number;
  /** The share of each frame's time the card's work took. */
  busy: number;
  smooth: boolean;
  error: string | null;
  usage: PcUsage | null;
}

/** The PC over a second before the test, every app included. */
export interface PcSnapshot {
  ramTotalMb: number;
  ramUsedMb: number;
  cpuName: string;
  logicalCores: number;
  /** Whole-machine CPU, %. */
  cpu: number;
  gpuName: string | null;
  vramMb: number | null;
  vramUsedMb: number | null;
  /** The whole card's 3D engine and video encoder, %. */
  gpu3d: number | null;
  gpuEncode: number | null;
  /** The test's own memory before it recorded, MB. */
  processMb: number;
}

/** pc-test.json: the probe's whole report (as setup's SystemCheck), with when and by which Klipp. */
export interface PcTest {
  testVersion: 1;
  klippVersion: string;
  /** ISO time, UTC. */
  at: string;
  monitor: { width: number; height: number; refreshHz: number } | null;
  recordingGpu: string | null;
  encoder: string | null;
  ramMb: number;
  performance: PcRun[];
  recommended: { height: number; fps: number } | null;
  system: PcSnapshot;
  /** How long the test took, s. */
  seconds: number;
}

/** What Klipp would use at some video settings, on this PC. */
export interface Prediction {
  /** The recorded size. */
  width: number;
  height: number;
  /** Klipp's share of the graphics card's 3D engine (capture and colour conversion), %. */
  gpu3d: number;
  /** Klipp's share of the video encoder (separate hardware on the card), %. */
  encoder: number;
  /** Klipp's share of the whole CPU, and of one core, %. */
  cpu: number;
  cpuCore: number;
  /** Memory, MB: Klipp itself, the replay buffer (video and audio), both. */
  baseMb: number;
  bufferMb: number;
  ramMb: number;
  /** Graphics memory, MB. */
  vramMb: number;
  /** Estimated bitrate in games, Mbit/s, and what a minute of clips takes, MB. */
  mbps: number;
  mbPerMinute: number;
  /** The share of each frame's time the card's work takes, and whether that passes the probe's smooth rule. */
  busy: number;
  smooth: boolean;
}

/** The probe's smooth rule (probe.rs): nearly nothing late, the card's work at most this share of a frame. */
export const MAX_BUSY = 0.25;
/** Klipp's 3D share (as in a game) counted as small enough for High, %. */
const SMALL_3D = 5;
/** Memory per second of buffered audio, per track, MB. */
const AUDIO_MB_PER_SEC = 0.05;

/** The size these settings record on this screen. */
export function recordedSize(choice: Pick<VideoChoice, "resolution">, screen: Screen, test?: PcTest | null) {
  const target = { native: screen.height, "1440p": 1440, "1080p": 1080, "720p": 720 }[choice.resolution];
  const height = Math.min(target, screen.height);
  const m = test?.monitor;
  const aspect = m && m.height > 0 ? m.width / m.height : 16 / 9;
  return { width: Math.round((height * aspect) / 2) * 2, height };
}

/**
 * The bitrate games come out at, Mbit/s. The desktop the test records says
 * little, so this is a model of real clips: about 28 Mbit/s at 1440p60 AV1,
 * quality 30 (CS2), scaled by size and frame rate, ×1.5 for H.264, doubling
 * every 6 steps of quality, and never above the limit.
 *
 * Checks: 1440p60 AV1 q30, 40 cap → 28 · 1440p120 AV1 q18, 40 cap → 40 (the
 * cap, as real clips at those settings are) · Performance+ (1080p60 AV1 q32, 20 cap)
 * → 14.4 · 1440p60 H.264 q30, 40 cap → 40 (42 uncapped) · 2160p60 AV1 q30,
 * 60 cap → 51.4.
 */
export function gameMbps(c: Pick<VideoChoice, "codec" | "fps" | "quality" | "max_bitrate_mbps">, width: number, height: number): number {
  const pixels = (width * height) / (2560 * 1440);
  const raw = 28 * Math.pow(pixels, 0.75) * Math.pow(c.fps / 60, 0.6) * (c.codec === "h264" ? 1.5 : 1) * Math.pow(2, (30 - c.quality) / 6);
  return Math.min(raw, c.max_bitrate_mbps);
}

type Point = { x: number; y: number };

/**
 * y at x from measured points, linear between them. Outside them: "rate"
 * scales the nearest point (work grows with the pixels per second); "memory"
 * keeps the smallest below and follows the last slope above.
 */
function interpolate(points: Point[], x: number, outside: "rate" | "memory"): number | null {
  const byX = new Map<number, number[]>();
  for (const p of points) if (p.x > 0 && Number.isFinite(p.y)) byX.set(p.x, [...(byX.get(p.x) ?? []), p.y]);
  const pts = [...byX].map(([x, ys]) => ({ x, y: ys.reduce((a, b) => a + b, 0) / ys.length })).sort((a, b) => a.x - b.x);
  if (pts.length === 0) return null;
  const first = pts[0];
  const last = pts[pts.length - 1];
  if (x <= first.x) return outside === "rate" ? (first.y * x) / first.x : first.y;
  if (x >= last.x) {
    if (outside === "rate" || pts.length === 1) return outside === "rate" ? (last.y * x) / last.x : last.y;
    const prev = pts[pts.length - 2];
    const slope = Math.max(0, (last.y - prev.y) / (last.x - prev.x));
    return last.y + slope * (x - last.x);
  }
  const i = pts.findIndex((p) => p.x >= x);
  const [a, b] = [pts[i - 1], pts[i]];
  return a.y + ((b.y - a.y) * (x - a.x)) / (b.x - a.x);
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Runs that recorded and were measured. */
function measured(test: PcTest): PcRun[] {
  return (test.performance ?? []).filter((r) => r.usage && !r.error && r.frames > 0);
}

/**
 * What a frame that repeats the one before costs, against a new one: the
 * capture and colour conversion only run for new pictures, the encoder for
 * every frame. Measured on the test PC (1440p120: 1.5 % 3D with a quarter of
 * the frames new, 3.9 % with all of them).
 */
const REPEAT_COST = { d3: 0.18, cpu: 0.5 };

/**
 * A run's usage as in a game, where every frame is new: a quiet desktop
 * repeats most frames, which costs the card's 3D engine and the CPU less.
 */
function inGame(r: PcRun): PcUsage {
  const u = r.usage!;
  const fresh = clamp(r.frames > 0 ? u.newFrames / r.frames : 1, 0.05, 1);
  const d3 = REPEAT_COST.d3 + (1 - REPEAT_COST.d3) * fresh;
  const cpu = REPEAT_COST.cpu + (1 - REPEAT_COST.cpu) * fresh;
  return { ...u, gpu3d: u.gpu3d == null ? null : u.gpu3d / d3, cpu: u.cpu / cpu, cpuCore: u.cpuCore / cpu };
}

/**
 * What the light capture and the fast encoder save on this PC, as factors:
 * the Performance+ run against the usual one at its size and rate.
 */
function lightFactors(test: PcTest, usual: PcRun[]) {
  const plus = measured(test).find((r) => r.capture === "light");
  const fallback = { d3: 0.7, encoder: 0.65, cpu: 1 };
  if (!plus?.usage) return fallback;
  const light = inGame(plus);
  const rate = plus.width * plus.height * plus.fps;
  const same = usual.find((r) => r.height === plus.height && r.fps === plus.fps && r.codec === plus.codec);
  const at = (f: (u: PcUsage) => number | null) =>
    same ? f(inGame(same)) : interpolate(usual.map((r) => ({ x: r.width * r.height * r.fps, y: f(inGame(r)) ?? NaN })), rate, "rate");
  const ratio = (value: number | null, ref: number | null, otherwise: number, lo: number, hi: number) =>
    value != null && ref != null && ref > 0.05 ? clamp(value / ref, lo, hi) : otherwise;
  return {
    d3: ratio(light.gpu3d, at((u) => u.gpu3d), fallback.d3, 0.2, 1.2),
    encoder: ratio(light.gpuEncode, at((u) => u.gpuEncode), fallback.encoder, 0.2, 1.2),
    cpu: ratio(light.cpu, at((u) => u.cpu), fallback.cpu, 0.3, 1.5),
  };
}

/**
 * What Klipp would use recording `choice` with a `lengthSecs` buffer on this
 * PC while a game runs: interpolated by pixels per second between the
 * settings the test measured (memory by the size of a frame), every frame
 * new as in a game, with the light capture and the fast encoder as the
 * Performance+ run measured them. The buffer uses the game bitrate model,
 * not the desktop's.
 */
export function predict(test: PcTest, choice: VideoChoice, lengthSecs: number, screen: Screen, audioTracks = 3): Prediction {
  const { width, height } = recordedSize(choice, screen, test);
  const rate = width * height * choice.fps;
  const runs = measured(test);
  const usualRuns = runs.filter((r) => r.capture !== "light");
  const usual = usualRuns.length > 0 ? usualRuns : runs;
  const byRate = (f: (u: PcUsage, r: PcRun) => number | null) =>
    interpolate(usual.map((r) => ({ x: r.width * r.height * r.fps, y: f(inGame(r), r) ?? NaN })), rate, "rate") ?? 0;
  const bySize = (f: (u: PcUsage) => number) => interpolate(usual.map((r) => ({ x: r.width * r.height, y: f(r.usage!) })), width * height, "memory") ?? 0;

  const light = choice.capture === "light" || choice.performance === "plus";
  const fast = choice.fast_encoder || choice.performance === "plus";
  const f = usualRuns.length > 0 ? lightFactors(test, usualRuns) : { d3: 1, encoder: 1, cpu: 1 };
  const gpu3d = byRate((u) => u.gpu3d) * (light ? f.d3 : 1);
  const encoder = byRate((u) => u.gpuEncode) * (fast ? f.encoder : 1);
  const cpuScale = light ? f.cpu : 1;
  const cpu = byRate((u) => u.cpu) * cpuScale;
  const cpuCore = byRate((u) => u.cpuCore) * cpuScale;
  const busy = byRate((_, r) => r.busy);

  // Klipp without its buffer: what it holds in RAM, less the test's small buffer.
  const baseMb = bySize((u) => Math.max(0, u.workingSetMb - u.bufferMb));
  const vramMb = bySize((u) => u.vramMb);
  const mbps = gameMbps(choice, width, height);
  const perSecMb = mbps / 8 + audioTracks * AUDIO_MB_PER_SEC;
  const bufferMb = lengthSecs * perSecMb;

  // A setting the test recorded as is keeps its own verdict.
  const exact = runs.find((r) => r.height === height && r.fps === choice.fps && (r.capture === "light") === light);
  return {
    width,
    height,
    gpu3d: clamp(gpu3d, 0, 100),
    encoder: clamp(encoder, 0, 100),
    cpu: clamp(cpu, 0, 100),
    cpuCore: Math.max(0, cpuCore),
    baseMb,
    bufferMb,
    ramMb: baseMb + bufferMb,
    vramMb,
    mbps,
    mbPerMinute: 60 * perSecMb,
    busy,
    smooth: exact ? exact.smooth : busy <= MAX_BUSY,
  };
}

/**
 * The best preset this PC records smoothly with plenty of room left for a
 * game, by the probe's smooth rule: High if the screen's size at 120 fps (60
 * on a slower screen) was smooth with at most half the frame time busy and
 * Klipp's 3D share small (and the PC has the memory for a 120 fps buffer);
 * else Normal if the screen's size (1440p on bigger screens) at 60 was
 * smooth; else Performance if 1080p60 was; else Performance+.
 */
export function recommendPreset(test: PcTest | null, screen: Screen): PresetId {
  if (!test) return "normal";
  const usual = measured(test).filter((r) => r.capture !== "light");
  if (usual.length === 0) return "plus";
  // The probe's own candidates: the screen's size (or 1440p) and 1080p.
  const top = Math.max(...usual.map((r) => r.height));
  const low = Math.min(top, 1080);
  const run = (height: number, fps: number) => usual.find((r) => r.height === height && r.fps === fps);
  const highFps = screen.refreshHz >= 120 ? 120 : 60;
  // High records the screen as it is: a bigger screen than the test's top size costs more.
  const scale = screen.height > top ? (screen.height / top) ** 2 : 1;
  const high = run(top, highFps);
  const roomy = !!high && high.smooth && high.busy * scale <= MAX_BUSY / 2 && (inGame(high).gpu3d ?? 0) * scale <= SMALL_3D;
  const memory = test.system?.ramTotalMb || test.ramMb || 0;
  if (roomy && (highFps === 60 || memory >= 12000)) return "high";
  if (run(top, 60)?.smooth) return "normal";
  if (run(low, 60)?.smooth) return "performance";
  return "plus";
}

/** "540 MB", "1.1 GB", "26 GB" (MB as Windows counts them). */
export function memory(mb: number): string {
  if (mb < 1000) return `${Math.max(1, Math.round(mb))} MB`;
  const gb = mb / 1024;
  return `${gb < 10 ? gb.toFixed(1) : Math.round(gb)} GB`;
}

/** The RAM a PC has as sold (32 GB), from what Windows can use (31.2 GB, the rest kept for graphics). */
export function installedGb(totalMb: number): number {
  const gb = totalMb / 1024;
  const sizes = [2, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256];
  return sizes.find((s) => s >= gb && s - gb <= 1.5) ?? Math.round(gb);
}
