<script lang="ts" module>
  export interface Lane {
    name: string;
    peaks: import("./audio").Peaks | null;
    /** Drawn at this level. */
    gain: number;
    volume: number;
    muted: boolean;
    solo: boolean;
    /** Silent because it's muted or another track is soloed. */
    off: boolean;
    /** The Mix row: the master level, no solo. */
    mix: boolean;
  }
</script>

<script lang="ts">
  import type { ClipEvent } from "./api";
  import { eventName, timecode } from "./format";
  import Icon from "./Icon.svelte";
  import { isHot, mergeMarkers } from "./markers";
  import Slider from "./ui/Slider.svelte";
  import Waveform from "./Waveform.svelte";

  type Drag = "seek" | "in" | "out";

  interface Props {
    duration: number;
    current: number;
    inPoint: number;
    outPoint: number;
    events: ClipEvent[];
    lanes: Lane[];
    onseek: (t: number) => void;
    onrange: (inPoint: number, outPoint: number) => void;
    /** A drag started or ended (to pause playback while scrubbing). */
    ondrag: (what: Drag, phase: "start" | "end") => void;
    onmarker: (t: number) => void;
    /** Lane -1 is the Mix row. */
    onvolume: (lane: number, v: number) => void;
    onmute: (lane: number) => void;
    onsolo: (lane: number) => void;
  }
  let { duration, current, inPoint, outPoint, events, lanes, onseek, onrange, ondrag, onmarker, onvolume, onmute, onsolo }: Props = $props();

  const HEAD_W = 262;
  const SNAP_PX = 7;
  const MIN_LEN = 0.5;

  let laneW = $state(0);
  let lanesEl: HTMLDivElement;
  let dragging = $state<Drag | null>(null);
  let hoverT = $state<number | null>(null);
  let snapped = $state<number | null>(null);

  const x = (t: number) => (Math.min(Math.max(t, 0), duration) / Math.max(duration, 0.001)) * laneW;

  let markers = $derived(mergeMarkers(events));

  // Ruler: labeled ticks about every 90 px, with minor ticks between.
  let ticks = $derived.by(() => {
    if (laneW <= 0 || duration <= 0) return { major: [] as number[], minor: [] as number[] };
    const steps = [0.5, 1, 2, 5, 10, 15, 30, 60, 120];
    const step = steps.find((s) => (s / duration) * laneW >= 90) ?? 300;
    const major: number[] = [];
    const minor: number[] = [];
    for (let t = 0; t <= duration + 1e-6; t += step / 4) {
      const n = Math.round(t / (step / 4));
      (n % 4 === 0 ? major : minor).push(t);
    }
    return { major, minor };
  });

  function tickLabel(t: number) {
    return duration >= 60 ? `${Math.floor(t / 60)}:${String(Math.round(t % 60)).padStart(2, "0")}` : `${+t.toFixed(1)}s`;
  }

  function timeAt(e: PointerEvent) {
    const r = lanesEl.getBoundingClientRect();
    return Math.min(Math.max((e.clientX - r.left - HEAD_W) / Math.max(laneW, 1), 0), 1) * duration;
  }

  /** Snaps to markers, the ends and the trim points (and the playhead for handles), unless Alt is held. */
  function snap(t: number, e: PointerEvent, what: Drag): number {
    snapped = null;
    if (e.altKey) return t;
    const targets = [0, duration, ...markers.map((m) => m.at)];
    if (what === "seek") targets.push(inPoint, outPoint);
    else targets.push(current);
    let best = t;
    let dist = SNAP_PX;
    for (const target of targets) {
      const d = Math.abs(x(target) - x(t));
      if (d < dist) {
        dist = d;
        best = target;
      }
    }
    if (best !== t) snapped = best;
    return best;
  }

  function down(e: PointerEvent, what: Drag) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    dragging = what;
    lanesEl.setPointerCapture(e.pointerId);
    ondrag(what, "start");
    move(e);
  }

  function laneDown(e: PointerEvent) {
    const r = lanesEl.getBoundingClientRect();
    if (e.clientX - r.left < HEAD_W) return;
    if ((e.target as HTMLElement).closest("button, input")) return;
    down(e, "seek");
  }

  function move(e: PointerEvent) {
    const r = lanesEl.getBoundingClientRect();
    const inside = e.clientX - r.left >= HEAD_W;
    hoverT = inside || dragging ? timeAt(e) : null;
    if (!dragging) return;
    const t = snap(timeAt(e), e, dragging);
    if (dragging === "in") onrange(Math.min(t, outPoint - MIN_LEN), outPoint);
    else if (dragging === "out") onrange(inPoint, Math.max(t, inPoint + MIN_LEN));
    else onseek(t);
  }

  function up(e: PointerEvent) {
    if (!dragging) return;
    const what = dragging;
    dragging = null;
    snapped = null;
    if (lanesEl.hasPointerCapture(e.pointerId)) lanesEl.releasePointerCapture(e.pointerId);
    ondrag(what, "end");
  }
