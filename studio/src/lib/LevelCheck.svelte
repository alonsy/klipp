<script lang="ts">
  // Twelve seconds of live levels for every track, heard the way the recorder
  // hears them: the quick way to see Discord lands on Chat and not on Game.
  import * as api from "./api";
  import type { TrackLevel } from "./api";
  import { toast } from "./state.svelte";
  import { stagger } from "./motion";

  let { hint }: { hint: string } = $props();

  let running = $state(false);
  let tracks = $state<TrackLevel[]>([]);
  /** The loudest each track got during this check, so a short sound still shows. */
  let peaks = $state<Record<string, number>>({});

  async function check() {
    running = true;
    tracks = [];
    peaks = {};
    const off = await api.onAudioLevels((t) => {
      tracks = t;
      for (const x of t) peaks[x.track] = Math.max(peaks[x.track] ?? -90, x.db);
    });
    try {
      await api.audioLevels();
    } catch (e) {
      toast(`Couldn't check the levels: ${e}`);
    } finally {
      off();
      running = false;
    }
  }

  /** -60 dB and below is an empty bar, 0 dB a full one. */
  const width = (db: number) => Math.max(0, Math.min(100, ((db + 60) / 60) * 100));

  const OFF: Record<string, string> = {
    mixer: "off with an audio mixer",
    "no discord": "Discord isn't running",
    "no device": "no device",
  };
</script>

<div class="levels">
  <div class="head">
    <p>{hint}</p>
    <button class="go" class:running disabled={running} onclick={check}>
      {#if running}<span class="pulse"></span>Listening{:else}{tracks.length > 0 ? "Check again" : "Check levels"}{/if}
    </button>
  </div>
  {#if tracks.length > 0}
    <div class="rows" aria-live="polite">
      {#each tracks as t, i (t.track)}
        <div class="row enter" class:off={!!t.off} class:heard={!t.off && t.db > -50} style:--delay="{stagger(i)}ms">
          <span class="name">{t.track}</span>
          <div class="bar" role="meter" aria-label="{t.track} level" aria-valuemin={-60} aria-valuemax={0} aria-valuenow={Math.round(t.db)}>
            <div class="leds">
              <div class="fill" style:transform="scaleX({width(t.db) / 100})"></div>
            </div>
            <div class="peak" style:left="{width(peaks[t.track] ?? -90)}%"></div>
          </div>
          <span class="db mono">{t.off ? OFF[t.off] : t.db <= -89 ? "silent" : `${Math.round(t.db)} dB`}</span>
        </div>
      {/each}
    </div>
  {:else if running}
    <div class="rows" aria-hidden="true">
      <div class="skeleton wait"></div>
      <div class="skeleton wait"></div>
    </div>
  {/if}
</div>

<style>
  .levels {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding: 14px 18px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
  }

  .head p {
    flex: 1;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.45;
  }

  .go {
    flex: none;
    min-width: 116px;
  }

  /* Listening is the lively part, not a dimmed button. */
  .go.running:disabled {
    opacity: 1;
    background: var(--accent-soft);
    color: var(--text);
  }

  .pulse {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1s var(--ease-out) infinite alternate;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }

  .wait {
    height: 18px;
    border-radius: var(--r-xs);
  }

  .row {
    display: grid;
    grid-template-columns: 90px 1fr 150px;
    align-items: center;
    gap: var(--sp-3);
    min-height: 18px;
  }

  .name {
    color: var(--muted);
    font-size: var(--fs-control);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color var(--dur) var(--ease-out);
  }

  /* A track with sound in it lights up. */
  .heard .name {
    color: var(--text);
  }

  .bar {
    position: relative;
    height: 10px;
  }

  /* The level is drawn in little segments, like a mixer's meter. */
  .leds {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: var(--surface-3);
    mask: repeating-linear-gradient(90deg, #000 0 4px, transparent 4px 6px);
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    transition: transform var(--dur-fast) linear;
  }

  .peak {
    position: absolute;
    top: -1px;
    bottom: -1px;
    width: 2px;
    border-radius: 1px;
    background: var(--text);
    opacity: 0.75;
  }

  .db {
    font-size: var(--fs-xs);
    color: var(--muted);
    text-align: right;
  }

  .row.off .name,
  .row.off .db {
    color: var(--faint);
  }

  @keyframes pulse {
    to {
      opacity: 0.35;
      transform: scale(0.8);
    }
  }
</style>
