<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount } from "svelte";
  import * as api from "../api";
  import Icon from "../Icon.svelte";
  import Logo from "../Logo.svelte";
  import { renderMarkdown } from "../markdown";
  import { library, toast } from "../state.svelte";

  let version = $state("");
  onMount(() => {
    getVersion().then((v) => (version = v), () => {});
  });

  // Release notes ship with the app (releases/vX.Y.Z.md, the same text as the
  // GitHub release), so they read offline. They're here for the curious;
  // Klipp never opens them by itself.
  const files = import.meta.glob("../../../../releases/v*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
  const parts = (v: string) => v.split(".").map(Number);
  const newer = (a: string, b: string) => {
    const [x, y] = [parts(a), parts(b)];
    for (let i = 0; i < 3; i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (y[i] ?? 0) - (x[i] ?? 0);
    return 0;
  };
  const releases = Object.entries(files)
    .map(([path, text]) => {
      const version = /v(\d+\.\d+\.\d+)\.md$/.exec(path)?.[1] ?? "";
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

  async function copyDetails() {
    try {
      const ok = await api.copyText(await api.supportDetails());
      toast(ok ? "Details copied. Paste them into your message or GitHub issue." : "Couldn't copy the details.");
    } catch (e) {
      toast(`Couldn't collect the details: ${e}`);
    }
  }
</script>

<div class="hero">
  <Logo size={44} />
  <p>Your moments, kept. A replay buffer that stays light: no leaks, a few MB of RAM besides the buffer.</p>
  {#if version}<span class="label">Version {version}</span>{/if}
</div>

<div class="group links">
  <button class="ghost link" onclick={() => api.reveal(library.logDir)}><Icon name="folder" size={17} />Open the log folder<span>recorder.log, cs2.log</span></button>
  <button class="ghost link" onclick={() => api.openFile(library.settingsPath)}><Icon name="sliders" size={17} />Open the settings file<span>settings.toml</span></button>
  <button class="ghost link" onclick={() => api.reveal(library.clipsDir)}><Icon name="film" size={17} />Open the clips folder<span>{library.clipsDir}</span></button>
</div>

<span class="label group-title">Something wrong?</span>
<div class="group links">
  <button class="ghost link" onclick={copyDetails}><Icon name="copy" size={17} />Copy details<span>your PC, settings and the recent log</span></button>
  <button class="ghost link" onclick={() => api.openFile("https://github.com/alonsy/klipp/issues/new/choose")}
    ><Icon name="warning" size={17} />Report a problem or ask for something<span>GitHub</span></button
  >
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

<p class="credits">
  The save and error sounds are by
  <button class="handle" onclick={() => api.openFile("https://instagram.com/f.inancimen")}>@f.inancimen</button>. Thank you!
</p>
<p class="credits">Video is encoded on your graphics card and saved with FFmpeg (LGPL).</p>

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

  .handle {
    display: inline;
    padding: 0;
    border: none;
    background: none;
    font-size: inherit;
    color: var(--accent);
  }

  .handle:hover {
    background: none;
    text-decoration: underline;
  }
</style>
