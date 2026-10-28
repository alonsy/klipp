<script lang="ts">
  // How heavy recording is on this PC at some video settings: RAM, the
  // graphics card and the CPU as pictures that fill up, with what Windows
  // and other apps used when the PC test ran in grey and Klipp's estimated
  // part in the accent colour. Moves smoothly as the settings change.
  import { cubicOut } from "svelte/easing";
  import { prefersReducedMotion } from "svelte/motion";
  import { untrack } from "svelte";
  import { installedGb, memory, predict, type PcTest } from "./pctest";
  import type { Screen, VideoChoice } from "./presets";

  let {
    test,
    choice,
    lengthSecs,
    screen,
    audioTracks = 3,
    onrun,
    running = false,
  }: {
    test: PcTest | null;
    choice: VideoChoice;
    lengthSecs: number;
    screen: Screen;
    /** Audio tracks in the buffer (the mix and each source). */
    audioTracks?: number;
    /** Runs the PC test (shows "Run the PC test" / "Test again"). */
    onrun?: () => void;
    running?: boolean;
  } = $props();

  const uid = $props.id();
  const p = $derived(test ? predict(test, choice, lengthSecs, screen, audioTracks) : null);
  const sys = $derived(test?.system ?? null);

  const pct = (part: number, whole: number) => (whole > 0 ? Math.min(100, (part / whole) * 100) : 0);
  const ramTotal = $derived(sys?.ramTotalMb ?? 0);
  const target = $derived({
    ramOthers: pct(sys?.ramUsedMb ?? 0, ramTotal),
    ramKlipp: pct(p?.ramMb ?? 0, ramTotal),
    gpuOthers: sys?.gpu3d ?? 0,
    gpuKlipp: p?.gpu3d ?? 0,
    cpuOthers: sys?.cpu ?? 0,
    cpuKlipp: p?.cpu ?? 0,
    encOthers: sys?.gpuEncode ?? 0,
    encKlipp: p?.encoder ?? 0,
    ramMb: p?.ramMb ?? 0,
    vramMb: p?.vramMb ?? 0,
    perMinute: p?.mbPerMinute ?? 0,
    mbps: p?.mbps ?? 0,
  });
  // Moves to new numbers over 420 ms as the settings change; a new test shows
  // its numbers at once. (A derived of svelte/motion's Tween didn't follow it
  // in the window, so this animates by hand, and lands on the numbers anyway
  // when animation frames don't run: the window hidden or covered.)
  type Values = typeof target;
  let v = $state<Values>(untrack(() => ({ ...target })));
  let shown: PcTest | null = untrack(() => test);
  $effect(() => {
    const to = { ...target };
    const fresh = test !== shown;
    shown = test;
    if (fresh || prefersReducedMotion.current) {
      v = to;
      return;
    }
    const from = untrack(() => ({ ...v }));
    const start = performance.now();
    let frame = 0;
    const step = (now: number) => {
      const k = cubicOut(Math.min(1, (now - start) / 420));
      v = Object.fromEntries((Object.keys(to) as (keyof Values)[]).map((key) => [key, from[key] + (to[key] - from[key]) * k])) as Values;
      if (k < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    const land = setTimeout(() => (v = to), 600);
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(land);
    };
  });
  /** "0.4%", "3.1%", "28%". */
  function percent(x: number) {
    if (x > 0 && x < 0.1) return "<0.1%";
    return x < 10 ? `${x.toFixed(1)}%` : `${Math.round(x)}%`;
  }
  const others = (x: number) => Math.min(100, Math.max(0, x));
  /** Klipp's part starts where the others' ends, at least a sliver so it shows. */
  const klipp = (o: number, k: number) => (k <= 0 ? 0 : Math.max(Math.min(k, 100 - others(o)), 2.4));

  const ramGb = $derived(installedGb(ramTotal));
  const cores = $derived(Math.max(2, sys?.logicalCores ?? 8));
  /** The CPU picture's cells: two rows, up to eight across. */
  const cols = $derived(Math.min(8, Math.ceil(cores / 2)));
  const tested = $derived(test ? new Date(test.at).toLocaleDateString(undefined, { day: "numeric", month: "short" }) : "");
  const ramWith = $derived(Math.min(100, v.ramOthers + v.ramKlipp));
  const tight = $derived(!!p && !!sys && (sys.ramUsedMb + p.ramMb) / sys.ramTotalMb > 0.92);

  // The pictures' fill areas: x where they start, and their width (viewBox units).
  const RAM = { x: 9, w: 142 };
  const GPU = { x: 8, w: 132 };
  const CPU = { x: 20, w: 120 };
  const fill = (area: { x: number; w: number }, o: number, k: number) => ({
    ox: area.x,
    ow: (area.w * others(o)) / 100,
    kx: area.x + (area.w * others(o)) / 100,
    kw: (area.w * klipp(o, k)) / 100,
  });
  const ramFill = $derived(fill(RAM, v.ramOthers, v.ramKlipp));
  const gpuFill = $derived(fill(GPU, v.gpuOthers, v.gpuKlipp));
  const cpuFill = $derived(fill(CPU, v.cpuOthers, v.cpuKlipp));
  /** A core's width in the CPU picture. */
  const cell = $derived((CPU.w - (cols - 1) * 2) / cols);

  /** Half a circle from left to right: the encoder gauge's arc for a 0–100 stretch. */
  function arc(from: number, to: number) {
    const r = 26;
    const at = (x: number) => {
      const a = Math.PI * (1 - Math.min(100, Math.max(0, x)) / 100);
      return `${(32 + r * Math.cos(a)).toFixed(2)} ${(32 - r * Math.sin(a)).toFixed(2)}`;
    };
    return `M ${at(from)} A ${r} ${r} 0 0 1 ${at(to)}`;
  }
</script>

{#snippet layers(id: string, f: { ox: number; ow: number; kx: number; kw: number }, area: { x: number; w: number })}
  <g clip-path="url(#{id})">
    <rect class="empty" x={area.x} y="0" width={area.w} height="60" />
    <rect class="others" x={f.ox} y="0" width={f.ow} height="60" />
    <rect class="klipp" x={f.kx} y="0" width={f.kw} height="60" />
  </g>
{/snippet}

{#if !test || !p || !sys}
  <div class="meters quiet">
    <div class="ghosts" aria-hidden="true">
      <span></span><span></span><span></span>
    </div>
    <p>{running ? "Testing your PC… about 20 seconds. Klipp keeps recording meanwhile." : "Run the PC test to see how much of your PC Klipp uses at these settings."}</p>
    {#if onrun}
      <button onclick={onrun} disabled={running}>{running ? "Testing…" : "Run the PC test"}</button>
    {/if}
  </div>
{:else}
  <div class="meters">
    <div class="head">
      <span class="label">On this PC</span>
      <span class="when">Tested {tested}{sys.gpuName ? ` · ${sys.gpuName}` : ""}</span>
      <span class="legend"><i class="k"></i>Klipp<i class="o"></i>Everything else</span>
      {#if onrun}
        <button class="ghost small" onclick={onrun} disabled={running}>{running ? "Testing…" : "Test again"}</button>
      {/if}
    </div>

    <div class="cards">
      <!-- RAM: a memory stick whose chips fill up. -->
      <div class="card">
        <div class="top"><span class="label">RAM</span><span class="total mono">{percent(ramWith)} in use</span></div>
        <svg viewBox="0 0 160 60" role="img" aria-label="RAM: Klipp {memory(p.ramMb)} of {ramGb} GB, Windows and apps {memory(sys.ramUsedMb)}">
          <defs>
            <clipPath id="{uid}-ram">
              {#each Array(8) as _, i (i)}<rect x={9 + i * 18} y="12" width="16" height="24" rx="1.5" />{/each}
            </clipPath>
          </defs>
          <rect class="board" x="3" y="6" width="154" height="44" rx="3" />
          {@render layers(`${uid}-ram`, ramFill, RAM)}
          {#each Array(29) as _, i (i)}
            {#if i !== 19}<rect class="pin" x={9 + i * 5} y="41" width="2.6" height="7" rx="0.6" />{/if}
          {/each}
          <rect class="notch" x="102" y="44" width="5" height="7" rx="1" />
        </svg>
        <div class="row k"><i></i><span class="name">Klipp</span><b class="mono">{memory(v.ramMb)}</b></div>
        <div class="row o"><i></i><span class="name">Windows and apps</span><span class="mono">{memory(sys.ramUsedMb)}</span></div>
        <div class="note">of {ramGb} GB · {percent(v.ramKlipp)} for Klipp</div>
      </div>

      <!-- GPU: a graphics card whose cover fills up behind its fans. -->
      <div class="card">
        <div class="top"><span class="label">GPU</span><span class="total mono">{percent(Math.min(100, v.gpuOthers + v.gpuKlipp))} busy</span></div>
        <svg viewBox="0 0 160 60" role="img" aria-label="Graphics card: Klipp {percent(p.gpu3d)}, other apps {percent(sys.gpu3d ?? 0)}">
          <defs>
            <clipPath id="{uid}-gpu"><rect x="8" y="11" width="132" height="30" rx="3" /></clipPath>
          </defs>
          <rect class="board" x="3" y="6" width="142" height="40" rx="5" />
          <rect class="bracket" x="148" y="3" width="7" height="52" rx="1.2" />
          {@render layers(`${uid}-gpu`, gpuFill, GPU)}
          {#each [42, 106] as cx (cx)}
            <circle class="fan" {cx} cy="26" r="12" />
            {#each [0, 72, 144, 216, 288] as a (a)}
              <path class="blade" d="M {cx} 26 q 5 -3 8.5 -8" transform="rotate({a} {cx} 26)" />
            {/each}
            <circle class="hub" {cx} cy="26" r="3" />
          {/each}
          {#each Array(16) as _, i (i)}
            {#if i !== 4}<rect class="pin" x={30 + i * 4.6} y="47" width="2.6" height="6" rx="0.6" />{/if}
          {/each}
        </svg>
        <div class="row k"><i></i><span class="name">Klipp</span><b class="mono">{percent(v.gpuKlipp)}</b></div>
        <div class="row o"><i></i><span class="name">Other apps</span><span class="mono">{percent(sys.gpu3d ?? 0)}</span></div>
        <div class="note">{memory(v.vramMb)} of its {sys.vramMb ? memory(sys.vramMb) : "memory"}</div>
      </div>

      <!-- CPU: a processor whose cores fill up. -->
      <div class="card">
        <div class="top"><span class="label">CPU</span><span class="total mono">{percent(Math.min(100, v.cpuOthers + v.cpuKlipp))} busy</span></div>
        <svg viewBox="0 0 160 60" role="img" aria-label="CPU: Klipp {percent(p.cpu)}, other apps {percent(sys.cpu)}">
          <defs>
            <clipPath id="{uid}-cpu">
              {#each Array(cols * 2) as _, i (i)}<rect x={CPU.x + (i % cols) * (cell + 2)} y={i < cols ? 14 : 29} width={cell} height="13" rx="1.2" />{/each}
            </clipPath>
          </defs>
          {#each Array(21) as _, i (i)}
            <rect class="pin" x={17 + i * 6.2} y="3" width="2.4" height="6" rx="0.6" />
            <rect class="pin" x={17 + i * 6.2} y="49" width="2.4" height="6" rx="0.6" />
          {/each}
          <rect class="board" x="13" y="7" width="134" height="44" rx="4" />
          {@render layers(`${uid}-cpu`, cpuFill, CPU)}
        </svg>
        <div class="row k"><i></i><span class="name">Klipp</span><b class="mono">{percent(v.cpuKlipp)}</b></div>
        <div class="row o"><i></i><span class="name">Other apps</span><span class="mono">{percent(sys.cpu)}</span></div>
        <div class="note">about {percent(p.cpuCore)} of one core</div>
      </div>
    </div>

    <div class="foot">
      <div class="encoder">
        <svg viewBox="0 0 64 36" role="img" aria-label="Video encoder: Klipp {percent(p.encoder)}">
          <path class="track" d={arc(0, 100)} />
          {#if v.encOthers > 0.5}<path class="arc-others" d={arc(0, others(v.encOthers))} />{/if}
          {#if v.encKlipp > 0}<path class="arc-klipp" d={arc(others(v.encOthers), others(v.encOthers) + klipp(v.encOthers, v.encKlipp))} />{/if}
        </svg>
        <div>
          <span class="what">Video encoder <b class="mono">{percent(v.encKlipp)}</b></span>
          <span class="hint">Its own part of the graphics card, so games barely feel it.</span>
        </div>
      </div>
      <div class="size">
        <span class="what">About <b class="mono">{memory(v.perMinute)}</b> per minute of clips</span>
        <span class="hint">About {v.mbps.toFixed(0)} Mbit/s in games; quiet scenes take less.</span>
      </div>
    </div>

    {#if !p.smooth}
      <p class="warn">The test saw the graphics card fall behind near these settings, so clips may stutter. A lower frame rate or size helps.</p>
    {:else if tight}
      <p class="warn">With this clip length the PC's memory gets nearly full. A shorter clip length helps.</p>
    {/if}
  </div>
{/if}

<style>
  .meters {
    --others: color-mix(in srgb, var(--muted) 48%, var(--surface-3));
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 16px 16px;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--line);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 24px;
  }

  .when {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--faint);
    font-size: 12px;
  }

  button.small {
    padding: 3px 10px;
    font-size: 12px;
    color: var(--muted);
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(176px, 1fr));
    gap: 10px;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 12px 12px 11px;
    border-radius: 11px;
    background: var(--surface-2);
    border: 1px solid var(--line);
    min-width: 0;
  }

  .top {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }

  .total {
    font-size: 11px;
    color: var(--faint);
  }

  svg {
    display: block;
    width: 100%;
    height: auto;
    margin: 4px 0 6px;
    overflow: visible;
  }

  .board {
    fill: var(--surface-3);
  }

  .empty {
    fill: var(--stage);
  }

  .others {
    fill: var(--others);
  }

  .klipp {
    fill: var(--accent);
  }

  .pin {
    fill: var(--faint);
    opacity: 0.55;
  }

  .notch {
    fill: var(--surface-2);
  }

  .bracket {
    fill: var(--faint);
    opacity: 0.45;
  }

  .fan {
    fill: none;
    stroke: var(--surface-3);
    stroke-width: 2.2;
  }

  .blade {
    fill: none;
    stroke: var(--surface-3);
    stroke-width: 1.8;
    stroke-linecap: round;
  }

  .hub {
    fill: var(--surface-3);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .row i {
    width: 8px;
    height: 8px;
    border-radius: 2.5px;
    flex: none;
  }

  .row.k i {
    background: var(--accent);
  }

  .row.o i {
    background: var(--others);
  }

  .row .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row b,
  .row span.mono {
    margin-left: auto;
  }

  .legend {
    display: none;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--faint);
  }

  .legend i {
    width: 8px;
    height: 8px;
    border-radius: 2.5px;
  }

  .legend i.k {
    background: var(--accent);
  }

  .legend i.o {
    margin-left: 6px;
    background: var(--others);
  }

  /* Narrow (setup, a small window): three columns still, the row names move into one legend. */
  @container (max-width: 600px) {
    .cards {
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 8px;
    }

    .card {
      padding: 10px 10px 9px;
    }

    .row .name,
    .total {
      display: none;
    }

    .row b,
    .row span.mono {
      margin-left: 0;
    }

    .legend {
      display: inline-flex;
    }

    .when {
      display: none;
    }

    .head .label {
      flex: 1;
    }
  }

  @container (max-width: 380px) {
    .cards {
      grid-template-columns: 1fr;
    }
  }

  .row b {
    color: var(--text);
    font-size: 14px;
    font-weight: 600;
  }

  .row span {
    font-size: 12px;
  }

  .note {
    margin-top: 2px;
    font-size: 11.5px;
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px 22px;
    padding: 2px 2px 0;
  }

  .encoder {
    display: flex;
    align-items: center;
    gap: 12px;
    flex: 1 1 280px;
  }

  .encoder svg {
    width: 58px;
    flex: none;
    margin: 0;
  }

  .track,
  .arc-others,
  .arc-klipp {
    fill: none;
    stroke-width: 7;
  }

  .track {
    stroke: var(--stage);
    stroke-linecap: round;
  }

  .arc-others {
    stroke: var(--others);
  }

  .arc-klipp {
    stroke: var(--accent);
  }

  .encoder > div,
  .size {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .size {
    flex: 1 1 220px;
  }

  .what {
    font-size: 13px;
    color: var(--text);
  }

  .what b {
    font-weight: 600;
  }

  .hint {
    font-size: 12px;
    color: var(--faint);
    line-height: 1.4;
  }

  .warn {
    margin: 0;
    padding: 9px 12px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: color-mix(in srgb, var(--danger) 80%, var(--text));
    font-size: 12.5px;
    line-height: 1.45;
  }

  .quiet {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px 16px;
    border-style: dashed;
    border-color: var(--line-strong);
    background: transparent;
  }

  .quiet p {
    flex: 1 1 260px;
    margin: 0;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.5;
  }

  .ghosts {
    display: flex;
    gap: 5px;
  }

  .ghosts span {
    width: 26px;
    height: 34px;
    border-radius: 6px;
    background: var(--surface-2);
    border: 1px solid var(--line);
  }
</style>
