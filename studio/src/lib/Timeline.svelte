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
  import { onMount } from "svelte";
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
    /** The video, so the playhead follows it every frame without re-rendering anything. */
    media?: () => HTMLVideoElement | undefined;
    /** No waveform will come for this clip. */
    noAudio?: boolean;
    /** The events row's name opens the list of what's in the clip. */
    onevents?: () => void;
    eventsOpen?: boolean;
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
  let { duration, current, inPoint, outPoint, events, lanes, media, noAudio = false, onevents, eventsOpen = false, onseek, onrange, ondrag, onmarker, onvolume, onmute, onsolo }: Props = $props();

  const HEAD_W = 148;
  const SNAP_PX = 7;
  const MIN_LEN = 0.5;

  let laneW = $state(0);
  let lanesEl: HTMLDivElement;
  let playhead: HTMLDivElement;
  let dragging = $state<Drag | null>(null);
  let hoverT = $state<number | null>(null);
  let snapped = $state<number | null>(null);

  const x = (t: number) => (Math.min(Math.max(t, 0), duration) / Math.max(duration, 0.001)) * laneW;

  let markers = $derived(mergeMarkers(events));
  let trimmed = $derived(inPoint > 0.01 || outPoint < duration - 0.01);
  /** The time beside the pointer: what's being dragged, or what's under it. */
  let tipT = $derived(dragging === "in" ? inPoint : dragging === "out" ? outPoint : dragging === "seek" ? current : hoverT);

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

  // The playhead runs on its own frame loop and only sets a transform. The video's
  // clock moves in small uneven steps, so while playing it runs on real time and
  // leans towards the video, and jumps when they really differ (a seek, a loop).
  onMount(() => {
    let frame = 0;
    let shown = 0;
    let last = performance.now();
    let drawn = NaN;
    const loop = (now: number) => {
      const video = media?.();
      const t = video ? video.currentTime : current;
      if (video && !video.paused) {
        shown += ((now - last) / 1000) * video.playbackRate;
        shown += (t - shown) * 0.12;
        if (Math.abs(t - shown) > 0.15) shown = t;
      } else {
        shown = t;
      }
      last = now;
      const px = x(shown);
      if (px !== drawn) {
        drawn = px;
        playhead.style.transform = `translate3d(${px}px, 0, 0)`;
      }
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(frame);
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

  {#if markers.length > 0}
    <div class="row events">
      <div class="head">
        <button class="ghost list" class:on={eventsOpen} aria-expanded={eventsOpen} title="In this clip" onclick={() => onevents?.()}>
          Events<span class="mono">{markers.length}</span>
        </button>
      </div>
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
            <Icon event={m.kind} size={16} />
            {#if isHot(m.kind)}<span class="mk mono">{eventName(m.kind)}</span>{/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}

  {#each lanes as lane, i (i)}
    {@const k = lane.mix ? -1 : i - 1}
    <div class="row audio" class:mix={lane.mix} class:off={lane.off}>
      <div class="head">
        <div class="line">
          <span class="name" title={lane.name}>{lane.name}</span>
          <span class="pct mono" class:changed={lane.volume !== 1}>{Math.round(lane.volume * 100)}</span>
          <button class="tog" class:on={lane.muted} aria-pressed={lane.muted} aria-label="Mute {lane.name}" title="Mute" onclick={() => onmute(k)}>M</button>
          {#if !lane.mix}<button class="tog solo" class:on={lane.solo} aria-pressed={lane.solo} aria-label="Solo {lane.name}" title="Solo" onclick={() => onsolo(k)}>S</button>{:else}<span class="tog-space"></span>{/if}
        </div>
        <Slider min={0} max={2} step={0.05} value={lane.volume} label="{lane.name} volume" oninput={(v) => onvolume(k, v)} />
      </div>
      <div class="lane">
        <Waveform peaks={lane.peaks} {duration} gain={lane.gain} off={lane.off} accent={lane.mix} waiting={!noAudio} />
        {#if noAudio && lane.mix}<span class="none">No waveform</span>{/if}
      </div>
    </div>
  {/each}

  <!-- Over every lane: where the moments are, what's cut away, the handles, the playhead, and where the pointer is. -->
  <div class="overlay" class:trimmed class:held={dragging === "in" || dragging === "out"} class:bare={markers.length === 0}>
    {#each markers as m (m.at)}<span class="stem" class:hot={isHot(m.kind)} style="left: {x(m.at)}px"></span>{/each}
    <div class="cut before" style="transform: scaleX({x(inPoint) / Math.max(laneW, 1)})"></div>
    <div class="cut after" style="transform: scaleX({1 - x(outPoint) / Math.max(laneW, 1)})"></div>
    <div class="frame" style="transform: translateX({x(inPoint)}px) scaleX({(x(outPoint) - x(inPoint)) / Math.max(laneW, 1)})"></div>
    {#if hoverT != null && !dragging}<div class="hover" style="left: {x(hoverT)}px"></div>{/if}
    {#if snapped != null}<div class="snap" style="left: {x(snapped)}px"></div>{/if}
    <button
      class="handle in"
      class:active={dragging === "in"}
      class:snapped={dragging === "in" && snapped != null}
      style="transform: translateX({x(inPoint)}px)"
      aria-label="Clip start"
      onpointerdown={(e) => down(e, "in")}
    ><span class="grip"></span></button>
    <button
      class="handle out"
      class:active={dragging === "out"}
      class:snapped={dragging === "out" && snapped != null}
      style="transform: translateX({x(outPoint)}px)"
      aria-label="Clip end"
      onpointerdown={(e) => down(e, "out")}
    ><span class="grip"></span></button>
    <div class="playhead" bind:this={playhead}><span class="cap"></span></div>
    {#if tipT != null}<span class="tip mono" style="left: {x(tipT)}px">{timecode(tipT)}</span>{/if}
  </div>
</div>

<style>
  .timeline {
    --ruler: 20px;
    --events: 30px;
    --lane-h: 36px;
    --right: 18px;
    --tail: 12px;
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 var(--right) var(--tail) 0;
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
    gap: var(--sp-1);
    padding: 0 18px 0 16px;
    min-width: 0;
  }

  .lane {
    position: relative;
    min-width: 0;
    cursor: pointer;
  }

  .ruler {
    height: var(--ruler);
  }

  .tick {
    position: absolute;
    bottom: 0;
    width: 1px;
    background: var(--line);
  }

  .tick.minor {
    height: 3px;
  }

  .tick.major {
    height: 6px;
    background: var(--line-strong);
  }

  .tick-label {
    position: absolute;
    top: 1px;
    transform: translateX(-50%);
    font-size: var(--fs-2xs);
    color: var(--faint);
    white-space: nowrap;
    user-select: none;
  }

  .events {
    height: var(--events);
  }

  .list {
    justify-content: flex-start;
    gap: 6px;
    width: 100%;
    height: 26px;
    padding: 0 var(--sp-2);
    margin-left: calc(-1 * var(--sp-2));
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .list span {
    color: var(--faint);
    font-size: var(--fs-xs);
  }

  .list:hover,
  .list.on {
    color: var(--text);
  }

  .list.on {
    background: var(--hover);
  }

  /* A round chip on the ruler's line; the big moments carry their name in the accent. */
  .marker {
    position: absolute;
    top: 3px;
    translate: -50% 0;
    gap: 3px;
    min-width: 24px;
    height: 24px;
    padding: 0 var(--sp-1);
    border-radius: var(--r-full);
    background: var(--surface-2);
    z-index: 2;
  }

  .marker:hover {
    background: var(--surface-4);
    z-index: 3;
  }

  .marker:active {
    transform: scale(0.94);
  }

  .marker.hot {
    padding: 0 var(--sp-2) 0 var(--sp-1);
    background: var(--accent);
    color: var(--on-accent);
    z-index: 3;
  }

  .marker.hot:hover {
    background: color-mix(in srgb, var(--accent) 88%, white);
  }

  .marker.hot :global(.icon) {
    --accent: var(--on-accent);
  }

  .mk {
    font-size: var(--fs-xs);
    font-weight: 700;
  }

  .audio {
    height: var(--lane-h);
  }

  .audio .lane {
    overflow: hidden;
    border-radius: var(--r-xs);
    background: var(--sunken);
  }

  /* The name, and under it the level as a thin line that belongs to the wave beside it. */
  .audio .head {
    flex-direction: column;
    align-items: stretch;
    justify-content: center;
    gap: 0;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 20px;
  }

  .name {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-sm);
    font-weight: 550;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color var(--dur-fast);
  }

  .off .name {
    color: var(--faint);
  }

  .audio .head :global(.slider) {
    display: block;
    width: 100%;
    height: 12px;
    margin: 0;
  }

  .audio .head :global(.slider::-webkit-slider-runnable-track) {
    height: 2px;
    background: linear-gradient(to right, var(--faint) var(--fill), var(--surface-2) var(--fill));
  }

  .audio .head :global(.slider::-webkit-slider-thumb) {
    width: 10px;
    height: 10px;
    margin-top: -4px;
    background: var(--muted);
    transform: scale(0.8);
  }

  .audio:hover .head :global(.slider::-webkit-slider-thumb),
  .audio .head :global(.slider:focus-visible::-webkit-slider-thumb) {
    background: var(--text);
    transform: scale(1.1);
  }

  .pct {
    font-size: var(--fs-2xs);
    color: var(--faint);
    opacity: 0;
    transition: opacity var(--dur-fast);
  }

  .pct.changed,
  .audio:hover .pct,
  .audio:focus-within .pct {
    opacity: 1;
  }

  .tog {
    width: 20px;
    height: 20px;
    padding: 0;
    border-radius: var(--r-xs);
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    font-weight: 700;
    color: var(--faint);
    background: transparent;
    opacity: 0.55;
  }

  .audio:hover .tog,
  .audio:focus-within .tog {
    opacity: 1;
  }

  .tog:hover {
    background: var(--hover);
    color: var(--text);
  }

  .tog.on {
    opacity: 1;
    background: var(--danger-soft);
    color: var(--danger);
  }

  .tog.solo.on {
    background: var(--warn-soft);
    color: var(--warn);
  }

  .tog-space {
    width: 20px;
  }

  .none {
    position: absolute;
    top: 50%;
    left: var(--sp-3);
    translate: 0 -50%;
    padding-right: var(--sp-2);
    font-size: var(--fs-sm);
    color: var(--faint);
    background: var(--sunken);
  }

  .overlay {
    --top: calc(var(--ruler) + var(--events) + 2px);
    position: absolute;
    top: 0;
    bottom: var(--tail);
    left: var(--head);
    right: var(--right);
    pointer-events: none;
  }

  .overlay.bare {
    --top: calc(var(--ruler) + 2px);
  }

  .stem {
    position: absolute;
    top: var(--top);
    bottom: 0;
    width: 1px;
    background: var(--line);
  }

  .stem.hot {
    background: color-mix(in srgb, var(--accent) 35%, transparent);
  }

  /* Each cut is the whole lane, scaled from its own end: only a transform moves. */
  .cut {
    position: absolute;
    top: var(--ruler);
    bottom: 0;
    left: 0;
    right: 0;
    background: color-mix(in srgb, var(--bg) 72%, transparent);
    transition: transform var(--dur-spring) var(--spring);
  }

  .cut.before {
    transform-origin: left;
  }

  .cut.after {
    transform-origin: right;
  }

  /* The kept part: a line along its top and bottom, the handles closing the sides. */
  .frame {
    position: absolute;
    top: var(--top);
    bottom: 0;
    left: 0;
    width: 100%;
    border-block: 2px solid var(--accent);
    transform-origin: left;
    opacity: 0;
    transition:
      transform var(--dur-spring) var(--spring),
      opacity var(--dur) var(--ease-out);
  }

  .trimmed .frame {
    opacity: 1;
  }

  .handle {
    position: absolute;
    top: var(--top);
    bottom: 0;
    width: 22px;
    padding: 0;
    border-radius: 0;
    background: transparent;
    cursor: ew-resize;
    pointer-events: auto;
    z-index: 3;
    transition: transform var(--dur-spring) var(--spring);
  }

  .handle:hover {
    background: transparent;
  }

  /* The grip sits outside the kept part; the area you can catch reaches a little in. */
  .handle.in {
    left: -16px;
  }

  .handle.out {
    left: -6px;
  }

  /* Dragged: it stays exactly under the pointer. */
  .held .handle,
  .held .cut,
  .held .frame {
    transition: none;
  }

  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 10px;
    display: grid;
    place-items: center;
    background: var(--surface-3);
    transition:
      background var(--dur-fast),
      transform var(--dur-spring) var(--spring);
  }

  .in .grip {
    right: 6px;
    border-radius: var(--r-xs) 0 0 var(--r-xs);
    transform-origin: right;
  }

  .out .grip {
    left: 6px;
    border-radius: 0 var(--r-xs) var(--r-xs) 0;
    transform-origin: left;
  }

  .grip::after {
    content: "";
    width: 2px;
    height: 16px;
    max-height: 40%;
    border-radius: 1px;
    background: var(--muted);
    transition: background var(--dur-fast);
  }

  .trimmed .grip,
  .handle:hover .grip,
  .handle.active .grip {
    background: var(--accent);
  }

  .trimmed .grip::after,
  .handle:hover .grip::after,
  .handle.active .grip::after {
    background: var(--on-accent);
  }

  .handle:hover .grip {
    transform: scaleX(1.2);
  }

  .handle.active .grip {
    transform: scaleX(1.3);
  }

  /* Caught on something: it swells, and a line shows what caught it. */
  .handle.snapped .grip {
    transform: scaleX(1.5);
  }

  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -1px;
    width: 2px;
    border-radius: 1px;
    background: var(--text);
    will-change: transform;
    z-index: 4;
  }

  .cap {
    position: absolute;
    top: 2px;
    left: -4px;
    width: 10px;
    height: 12px;
    border-radius: 3px 3px 5px 5px;
    background: var(--text);
    box-shadow: var(--shadow-1);
  }

  .hover {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--line-strong);
  }

  .tip {
    position: absolute;
    top: 1px;
    translate: -50% 0;
    padding: 1px 5px;
    border-radius: var(--r-xs);
    font-size: var(--fs-2xs);
    background: var(--surface-3);
    color: var(--text);
    white-space: nowrap;
    box-shadow: var(--shadow-1);
    z-index: 5;
  }

  .snap {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--accent);
    animation: fade var(--dur-fast) var(--ease-out);
  }

  .dragging,
  .dragging .lane {
    cursor: grabbing;
    user-select: none;
  }

  /* Short windows keep the picture: every row gives up a little. */
  @media (max-height: 700px) {
    .timeline {
      --lane-h: 31px;
      --events: 28px;
      --tail: 8px;
    }

    .line {
      height: 18px;
    }

    .audio .head :global(.slider) {
      height: 10px;
    }
  }
</style>
