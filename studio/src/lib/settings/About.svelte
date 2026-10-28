<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount } from "svelte";
  import * as api from "../api";
  import Icon from "../Icon.svelte";
  import Logo from "../Logo.svelte";
  import { renderMarkdown } from "../markdown";

  let version = $state("");
  let shortcuts = $state(false);
  onMount(() => {
    getVersion().then((v) => (version = v), () => {});
  });

  const keys = [
    [["Space"], "Play / pause"],
    [["I"], "Start the clip here"],
    [["O"], "End the clip here"],
    [["L"], "Loop the trimmed part"],
    [["←", "→"], "Back / forward 1 s (Shift: 5 s)"],
    [[",", "."], "Previous / next frame"],
    [["[", "]"], "Previous / next event"],
    [["Home", "End"], "Go to the start / end of the trim"],
    [["F"], "Fullscreen"],
    [["Esc"], "Back to clips"],
  ] as const;

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
  <Logo size={44} />
  <p>Your moments, kept. A replay buffer that stays light: no leaks, a few MB of RAM besides the buffer.</p>
  {#if version}<span class="label">Version {version}</span>{/if}
</div>

<span class="label group-title">Editor</span>
<div class="group links">
  <button class="ghost link" aria-expanded={shortcuts} onclick={() => (shortcuts = !shortcuts)}><Icon name="keyboard" size={17} />See shortcuts<span>{shortcuts ? "Hide" : ""}</span></button>
  {#if shortcuts}
    <div class="keys">
      {#each keys as [combo, what] (what)}
        <div class="key">
          <span class="combo">{#each combo as k (k)}<kbd>{k}</kbd>{/each}</span>
          <span>{what}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<span class="label group-title">What's new</span>
<div class="group notes" bind:this={notes}>
  {#each releases as r, i (r.version)}
    <details open={i === 0}>
      <summary>
        <b>Klipp {r.version}</b>
        {#if r.version === version}<span class="tag">You have this</span>{/if}
      </summary>
      <div class="md">{@html r.html}</div>
    </details>
  {/each}
</div>

<p class="credits">Video is encoded on your graphics card and saved with FFmpeg (LGPL).</p>
<p class="credits">Klipp isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games or anyone officially involved in producing or managing Riot Games properties. Riot Games, and all associated properties are trademarks or registered trademarks of Riot Games, Inc.</p>

<style>
  .hero {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 26px;
    border-radius: var(--radius);
    background: radial-gradient(120% 140% at 0% 0%, var(--accent-soft), transparent 55%), var(--surface);
    border: 1px solid var(--line);
  }

  .hero p {
    margin: 0;
    max-width: 440px;
    color: var(--muted);
    line-height: 1.55;
  }

  .links {
    display: flex;
    flex-direction: column;
    padding: 6px;
  }

  .link {
    justify-content: flex-start;
    gap: 12px;
    padding: 10px 12px;
    text-align: left;
  }

  .link span {
    margin-left: auto;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--faint);
  }

  .keys {
    padding: 4px 12px 8px;
  }

  .key {
    display: grid;
    grid-template-columns: 120px 1fr;
    align-items: center;
    padding: 7px 0;
    font-size: 13px;
  }

  .key + .key {
    border-top: 1px solid var(--line);
  }

  .combo {
    display: flex;
    gap: 4px;
  }

  .notes {
    display: flex;
    flex-direction: column;
    padding: 6px 18px;
  }

  details + details {
    border-top: 1px solid var(--line);
  }

  summary {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 0;
    cursor: pointer;
    list-style: none;
  }

  summary::-webkit-details-marker {
    display: none;
  }

  summary::before {
    content: "";
    width: 6px;
    height: 6px;
    border-right: 1.5px solid var(--muted);
    border-bottom: 1.5px solid var(--muted);
    transform: rotate(-45deg);
    transition: transform 0.15s;
  }

  details[open] summary::before {
    transform: rotate(45deg);
  }

  .tag {
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 11px;
    font-weight: 600;
  }

  .md {
    padding: 0 0 14px 16px;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.55;
  }

  .md :global(h3),
  .md :global(h4),
  .md :global(h5) {
    margin: 12px 0 4px;
    font-size: 12px;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
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
    font-size: 12px;
  }

  .credits {
    margin: 0 4px;
    font-size: 12px;
    color: var(--faint);
  }

</style>
