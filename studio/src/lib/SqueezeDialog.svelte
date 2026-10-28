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
  <div class="scrim" role="presentation" onclick={close}>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="squeeze-title" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
      <div class="head">
        <div>
          <h2 id="squeeze-title">Squeeze Space</h2>
          <p class="sub">Makes clips smaller by re-encoding them on your graphics card. Every audio track and marker stays.</p>
        </div>
        <button class="ghost icon" title="Close" aria-label="Close" onclick={close}><Icon name="x" size={18} /></button>
      </div>

      {#if phase === "running" && progress}
        {@const shownDone = Math.min(progress.done + 1, progress.total)}
        <div class="running">
          <div class="p-label">
            <span>
              {#if progress.total > 0}Squeezing at {LEVELS[progress.level].name}: clip {shownDone} of {progress.total}{:else}Getting ready…{/if}
            </span>
            <span class="mono">{Math.round(progress.fraction * 100)}%</span>
          </div>
          <div class="track"><div class="fill" style="width: {progress.fraction * 100}%"></div></div>
          <div class="p-meta">
            <span class="current mono" title={progress.current}>{progress.current || "Starting…"}</span>
            {#if progress.savedBytes > 0}<span class="freed">Freed {size(progress.savedBytes)} so far</span>{/if}
          </div>
          <p class="hint">You can close this, Klipp keeps going. Cancel puts the clip in progress back as it was; clips already squeezed stay squeezed.</p>
          <div class="actions">
            <button class="ghost" onclick={close}>Close</button>
            <button class="danger" onclick={() => api.cancelSqueeze()}>Cancel</button>
          </div>
        </div>
      {:else if phase === "done" && summary}
        <div class="done">
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
        <div class="loading"><span class="spinner"></span>Looking at your clips…</div>
      {:else}
        {#if !plan.encoder}
          <p class="bad"><Icon name="warning" size={16} />This PC's graphics card can't re-encode video, so clips can't be squeezed here.</p>
        {/if}
        <div class="scope">
          <span class="mono">{clips(plan.clips)} · {size(plan.bytes)}</span>
          {#if plan.favorites > 0}<span>· {plan.favorites === 1 ? "1 favorite" : `${plan.favorites} favorites`} left out</span>{/if}
          {#if plan.recent > 0}<span>· {clips(plan.recent)} from the last minutes left out</span>{/if}
          {#if loading}<span class="spinner small"></span>{/if}
        </div>

        <div class="levels" role="radiogroup" aria-label="How hard to squeeze">
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
              <span class="l-what">{what(l)}</span>
              {#if l.clips > 0}
                <span class="l-about">Saves about</span>
                <span class="l-save mono">{size(saved(l))}</span>
                <span class="l-pct">{percent(l)}% smaller · {clips(l.clips)}</span>
              {:else}
                <span class="l-save none">Nothing to squeeze</span>
                <span class="l-pct">{l.already > 0 ? "Squeezed this hard already" : "Small enough already"}</span>
              {/if}
              <span class="l-loss" class:warm={info.kept === 2} class:hot={info.kept === 1}>
                <span class="meter" aria-label="Quality kept: {info.kept} of 5">
                  {#each [1, 2, 3, 4, 5] as n (n)}<i class:full={n <= info.kept}></i>{/each}
                </span>
                {info.loss}
              </span>
              {#if l.already > 0 && l.clips > 0}<span class="l-note">{clips(l.already)} squeezed already</span>{/if}
            </button>
          {/each}
        </div>

        <div class="option">
          <div class="o-text">
            <span class="o-title">Skip favorites</span>
            <span class="o-hint">{skipFavorites ? "Favorites keep their full quality." : "Favorites are squeezed too."}</span>
          </div>
          <Toggle checked={skipFavorites} label="Skip favorites" disabled={phase === "confirm"} onchange={setSkipFavorites} />
        </div>

        <div class="warn">
          <Icon name="warning" size={16} />
          <span><b>Squeezing replaces your clips and can't be undone.</b> Each clip can be squeezed twice at most, and only harder than before.</span>
        </div>

        <div class="actions">
          {#if phase === "confirm" && choice}
            <span class="ask">Squeeze {clips(choice.clips)} at {LEVELS[choice.level].name}? This can't be undone.</span>
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
    animation: fade 0.18s var(--ease);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: min(760px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 22px 24px 20px;
    border-radius: 18px;
    background: var(--surface);
    border: 1px solid var(--line);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.55);
    animation: rise 0.22s var(--ease);
  }

  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }

  h2 {
    margin: 0 0 6px;
    font-family: var(--font-display);
    font-size: 18px;
    letter-spacing: -0.02em;
  }

  .sub {
    margin: 0;
    color: var(--muted);
    line-height: 1.5;
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
    gap: 6px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .scope .mono {
    color: var(--text);
  }

  .levels {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }

  @media (max-width: 680px) {
    .levels {
      grid-template-columns: repeat(2, 1fr);
    }
  }

  .level {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: flex-start;
    gap: 4px;
    padding: 12px 12px 11px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--line);
    text-align: left;
  }

  .level:hover:not(:disabled) {
    background: var(--surface-3);
  }

  .level.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .level:disabled {
    cursor: default;
  }

  .l-name {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .on .l-name {
    color: var(--accent);
  }

  .l-what {
    min-height: 2.8em;
    font-size: 13px;
    line-height: 1.4;
  }

  .l-about {
    margin-top: 4px;
    font-size: 11.5px;
    color: var(--muted);
  }

  .l-save {
    font-size: 19px;
    font-weight: 700;
  }

  .l-save.none {
    margin-top: 4px;
    font-size: 13px;
    font-weight: 550;
    color: var(--muted);
  }

  .l-pct,
  .l-note {
    font-size: 11.5px;
    color: var(--muted);
  }

  .l-note {
    color: var(--faint);
  }

  .l-loss {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin-top: 8px;
    padding-top: 8px;
    width: 100%;
    border-top: 1px solid var(--line);
    font-size: 12px;
    line-height: 1.35;
    color: var(--muted);
  }

  .l-loss.warm {
    color: color-mix(in srgb, var(--danger) 55%, var(--muted));
  }

  .l-loss.hot {
    color: var(--danger);
  }

  .meter {
    display: flex;
    gap: 3px;
  }

  .meter i {
    width: 14px;
    height: 4px;
    border-radius: 2px;
    background: var(--surface-3);
  }

  .meter i.full {
    background: currentColor;
  }

  .option {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 2px 0;
    border-top: 1px solid var(--line);
  }

  .o-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .o-title {
    font-size: 13px;
    font-weight: 550;
  }

  .o-hint {
    font-size: 11.5px;
    color: var(--muted);
  }

  .warn {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 10px 12px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: 12.5px;
    line-height: 1.45;
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
    gap: 8px;
  }

  .ask {
    margin-right: auto;
    font-size: 13px;
  }

  button.danger {
    color: var(--danger);
  }

  .bad {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    color: var(--danger);
    line-height: 1.45;
  }

  .loading {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 28px 0;
    color: var(--muted);
  }

  .spinner {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid currentColor;
    border-right-color: transparent;
    animation: spin 0.8s linear infinite;
  }

  .spinner.small {
    width: 11px;
    height: 11px;
  }

  .running {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .p-label,
  .p-meta {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 13px;
  }

  .p-meta {
    color: var(--muted);
    font-size: 12px;
  }

  .current {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .freed {
    flex-shrink: 0;
    color: var(--good);
  }

  .track {
    height: 6px;
    border-radius: 3px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s linear;
  }

  .hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--muted);
  }

  .done {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .result {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--good) 10%, transparent);
    color: var(--good);
    font-size: 15px;
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
    font-size: 11.5px;
    color: var(--faint);
  }

  .errors li {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(10px) scale(0.98);
    }
  }
</style>
