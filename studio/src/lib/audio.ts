// The editor's audio: every track of a clip decoded once, played through Web
// Audio with its own gain (the video element itself stays muted), and
// reduced to waveform peaks for drawing.
import * as api from "./api";

/** Restart the tracks if they drift this far from the video (seconds). */
const MAX_DRIFT = 0.08;
/** Waveform resolution. */
export const BUCKETS_PER_SEC = 100;

/** Per bucket: [peak, rms] of every channel, 0..1. */
export type Peaks = Float32Array;

export function computePeaks(buf: AudioBuffer): Peaks {
  const n = Math.ceil(buf.duration * BUCKETS_PER_SEC);
  const out = new Float32Array(n * 2);
  const size = buf.sampleRate / BUCKETS_PER_SEC;
  const channels = Array.from({ length: buf.numberOfChannels }, (_, c) => buf.getChannelData(c));
  for (let i = 0; i < n; i++) {
    const s0 = Math.floor(i * size);
    const s1 = Math.min(buf.length, Math.floor((i + 1) * size));
    let peak = 0;
    let sum = 0;
    for (const ch of channels) {
      for (let s = s0; s < s1; s++) {
        const v = ch[s];
        const a = v < 0 ? -v : v;
        if (a > peak) peak = a;
        sum += v * v;
      }
    }
    const count = (s1 - s0) * channels.length;
    out[2 * i] = peak;
    out[2 * i + 1] = count > 0 ? Math.sqrt(sum / count) : 0;
  }
  return out;
}

/** What the separate tracks add up to at these levels (an estimate, for drawing). */
export function mixPeaks(tracks: Peaks[], gains: number[]): Peaks {
  const n = Math.max(0, ...tracks.map((t) => t.length));
  const out = new Float32Array(n);
  for (let i = 0; i < n; i += 2) {
    let peak = 0;
    let power = 0;
    tracks.forEach((t, k) => {
      const g = gains[k] ?? 0;
      peak += (t[i] ?? 0) * g;
      power += ((t[i + 1] ?? 0) * g) ** 2;
    });
    out[i] = Math.min(1, peak);
    out[i + 1] = Math.min(1, Math.sqrt(power));
  }
  return out;
}

export class TrackMixer {
  private ctx = new AudioContext({ latencyHint: "interactive" });
  private master = this.ctx.createGain();
  /** How loud you hear it here; never part of an export. */
  private monitor = this.ctx.createGain();
  private buffers: AudioBuffer[] = [];
  private gains: GainNode[] = [];
  private sources: AudioBufferSourceNode[] = [];
  /** Context time and clip time when the tracks last started. */
  private startedCtx = 0;
  private startedClip = 0;
  /** Playback speed; the pitch follows it, like tape. */
  private rate = 1;
  private playing = false;
  /** True when it plays the separate tracks; false when only the Mix track exists. */
  separate = false;
  peaks: Peaks[] = [];

  constructor() {
    this.master.connect(this.monitor);
    this.monitor.connect(this.ctx.destination);
  }

  /** Loads the clip's audio. False if it has none (or it couldn't be read). */
  async load(clipPath: string): Promise<boolean> {
    const files = await api.audioTracks(clipPath);
    if (files.length === 0) return false;
    this.separate = files.length > 1;
    const play = this.separate ? files.slice(1) : files;
    for (const file of play) {
      const data = await api.readCached(file);
      const buffer = await this.ctx.decodeAudioData(data);
      this.buffers.push(buffer);
      this.peaks.push(computePeaks(buffer));
      const gain = this.ctx.createGain();
      gain.connect(this.master);
      this.gains.push(gain);
    }
    return true;
  }

  setVolume(track: number, volume: number) {
    const g = this.gains[track];
    if (g) g.gain.setTargetAtTime(volume, this.ctx.currentTime, 0.015);
  }

  setMaster(volume: number) {
    this.master.gain.setTargetAtTime(volume, this.ctx.currentTime, 0.015);
  }

  setMonitor(volume: number) {
    this.monitor.gain.setTargetAtTime(volume, this.ctx.currentTime, 0.015);
  }

  /** Starts every track at `time` seconds into the clip. */
  play(time: number) {
    this.stopSources();
    if (this.ctx.state === "suspended") this.ctx.resume();
    for (const [i, buffer] of this.buffers.entries()) {
      const src = this.ctx.createBufferSource();
      src.buffer = buffer;
      src.playbackRate.value = this.rate;
      src.connect(this.gains[i]);
      src.start(0, Math.max(0, time));
      this.sources.push(src);
    }
    this.startedCtx = this.ctx.currentTime;
    this.startedClip = time;
    this.playing = true;
  }

  pause() {
    this.stopSources();
    this.playing = false;
  }

  /** Plays faster or slower from where it is now. */
  setRate(rate: number) {
    if (rate === this.rate) return;
    if (this.playing) {
      this.startedClip = this.position();
      this.startedCtx = this.ctx.currentTime;
      for (const s of this.sources) s.playbackRate.value = rate;
    }
    this.rate = rate;
  }

  /** Call regularly while the video plays to correct any drift. */
  follow(videoTime: number) {
    // The allowed drift is in heard time: at half speed, half as much of the clip.
    if (this.playing && Math.abs(this.position() - videoTime) > MAX_DRIFT * this.rate) this.play(videoTime);
  }

  /** Where in the clip the tracks are now. */
  private position() {
    return this.startedClip + (this.ctx.currentTime - this.startedCtx) * this.rate;
  }

  close() {
    this.stopSources();
    this.buffers = [];
    this.ctx.close();
  }

  private stopSources() {
    for (const s of this.sources) {
      try {
        s.stop();
      } catch {
        // Already stopped.
      }
      s.disconnect();
    }
    this.sources = [];
  }
}
