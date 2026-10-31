<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount } from "svelte";
  import * as api from "../api";
  import Logo from "../Logo.svelte";
  import { renderMarkdown } from "../markdown";
  import { EDITOR_KEYS } from "../shortcuts";
  import Disclosure from "./Disclosure.svelte";

  let version = $state("");
  onMount(() => {
    getVersion().then((v) => (version = v), () => {});
  });

  // Release notes ship with the app (releases/vX.Y.Z.md, the same text as the
  // GitHub release), so they read offline. They're here for the curious;
  // Klipp never opens them by itself.
  const files = import.meta.glob("../../../../releases/v*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
  // Newest first; a test build ("0.3.5-test.2") comes before its release ("0.3.5") and after the one before.
  const parts = (v: string) => {
    const [core, pre] = v.split("-");
    return { n: core.split(".").map(Number), pre: pre ? Number(pre.split(".")[1] ?? 0) : Infinity };
  };
  const newer = (a: string, b: string) => {
    const [x, y] = [parts(a), parts(b)];
    for (let i = 0; i < 3; i++) if ((x.n[i] ?? 0) !== (y.n[i] ?? 0)) return (y.n[i] ?? 0) - (x.n[i] ?? 0);
    return y.pre === x.pre ? 0 : y.pre > x.pre ? 1 : -1;
  };
  const releases = Object.entries(files)
    .map(([path, text]) => {
      const version = /v(\d+\.\d+\.\d+(?:-test\.\d+)?)\.md$/.exec(path)?.[1] ?? "";
      // From the first heading on: what came before is for the GitHub page (download link).
      const from = text.search(/^#{1,4}\s/m);
      return { version, html: renderMarkdown(from >= 0 ? text.slice(from) : text) };
    })
    .filter((r) => r.version)
    .sort((a, b) => newer(a.version, b.version));

  let notes: HTMLDivElement;
  onMount(() => {
    // Links open in the browser, not inside Klipp's window.
    const onClick = (e: MouseEvent) => {
      const a = (e.target as HTMLElement).closest("a");
      if (!a) return;
      e.preventDefault();
      api.openFile(a.href);
    };
    notes.addEventListener("click", onClick);
    return () => notes.removeEventListener("click", onClick);
  });
</script>

<div class="hero">
  <Logo size={36} />
  {#if version}<span class="version mono">Version {version}</span>{/if}
</div>

<div class="group">
  <h3 class="group-title">Editor</h3>
  <Disclosure label="Shortcuts">
    <div class="keys">
      {#each EDITOR_KEYS as [combo, what] (what)}
        <div class="key">
          <span class="combo">{#each combo as k (k)}<kbd>{k}</kbd>{/each}</span>
          <span>{what}</span>
        </div>
      {/each}
    </div>
  </Disclosure>
</div>

<div class="group" bind:this={notes}>
  <h3 class="group-title">What's new</h3>
  {#each releases as r, i (r.version)}
    <Disclosure label="Klipp {r.version}" open={i === 0}>
      {#snippet meta()}
        {#if r.version === version}<span class="tag">You have this</span>{/if}
      {/snippet}
      <div class="md">{@html r.html}</div>
    </Disclosure>
  {/each}
</div>

<div class="group">
  <p class="note">Video is encoded on your graphics card and saved with FFmpeg (LGPL).</p>
  <p class="note">Klipp isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games or anyone officially involved in producing or managing Riot Games properties. Riot Games, and all associated properties are trademarks or registered trademarks of Riot Games, Inc.</p>
</div>

<style>
  .hero {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
  }

  .version {
    color: var(--muted);
    font-size: var(--fs-sm);
  }

  .keys {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    column-gap: var(--sp-6);
    padding: var(--sp-1) 0 var(--sp-2) var(--sp-5);
  }

  .key {
    display: grid;
    grid-template-columns: 84px 1fr;
    align-items: center;
    padding: 6px 0;
    font-size: var(--fs-control);
    color: var(--muted);
  }

  .combo {
    display: flex;
    gap: var(--sp-1);
  }

  .tag {
    padding: 1px 6px;
    border-radius: var(--r-xs);
    background: var(--surface-3);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 500;
  }

  .md {
    padding: 0 0 var(--sp-3) var(--sp-5);
    color: var(--muted);
    font-size: var(--fs-control);
    line-height: 1.55;
  }

  .md :global(h3),
  .md :global(h4),
  .md :global(h5) {
    margin: var(--sp-3) 0 var(--sp-1);
    font-size: var(--fs-sm);
    font-weight: 650;
    color: var(--faint);
  }

  .md :global(ul) {
    margin: 0;
    padding-left: 18px;
  }

  .md :global(li) {
    margin: 3px 0;
  }

  .md :global(b) {
    color: var(--text);
    font-weight: 600;
  }

  .md :global(p) {
    margin: 6px 0;
  }

  .md :global(a) {
    color: var(--accent);
  }

  .md :global(code) {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }

  .group:last-child {
    gap: var(--sp-2);
  }
</style>