</script>

<div
  class="timeline"
  class:dragging
  bind:this={lanesEl}
  style="--head: {HEAD_W}px"
  onpointerdown={laneDown}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onpointerleave={() => !dragging && (hoverT = null)}
  role="presentation"
>
  <div class="row ruler">
    <div class="head"></div>
    <div class="lane" bind:clientWidth={laneW}>
      {#each ticks.minor as t (t)}<span class="tick minor" style="left: {x(t)}px"></span>{/each}
      {#each ticks.major as t (t)}
        <span class="tick major" style="left: {x(t)}px"></span>
        <span class="tick-label mono" style="left: {x(t)}px">{tickLabel(t)}</span>
      {/each}
    </div>
  </div>

  <div class="row events">
    <div class="head"><span class="label">Events</span></div>
    <div class="lane">
      {#each markers as m (m.at)}
        <button
          class="marker"
          class:hot={isHot(m.kind)}
          style="left: {x(m.at)}px"
          title="{timecode(m.at)} · {m.all.join(', ')}"
          onpointerdown={(e) => e.stopPropagation()}
          onclick={() => onmarker(m.at)}
        >
          <Icon event={m.kind} size={18} />
          {#if isHot(m.kind)}<span class="mk mono">{eventName(m.kind)}</span>{/if}
        </button>
      {/each}
      {#if markers.length === 0}<span class="none">No game events in this clip</span>{/if}
    </div>
  </div>

  <div class="row clip">
    <div class="head"><span class="label">Clip</span><span class="len mono">{(outPoint - inPoint).toFixed(1)} s</span></div>
    <div class="lane">
      <div class="bar"></div>
      <div class="range" class:full={inPoint <= 0.01 && outPoint >= duration - 0.01} style="left: {x(inPoint)}px; width: {x(outPoint) - x(inPoint)}px">
        {#if x(outPoint) - x(inPoint) > 70}<span class="range-len mono">{timecode(inPoint)} – {timecode(outPoint)}</span>{/if}
      </div>
      <button class="handle in" class:active={dragging === "in"} style="left: {x(inPoint)}px" aria-label="Clip start" onpointerdown={(e) => down(e, "in")}></button>
      <button class="handle out" class:active={dragging === "out"} style="left: {x(outPoint)}px" aria-label="Clip end" onpointerdown={(e) => down(e, "out")}></button>
    </div>
  </div>

  {#each lanes as lane, i (i)}
    {@const k = lane.mix ? -1 : i - 1}
    <div class="row audio" class:mix={lane.mix} class:off={lane.off}>
      <div class="head">
        <span class="name">{lane.name}</span>
        <button class="tog" class:on={lane.muted} title="Mute" onclick={() => onmute(k)}>M</button>
        {#if !lane.mix}<button class="tog solo" class:on={lane.solo} title="Solo" onclick={() => onsolo(k)}>S</button>{:else}<span class="tog-space"></span>{/if}
        <Slider min={0} max={2} step={0.05} value={lane.volume} label="{lane.name} volume" oninput={(v) => onvolume(k, v)} />
        <span class="pct mono">{Math.round(lane.volume * 100)}</span>
      </div>
      <div class="lane"><Waveform peaks={lane.peaks} {duration} gain={lane.gain} off={lane.off} accent={lane.mix} /></div>
    </div>
  {/each}

  <!-- Over every lane: what's cut away, the playhead, and where the pointer is. -->
  <div class="overlay">
    <div class="cut" style="left: 0; width: {x(inPoint)}px"></div>
    <div class="cut" style="left: {x(outPoint)}px; right: 0"></div>
    {#if hoverT != null && !dragging}
      <div class="hover" style="left: {x(hoverT)}px"><span class="hover-time mono">{timecode(hoverT)}</span></div>
    {/if}
    {#if snapped != null}<div class="snap" style="left: {x(snapped)}px"></div>{/if}
    <div class="playhead" style="left: {x(current)}px"><span class="cap"></span></div>
  </div>
</div>

<style>
  .timeline {
    position: relative;
    display: flex;
    flex-direction: column;
    padding: 0 20px 14px 0;
    touch-action: none;
  }

  .row {
    display: grid;
    grid-template-columns: var(--head) 1fr;
    align-items: stretch;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px 0 20px;
    min-width: 0;
  }

  .lane {
    position: relative;
    min-width: 0;
    cursor: text;
  }

  .ruler {
    height: 26px;
  }

  .tick {
    position: absolute;
    bottom: 0;
    width: 1px;
    background: var(--line-strong);
  }

  .tick.minor {
    height: 4px;
  }

  .tick.major {
    height: 8px;
    background: var(--muted);
  }

  .tick-label {
    position: absolute;
    top: 3px;
    transform: translateX(-50%);
    font-size: 10px;
    color: var(--muted);
    white-space: nowrap;
  }

  .events {
    height: 34px;
  }

  .marker {
    position: absolute;
    top: 4px;
    transform: translateX(-13px);
    gap: 4px;
    height: 26px;
    padding: 0 4px;
    border: none;
    border-radius: 7px;
    background: var(--surface);
    z-index: 2;
  }

  .marker:hover {
    background: var(--surface-3);
  }

  .marker.hot {
    background: var(--accent);
    color: var(--on-accent);
  }

  .marker.hot :global(.icon) {
    --accent: var(--on-accent);
  }

  .mk {
    font-size: 11px;
    font-weight: 700;
    padding-right: 2px;
  }

  .none {
    position: absolute;
    top: 9px;
    left: 2px;
    font-size: 12px;
    color: var(--faint);
  }

  .clip {
    height: 40px;
    margin-bottom: 6px;
  }

  .len {
    margin-left: auto;
    font-size: 11px;
    color: var(--muted);
  }

  .bar {
    position: absolute;
    inset: 6px 0;
    border-radius: 8px;
    background: repeating-linear-gradient(90deg, var(--surface) 0 6px, var(--surface-2) 6px 7px);
  }

  .range {
    position: absolute;
    top: 6px;
    bottom: 6px;
    border-radius: 8px;
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 2px var(--accent);
    display: grid;
    place-items: center;
    pointer-events: none;
  }

  /* Untrimmed: calm until a handle moves. */
  .range.full {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--line-strong);
  }

  .range-len {
    font-size: 10px;
    color: var(--text);
    opacity: 0.8;
    white-space: nowrap;
  }

  .handle {
    position: absolute;
    top: 2px;
    bottom: 2px;
    width: 12px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: var(--accent);
    cursor: ew-resize;
    z-index: 3;
    transition:
      transform 0.12s,
      box-shadow 0.12s;
  }

  .handle.in {
    transform: translateX(-10px);
  }

  .handle.out {
    transform: translateX(-2px);
  }

  .handle::after {
    content: "";
    position: absolute;
    left: 5px;
    top: 30%;
    bottom: 30%;
    width: 2px;
    border-radius: 1px;
    background: var(--on-accent);
    opacity: 0.6;
  }

  .handle:hover,
  .handle.active {
    box-shadow: 0 0 0 4px var(--accent-soft);
  }

  .audio {
    height: 48px;
    border-top: 1px solid var(--line);
  }

  .audio .head {
    gap: 6px;
  }

  .audio.off .lane {
    opacity: 0.55;
  }

  .name {
    width: 46px;
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mix .name {
    color: var(--accent);
  }

  .tog {
    width: 22px;
    height: 22px;
    padding: 0;
    border-radius: 6px;
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 700;
    color: var(--muted);
    background: var(--surface);
  }

  .tog.on {
    background: var(--danger);
    border-color: var(--danger);
    color: var(--on-accent);
  }

  .tog.solo.on {
    background: #f2c84b;
    border-color: #f2c84b;
  }

  .tog-space {
    width: 22px;
  }

  .head :global(.slider) {
    flex: 1;
    min-width: 0;
  }

  .pct {
    width: 28px;
    text-align: right;
    font-size: 11px;
    color: var(--muted);
  }

  .overlay {
    position: absolute;
    top: 0;
    bottom: 14px;
    left: var(--head);
    right: 20px;
    pointer-events: none;
  }

  .cut {
    position: absolute;
    top: 26px;
    bottom: 0;
    background: rgba(18, 18, 17, 0.6);
  }

  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
    box-shadow: 0 0 8px rgba(0, 0, 0, 0.6);
    z-index: 4;
  }

  .cap {
    position: absolute;
    top: 0;
    left: -5px;
    width: 12px;
    height: 12px;
    border-radius: 3px 3px 6px 6px;
    background: var(--text);
  }

  .hover {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--line-strong);
  }

  .hover-time {
    position: absolute;
    top: 2px;
    left: 6px;
    padding: 1px 5px;
    border-radius: 4px;
    font-size: 10px;
    background: var(--surface-3);
    color: var(--text);
    white-space: nowrap;
  }

  .snap {
    position: absolute;
    top: 26px;
    bottom: 0;
    width: 1px;
    background: var(--accent);
    box-shadow: 0 0 6px var(--accent);
  }

  .dragging,
  .dragging .lane {
    cursor: grabbing;
  }
</style>
