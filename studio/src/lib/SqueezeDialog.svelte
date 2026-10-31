<script module lang="ts">
  import * as api from "./api";
  import type { SqueezeLevel, SqueezeLevelPlan, SqueezePlan, SqueezeProgress, SqueezeSummary } from "./api";
  import { size } from "./format";
  import { reload, toast } from "./state.svelte";

  /** What each level costs, in plain words (measured on real clips). `kept`: quality kept, of 5. */
  const LEVELS: Record<SqueezeLevel, { name: string; loss: string; kept: number }> = {
    low: { name: "Low", loss: "Hard to tell apart", kept: 4 },
    mid: { name: "Mid", loss: "Same picture, a little less smooth", kept: 3 },
    high: { name: "High", loss: "Slightly softer, fine for sharing", kept: 2 },
    ultra: { name: "Ultra", loss: "Visibly softer and choppier: keeps the moment, not the detail", kept: 1 },
  };

  /** What a level does, from its limits in the plan. The size stays (only 4K comes down to 1440p). */
  function what(l: SqueezeLevelPlan): string {
    const fps = l.maxFps > 0 ? `at most ${l.maxFps} fps` : "";
    if (l.level === "low") return "Same size and smoothness, half the bitrate";
    if (l.level === "mid") return `Same size, ${fps}`;
    if (l.level === "high") return `Same size, ${fps}, fewer details`;
    return `Same size, ${fps}, far fewer details`;
  }

  const clips = (n: number) => (n === 1 ? "1 clip" : `${n} clips`);

  function summaryText(s: SqueezeSummary): string {
    if (s.cancelled) return s.squeezed > 0 ? `Stopped. Squeezed ${clips(s.squeezed)}, freed ${size(s.savedBytes)}.` : "Stopped. No clips were changed.";
    if (s.squeezed === 0) return "Nothing needed squeezing.";
    return `Squeezed ${clips(s.squeezed)}, freed ${size(s.savedBytes)}.`;
  }

  /** A dialog is showing; otherwise the end of a squeeze is a toast. */
  const shown = { open: false };
  let watching = false;

  /** The library reloads after every squeeze, even when the dialog was closed meanwhile. */
  function watch() {
    if (watching) return;
    watching = true;
    api.onSqueeze({
      progress: () => {},
      done: (s) => {
        reload();
        if (!shown.open && !s.error) toast(summaryText(s), undefined, 10000);
      },
    });
  }
</script>

