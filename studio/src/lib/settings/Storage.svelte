<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import * as api from "../api";
  import type { MovePlan } from "../api";
  import { size } from "../format";
  import Icon from "../Icon.svelte";
  import { library, reload, toast } from "../state.svelte";
  import Field from "../ui/Field.svelte";
  import { set } from "./store.svelte";

  // Changing the folder: pick it, then (if there are clips) move them or not.
  type Change = { kind: "ask"; from: string; to: string; plan: MovePlan } | { kind: "moving"; to: string; done: number; total: number };
  let change = $state<Change | null>(null);

  async function pick() {
    const to = await open({ directory: true, defaultPath: library.clipsDir, title: "Choose where Klipp saves clips" }).catch(() => null);
    if (typeof to !== "string" || !to) return;
    let plan: MovePlan;
    try {
      plan = await api.planMove(to);
    } catch (e) {
      toast(`Couldn't use that folder: ${e}`);
      return;
    }
    if (plan.problem) {
      toast(plan.problem);
      return;
    }
    if (plan.clips === 0 && plan.bytes === 0) {
      await use(to);
      toast(`Clips are now saved in ${to}.`);
      return;
    }
    change = { kind: "ask", from: library.clipsDir, to, plan };
  }

  /** New clips go to `to` from now on. */
  async function use(to: string) {
    await set(["storage", "clips_dir"], to);
    await reload();
  }

  async function leave() {
    if (change?.kind !== "ask") return;
    const { from, to } = change;
    change = null;
    await use(to);
    toast(`New clips are saved in ${to}. Older ones stay in ${from}.`);
  }

  async function move() {
    if (change?.kind !== "ask") return;
    const { from, to, plan } = change;
    const moving = { kind: "moving" as const, to, done: 0, total: plan.bytes };
    change = moving;
    const off = await api.onMoveProgress((done, total) => {
      if (change?.kind === "moving") change = { ...moving, done, total };
    });
    try {
      // The recorder saves into the new folder from now on, even during the move.
      await set(["storage", "clips_dir"], to);
      const report = await api.moveClips(from, to);
      const what = report.moved === 1 ? "1 clip" : `${report.moved} clips`;
      if (report.left.length === 0) toast(`Moved ${what} to ${to}.`);
      else toast(`Moved ${what}. ${report.left.length} stayed in ${from}: open in another app, or already in the new folder.`, undefined, 12000);
    } catch (e) {
      toast(`Moving stopped: ${e}. Clips that weren't moved yet are still in ${from}.`, undefined, 12000);
    } finally {
      off();
      change = null;
      await reload();
    }
  }

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
    <span class="path mono" title={library.clipsDir}>{library.clipsDir}</span>
    <button onclick={pick} disabled={change?.kind === "moving"}>Change…</button>
    <button onclick={() => api.reveal(library.clipsDir)}>Open</button>
  </Field>
  {#if change?.kind === "ask"}
    {@const p = change.plan}
    <div class="move">
      <p>
        <b>Move your {p.clips === 1 ? "clip" : `${p.clips} clips`} ({size(p.bytes)}) to <span class="mono">{change.to}</span>?</b>
        They keep their trims, favorites and thumbnails.
        {p.sameDrive ? "It's the same drive, so it only takes a moment." : "They're copied to the other drive, then removed here."}
        If you don't move them, they stay where they are and the library only shows the new folder.
      </p>
      {#if !p.room}
        <p class="bad"><Icon name="warning" size={15} />Not enough space there: they need {size(p.bytes)}, and {size(p.freeBytes ?? 0)} is free.</p>
      {/if}
      <div class="actions">
        <button class="primary" disabled={!p.room} onclick={move}>Move clips</button>
        <button onclick={leave}>Don't move them</button>
        <button class="ghost" onclick={() => (change = null)}>Cancel</button>
      </div>
    </div>
  {:else if change?.kind === "moving"}
    {@const pct = change.total > 0 ? Math.min(100, Math.round((change.done / change.total) * 100)) : 0}
    <div class="move">
      <p><b>Moving clips… {pct}%</b> Klipp keeps recording; new clips already go to the new folder.</p>
      <div class="bar"><div class="fill" style="width: {pct}%"></div></div>
    </div>
  {/if}
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
    max-width: 280px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--muted);
  }

  .move {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 18px 16px;
    border-top: 1px solid var(--line);
  }

  .move p {
    margin: 0;
    max-width: 640px;
    color: var(--muted);
    line-height: 1.55;
  }

  .move b {
    color: var(--text);
    font-weight: 600;
  }

  .move .bad {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--danger);
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .bar {
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
