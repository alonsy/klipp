<script lang="ts">
  import { BUCKETS_PER_SEC, type Peaks } from "./audio";

  interface Props {
    peaks: Peaks | null;
    /** Seconds the full width stands for. */
    duration: number;
    /** Drawn louder or quieter, like it will sound. */
    gain?: number;
    off?: boolean;
    accent?: boolean;
    /** Peaks are on their way. False when none will come. */
    waiting?: boolean;
  }
  let { peaks, duration, gain = 1, off = false, accent = false, waiting = true }: Props = $props();

  /** Bars, not a solid shape: 2 px wide, 1 px apart. */
  const BAR = 2;
  const STEP = 3;

  let canvas: HTMLCanvasElement;
  let width = $state(0);
  let height = $state(0);

  $effect(() => {
    draw(peaks, duration, gain, off, accent, width, height);
  });

  function draw(peaks: Peaks | null, duration: number, gain: number, off: boolean, accent: boolean, w: number, h: number) {
    if (!canvas || w === 0 || h === 0) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const g = canvas.getContext("2d")!;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    const style = getComputedStyle(canvas);
    const base = accent ? style.getPropertyValue("--text") : style.getPropertyValue("--muted");
    const mid = h / 2;
    // Center line, so silence still shows the track is there.
    g.fillStyle = style.getPropertyValue("--line");
    g.fillRect(0, mid - 0.5, w, 1);
    if (!peaks || duration <= 0) return;
    const buckets = peaks.length / 2;
    const perPx = (duration * BUCKETS_PER_SEC) / w;
    const peakPath = new Path2D();
    const rmsPath = new Path2D();
    const max = mid - 3;
    for (let x = 0; x < w; x += STEP) {
      const b0 = Math.floor(x * perPx);
      const b1 = Math.min(buckets, Math.max(b0 + 1, Math.floor((x + STEP) * perPx)));
      let peak = 0;
      let rms = 0;
      for (let b = b0; b < b1; b++) {
        if (peaks[2 * b] > peak) peak = peaks[2 * b];
        if (peaks[2 * b + 1] > rms) rms = peaks[2 * b + 1];
      }
      // A gentle curve so quiet voices are still visible next to gunfire.
      const ph = Math.min(1, Math.sqrt(peak * gain)) * max;
      const rh = Math.min(1, Math.sqrt(rms * gain)) * max;
      if (ph > 0.5) peakPath.roundRect(x, mid - ph, BAR, ph * 2, 1);
      if (rh > 0.5) rmsPath.roundRect(x, mid - rh, BAR, rh * 2, 1);
    }
    g.globalAlpha = off ? 0.1 : 0.32;
    g.fillStyle = base;
    g.fill(peakPath);
    g.globalAlpha = off ? 0.16 : 0.85;
    g.fill(rmsPath);
  }
</script>

<div class="wave" bind:clientWidth={width} bind:clientHeight={height}>
  <canvas bind:this={canvas} class:ready={peaks || !waiting} style="width: {width}px; height: {height}px"></canvas>
  {#if !peaks && waiting}<div class="loading skeleton"></div>{/if}
</div>

<style>
  .wave {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  canvas {
    position: absolute;
    inset: 0;
    opacity: 0;
    transition: opacity var(--dur-slow) var(--ease-out);
  }

  canvas.ready {
    opacity: 1;
  }

  .loading {
    position: absolute;
    inset: 35% 0;
    border-radius: var(--r-xs);
  }
</style>