<script lang="ts">
  // Squeeze Space: re-encodes clips smaller at one of four levels, after
  // showing what each saves and what it costs. Opened from Settings → Storage.
  import { onMount, untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import { enter, leave, pop } from "./motion";
  import Num from "./settings/Num.svelte";
  import Toggle from "./ui/Toggle.svelte";

  interface Props {
    /** Shows the dialog. A squeeze keeps going while it's closed. */
    open: boolean;
    /** Close was asked for (×, Escape, the backdrop, Done). */
    onclose: () => void;
  }
  let { open, onclose }: Props = $props();

  type Phase = "plan" | "confirm" | "running" | "done";
  let phase = $state<Phase>("plan");
  let skipFavorites = $state(true);
  let plan = $state<SqueezePlan | null>(null);
  let loading = $state(false);
  let failed = $state<string | null>(null);
  let chosen = $state<SqueezeLevel | null>(null);
  let progress = $state<SqueezeProgress | null>(null);
  let summary = $state<SqueezeSummary | null>(null);

  let choice = $derived(plan?.levels.find((l) => l.level === chosen) ?? null);
  let canSqueeze = $derived(!!plan?.encoder && !!choice && choice.clips > 0);
  let chosenIndex = $derived(plan?.levels.findIndex((l) => l.level === chosen) ?? -1);

  onMount(() => {
    watch();
    let off = () => {};
    api
      .onSqueeze({
        progress: (p) => {
          progress = p;
          if (phase !== "running") phase = "running";
        },
        done: (s) => {
          if (phase !== "running") return;
          summary = s;
          progress = null;
          phase = "done";
        },
      })
      .then((unlisten) => (off = unlisten));
    return () => off();
  });

  $effect(() => {
    shown.open = open;
    if (open) untrack(opened);
    return () => (shown.open = false);
  });

  /** Picks up a squeeze that's running, or works out the plan. */
  async function opened() {
    const running = await api.squeezeStatus().catch(() => null);
    if (running) {
      progress = running;
      phase = "running";
      return;
    }
    if (phase === "running") return;
    phase = "plan";
    summary = null;
    await load();
  }

  async function load() {
    loading = true;
    failed = null;
    try {
      plan = await api.squeezePlan(skipFavorites);
      if (!plan.levels.some((l) => l.level === chosen && l.clips > 0)) chosen = null;
    } catch (e) {
      failed = String(e);
    } finally {
      loading = false;
    }
  }

  function setSkipFavorites(v: boolean) {
    skipFavorites = v;
    load();
  }

  async function squeeze() {
    if (!chosen || !choice) return;
    const level = chosen;
    progress = { level, done: 0, total: choice.clips, current: "", fraction: 0, savedBytes: 0 };
    phase = "running";
    try {
      await api.startSqueeze(level, skipFavorites);
    } catch (e) {
      toast(`Couldn't start: ${e}`);
      progress = null;
      phase = "plan";
    }
  }

  function close() {
    if (phase === "confirm") phase = "plan";
    onclose();
  }

  function onKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  const saved = (l: SqueezeLevelPlan) => Math.max(0, l.bytesBefore - l.bytesAfter);
  const percent = (l: SqueezeLevelPlan) => (l.bytesBefore > 0 ? Math.round((saved(l) / l.bytesBefore) * 100) : 0);
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <div class="scrim fade-in" role="presentation" onclick={close} out:leave>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="squeeze-title" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}} in:pop>
      <div class="head">
        <div>
          <h2 id="squeeze-title">Squeeze space</h2>
          <p class="sub">Re-encodes clips smaller on your graphics card. Every audio track and marker stays.</p>
        </div>
        <button class="ghost icon" title="Close" aria-label="Close" onclick={close}><Icon name="x" size={18} /></button>
      </div>

      {#if phase === "running" && progress}
        {@const shownDone = Math.min(progress.done + 1, progress.total)}
        <div class="running" in:enter>
          <div class="p-top">
            <span class="p-pct mono"><Num value={progress.fraction * 100} format={(x) => `${Math.round(x)}%`} /></span>
            <span class="p-what">
              {#if progress.total > 0}Squeezing at {LEVELS[progress.level].name}: clip {shownDone} of {progress.total}{:else}Getting ready…{/if}
            </span>
            {#if progress.savedBytes > 0}<span class="freed">Freed <Num value={progress.savedBytes} format={size} /></span>{/if}
          </div>
          <div class="track"><div class="fill" style:transform="scaleX({progress.fraction})"></div></div>
          <span class="current mono" title={progress.current}>{progress.current || "Starting…"}</span>
          <p class="hint">You can close this, Klipp keeps going. Cancel puts the clip in progress back as it was; clips already squeezed stay squeezed.</p>
          <div class="actions">
            <button class="ghost" onclick={close}>Close</button>
            <button class="danger" onclick={() => api.cancelSqueeze()}>Cancel</button>
          </div>
        </div>
      {:else if phase === "done" && summary}
        <div class="done" in:enter>
          {#if summary.error}
            <p class="bad"><Icon name="warning" size={16} />{summary.error}</p>
          {:else}
            <div class="result" class:muted={summary.squeezed === 0}>
              <Icon name={summary.squeezed > 0 ? "check" : "info"} size={20} /><span>{summaryText(summary)}</span>
            </div>
            {#if summary.failed > 0}
              <p class="hint">{clips(summary.failed)} couldn't be squeezed and stayed as {summary.failed === 1 ? "it was" : "they were"}.</p>
              <ul class="errors">
                {#each summary.errors as e (e)}
                  <li class="mono">{e}</li>
                {/each}
              </ul>
            {/if}
          {/if}
          <div class="actions">
            <button class="primary" onclick={close}>Done</button>
          </div>
        </div>
      {:else if failed}
        <p class="bad"><Icon name="warning" size={16} />Couldn't look at your clips: {failed}</p>
        <div class="actions"><button onclick={load}>Try again</button></div>
      {:else if !plan}
        <div class="levels" aria-label="Looking at your clips…">
          {#each [0, 1, 2, 3] as i (i)}<div class="level skeleton"></div>{/each}
        </div>
      {:else}
        {#if !plan.encoder}
          <p class="bad"><Icon name="warning" size={16} />This PC's graphics card can't re-encode video, so clips can't be squeezed here.</p>
        {/if}
        <div class="scope">
          <span class="mono">{clips(plan.clips)} · {size(plan.bytes)}</span>
          {#if plan.favorites > 0}<span>· {plan.favorites === 1 ? "1 favorite" : `${plan.favorites} favorites`} left out</span>{/if}
          {#if plan.recent > 0}<span>· {clips(plan.recent)} from the last minutes left out</span>{/if}
          <span class="skip"><span>Skip favorites</span><Toggle checked={skipFavorites} label="Skip favorites" disabled={phase === "confirm"} onchange={setSkipFavorites} /></span>
        </div>

        <div class="levels" class:busy={loading} role="radiogroup" aria-label="How hard to squeeze" style:--i={chosenIndex}>
          {#if chosenIndex >= 0}<span class="thumb" aria-hidden="true"></span>{/if}
          {#each plan.levels as l (l.level)}
            {@const info = LEVELS[l.level]}
            <button
              class="level"
              class:on={chosen === l.level}
              role="radio"
              aria-checked={chosen === l.level}
              disabled={l.clips === 0 || !plan.encoder}
              onclick={() => {
                chosen = l.level;
                phase = "plan";
              }}
            >
              <span class="l-name">{info.name}</span>
              <span class="l-text">
                <span class="l-what">{what(l)}</span>
                <span class="l-loss" class:warm={info.kept === 2} class:hot={info.kept === 1}>
                  <span class="meter" role="img" aria-label="Quality kept: {info.kept} of 5">
                    {#each [1, 2, 3, 4, 5] as n (n)}<i class:full={n <= info.kept}></i>{/each}
                  </span>
                  {info.loss}
                </span>
              </span>
              <span class="l-gain">
                {#if l.clips > 0}
                  <span class="l-save"><span class="l-about">Saves about</span> <b class="mono"><Num value={saved(l)} format={size} /></b></span>
                  <span class="l-pct">{percent(l)}% smaller · {clips(l.clips)}{#if l.already > 0}{" "}· {l.already} squeezed already{/if}</span>
                {:else}
                  <span class="l-none">Nothing to squeeze</span>
                  <span class="l-pct">{l.already > 0 ? "Squeezed this hard already" : "Small enough already"}</span>
                {/if}
              </span>
            </button>
          {/each}
        </div>

        <div class="warn">
          <Icon name="warning" size={16} />
          <span><b>Squeezing replaces your clips and can't be undone.</b> Each clip can be squeezed twice at most, and only harder than before.</span>
        </div>

        <div class="actions">
          {#if phase === "confirm" && choice}
            <span class="ask" in:enter={{ x: -8, y: 0 }}>Squeeze {clips(choice.clips)} at {LEVELS[choice.level].name}? This can't be undone.</span>
            <button class="ghost" onclick={() => (phase = "plan")}>Back</button>
            <button class="primary" onclick={squeeze}>Yes, squeeze them</button>
          {:else}
            <button class="ghost" onclick={close}>Cancel</button>
            <button class="primary" disabled={!canSqueeze || loading} onclick={() => (phase = "confirm")}>
              {choice && choice.clips > 0 ? `Squeeze ${clips(choice.clips)}` : "Pick a level"}
            </button>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    background: var(--scrim);
    backdrop-filter: blur(6px);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    width: min(720px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    padding: var(--sp-5);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--shadow-3);
  }

  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-4);
  }

  h2 {
    margin: 0 0 var(--sp-1);
    font-size: var(--fs-lg);
    font-weight: 650;
  }

  .sub {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-control);
    line-height: 1.45;
  }

  .head .icon {
    flex-shrink: 0;
    width: 30px;
    height: 30px;
    color: var(--muted);
  }

  .scope {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1) 6px;
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .scope .mono {
    color: var(--text);
  }

  .skip {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    margin-left: auto;
    color: var(--text);
    font-size: var(--fs-control);
  }

  /* One row per level; the highlight slides between them. */
  .levels {
    --row: 62px;
    position: relative;
    display: grid;
    grid-auto-rows: var(--row);
    transition: opacity var(--dur) var(--ease-out);
  }

  .levels.busy {
    opacity: 0.6;
  }

  .thumb {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: var(--row);
    border-radius: var(--r-sm);
    background: var(--surface-2);
    transform: translateY(calc(var(--i) * 100%));
    transition: transform var(--dur-spring) var(--spring);
  }

  .level {
    position: relative;
    display: grid;
    grid-template-columns: 52px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--sp-3);
    padding: 0 var(--sp-3);
    border-radius: var(--r-sm);
    background: transparent;
    text-align: left;
  }

  .level.skeleton {
    margin-bottom: var(--sp-1);
  }

  .level:hover:not(:disabled) {
    background: var(--hover);
  }

  .level.on:hover {
    background: transparent;
  }

  .level:active {
    transform: none;
  }

  .level:disabled {
    cursor: default;
  }

  .l-name {
    font-weight: 600;
    color: var(--muted);
    transition: color var(--dur-fast);
  }

  .on .l-name {
    color: var(--accent);
  }

  .l-text,
  .l-gain {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .l-gain {
    align-items: flex-end;
    text-align: right;
  }

  .l-what {
    font-size: var(--fs-control);
  }

  .l-loss {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .l-loss.warm {
    color: color-mix(in srgb, var(--danger) 55%, var(--muted));
  }

  .l-loss.hot {
    color: var(--danger);
  }

  .meter {
    display: flex;
    flex-shrink: 0;
    gap: 2px;
  }

  .meter i {
    width: 9px;
    height: 4px;
    border-radius: 2px;
    background: var(--surface-3);
  }

  .meter i.full {
    background: currentColor;
  }

  .l-save b {
    font-size: var(--fs-md);
    font-weight: 650;
  }

  .l-about {
    color: var(--muted);
    font-size: var(--fs-xs);
  }

  .l-none {
    font-size: var(--fs-control);
    color: var(--muted);
  }

  .l-pct {
    color: var(--muted);
    font-size: var(--fs-xs);
    white-space: nowrap;
  }

  .warn {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--r-sm);
    background: var(--danger-soft);
    color: var(--danger);
    font-size: var(--fs-sm);
    line-height: 1.45;
  }

  .warn :global(.icon) {
    flex-shrink: 0;
  }

  .warn span {
    color: var(--text);
  }

  .warn b {
    font-weight: 600;
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
  }

  .ask {
    margin-right: auto;
    font-size: var(--fs-control);
  }

  button.danger {
    color: var(--danger);
  }

  .bad {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin: 0;
    color: var(--danger);
    line-height: 1.45;
  }

  .running {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }

  .p-top {
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
  }

  .p-pct {
    min-width: 76px;
    font-size: var(--fs-2xl);
    font-weight: 650;
    letter-spacing: -0.03em;
  }

  .p-what {
    flex: 1;
    min-width: 0;
    color: var(--muted);
    font-size: var(--fs-control);
  }

  .freed {
    flex-shrink: 0;
    color: var(--good);
    font-size: var(--fs-control);
  }

  .track {
    position: relative;
    height: 6px;
    border-radius: var(--r-full);
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    transition: transform var(--dur-slow) var(--ease-out);
  }

  /* A sheen, so a long clip still looks alive between progress reports. */
  .track::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(100deg, transparent 30%, rgba(244, 242, 236, 0.14) 50%, transparent 70%);
    transform: translateX(-100%);
    animation: shimmer 1.8s ease-in-out infinite;
  }

  .current {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .hint {
    margin: 0;
    font-size: var(--fs-sm);
    line-height: 1.5;
    color: var(--muted);
  }

  .done {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }

  .result {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
    border-radius: var(--r-sm);
    background: var(--good-soft);
    color: var(--good);
    font-size: var(--fs-md);
  }

  .result span {
    color: var(--text);
  }

  .result.muted {
    background: var(--surface-2);
    color: var(--muted);
  }

  .errors {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--fs-xs);
    color: var(--faint);
  }

  .errors li {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
