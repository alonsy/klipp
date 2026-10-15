<script lang="ts">
  import * as api from "../api";
  import { size } from "../format";
  import { library } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import { set, store } from "./store.svelte";

  const s = $derived(store.s!);

  let byGame = $derived.by(() => {
    const m = new Map<string, { n: number; bytes: number }>();
    for (const c of library.clips) {
      const g = m.get(c.game) ?? { n: 0, bytes: 0 };
      g.n++;
      g.bytes += c.sizeBytes;
      m.set(c.game, g);
    }
    return [...m].sort((a, b) => b[1].bytes - a[1].bytes);
  });
  let total = $derived(byGame.reduce((t, [, g]) => t + g.bytes, 0));
</script>

<div class="group">
  <Field title="Clips folder" hint="Each game gets its own folder inside. Klipp keeps its notes about clips in a hidden .klipp folder there.">
    <input class="path" value={s.storage.clips_dir} placeholder={library.clipsDir} onchange={(e) => set(["storage", "clips_dir"], e.currentTarget.value.trim())} />
    <button onclick={() => api.reveal(library.clipsDir)}>Open</button>
  </Field>
</div>

<span class="label group-title">Space used</span>
<div class="group usage">
  <div class="total"><span class="big mono">{size(total)}</span><span class="muted">{library.clips.length} clips</span></div>
  <div class="stack">
    {#each byGame as [game, g], i (game)}
      <span class="seg" style="flex: {g.bytes}; opacity: {1 - i * 0.18}" title="{game} · {size(g.bytes)}"></span>
    {/each}
  </div>
  <ul>
    {#each byGame as [game, g], i (game)}
      <li>
        <span class="dot" style="opacity: {1 - i * 0.18}"></span>
        <span class="game">{game}</span>
        <span class="muted mono">{g.n} clips</span>
        <span class="mono">{size(g.bytes)}</span>
      </li>
    {/each}
  </ul>
</div>

<style>
  .path {
    width: 300px;
  }

  .usage {
    padding: 18px;
  }

  .total {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 12px;
  }

  .big {
    font-size: 22px;
    font-weight: 700;
  }

  .muted {
    color: var(--muted);
  }

  .stack {
    display: flex;
    gap: 3px;
    height: 10px;
    border-radius: 5px;
    overflow: hidden;
    background: var(--surface-3);
  }

  .seg {
    background: var(--accent);
    min-width: 4px;
  }

  ul {
    list-style: none;
    margin: 14px 0 0;
    padding: 0;
  }

  li {
    display: grid;
    grid-template-columns: 12px 1fr auto 80px;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    font-size: 13px;
  }

  li .mono {
    text-align: right;
    font-size: 12px;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 3px;
    background: var(--accent);
  }
</style>
