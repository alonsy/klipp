<script lang="ts">
  // Twelve seconds of live levels for every track, heard the way the recorder
  // hears them: the quick way to see Discord lands on Chat and not on Game.
  import * as api from "./api";
  import type { TrackLevel } from "./api";
  import { toast } from "./state.svelte";

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
    <button disabled={running} onclick={check}>{running ? "Listening…" : "Check levels"}</button>
  </div>
  {#if tracks.length > 0}
    <div class="rows" aria-live="polite">
      {#each tracks as t (t.track)}
        <div class="row" class:off={!!t.off}>
          <span class="name">{t.track}</span>
          <div class="bar" role="meter" aria-label="{t.track} level" aria-valuemin={-60} aria-valuemax={0} aria-valuenow={Math.round(t.db)}>
            <div class="fill" style="width: {width(t.db)}%"></div>
            <div class="peak" style="left: {width(peaks[t.track] ?? -90)}%"></div>
          </div>
          <span class="db mono">{t.off ? OFF[t.off] : t.db <= -89 ? "silent" : `${Math.round(t.db)} dB`}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .levels {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 18px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .head p {
    flex: 1;
    margin: 0;
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.45;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .row {
    display: grid;
    grid-template-columns: 90px 1fr 150px;
    align-items: center;
    gap: 12px;
  }

  .name {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bar {
    position: relative;
    height: 8px;
    border-radius: 4px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.08s linear;
  }

  .peak {
    position: absolute;
    top: 0;
    width: 2px;
    height: 100%;
    background: var(--text);
    opacity: 0.6;
  }

  .db {
    font-size: 11.5px;
    color: var(--muted);
    text-align: right;
  }

  .row.off .name,
  .row.off .db {
    color: var(--faint);
  }

  @media (prefers-reduced-motion: reduce) {
    .fill {
      transition: none;
    }
  }
</style>
