<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../Icon.svelte";
  import { DUR, enter, pageOut } from "../motion";
  import { ui } from "../state.svelte";
  import About from "./About.svelte";
  import Advanced from "./Advanced.svelte";
  import Audio from "./Audio.svelte";
  import AutoClips from "./AutoClips.svelte";
  import General from "./General.svelte";
  import Recording from "./Recording.svelte";
  import Storage from "./Storage.svelte";
  import { load, store } from "./store.svelte";
  import Video from "./Video.svelte";

  const sections = [
    { id: "general", name: "General" },
    { id: "recording", name: "Recording" },
    { id: "video", name: "Video" },
    { id: "audio", name: "Audio" },
    { id: "autoclips", name: "Auto-clips" },
    { id: "storage", name: "Storage" },
    { id: "advanced", name: "Advanced" },
    { id: "about", name: "About" },
  ];
  // Pages that moved: the save sound is under Recording now, editor keys under About.
  const moved: Record<string, string> = { sounds: "recording", playback: "about" };

  let current = $derived(sections.find((s) => s.id === (moved[ui.section] ?? ui.section)) ?? sections[0]);
  let index = $derived(sections.indexOf(current));

  // A new page comes in from the side its tab is on.
  let before = 0;
  let side = $state(0);
  $effect.pre(() => {
    side = Math.sign(index - before);
    before = index;
  });

  // The underline under the current tab: as wide as its word, and it slides.
  let nav = $state<HTMLElement>();
  let line = $state({ x: 0, w: 0 });
  /** Not on the first placement, or it would slide in from the corner. */
  let settled = $state(false);
  function measure() {
    const word = nav?.querySelector<HTMLElement>("[aria-current] span");
    if (word) line = { x: word.offsetLeft, w: word.offsetWidth };
  }
  $effect(() => {
    void index;
    measure();
  });

  onMount(() => {
    load();
    measure();
    const watcher = new ResizeObserver(measure);
    if (nav) watcher.observe(nav);
    const frame = requestAnimationFrame(() => (settled = true));
    return () => {
      watcher.disconnect();
      cancelAnimationFrame(frame);
    };
  });
</script>

<section class="settings">
  <header>
    <div class="col">
      <h1>Settings</h1>
      <nav aria-label="Settings" bind:this={nav}>
        {#each sections as s (s.id)}
          <button class="tab" aria-current={current.id === s.id ? "page" : undefined} onclick={() => (ui.section = s.id)}><span>{s.name}</span></button>
        {/each}
        <i class="line" class:settled style:transform="translateX({line.x}px) scaleX({line.w})"></i>
      </nav>
    </div>
  </header>

  {#if store.error}
    <div class="failed">
      <Icon name="warning" size={18} />
      <p>{store.error}</p>
    </div>
  {:else if store.s}
    <div class="stack body">
      {#key current.id}
        <div class="scroll" in:enter={{ x: side * 14, y: 0, delay: 50, duration: DUR.base }} out:pageOut>
          <div class="page">
            {#if current.id === "general"}<General />
            {:else if current.id === "recording"}<Recording />
            {:else if current.id === "video"}<Video />
            {:else if current.id === "audio"}<Audio />
            {:else if current.id === "autoclips"}<AutoClips />
            {:else if current.id === "storage"}<Storage />
            {:else if current.id === "advanced"}<Advanced />
            {:else}<About />{/if}
          </div>
        </div>
      {/key}
    </div>
  {:else}
    <div class="loading page" aria-hidden="true">
      {#each [40, 56, 32, 48] as w, i (i)}<span class="skeleton text" style:width="{w}%"></span>{/each}
    </div>
  {/if}
</section>

<style>
  .settings {
    --col: 680px;
    --gutter: 10px; /* the page's scrollbar */
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* The title sits in the window's top bar, where dragging goes on working. */
  header {
    flex-shrink: 0;
    margin-top: -36px;
    padding: 0 calc(var(--sp-5) + var(--gutter)) 0 var(--sp-5);
    pointer-events: none;
  }

  .col {
    max-width: var(--col);
    margin-inline: auto;
  }

  h1 {
    display: flex;
    align-items: center;
    height: 36px;
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--fs-lg);
    font-weight: 600;
    letter-spacing: -0.03em;
  }

  nav {
    position: relative;
    display: flex;
    gap: var(--sp-1);
    margin: var(--sp-1) 0 0 calc(-1 * var(--sp-3));
    padding-bottom: var(--sp-1);
    pointer-events: auto;
  }

  .tab {
    padding: 6px var(--sp-3);
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-control);
    white-space: nowrap;
  }

  .tab:hover {
    background: transparent;
    color: var(--text);
  }

  .tab:active {
    transform: none;
  }

  .tab[aria-current] {
    color: var(--text);
    font-weight: 550;
  }

  .tab span {
    display: block;
  }

  .line {
    position: absolute;
    bottom: 0;
    left: 0;
    width: 1px;
    height: 2px;
    background: var(--accent);
    transform-origin: left;
    pointer-events: none;
  }

  .line.settled {
    transition: transform var(--dur-spring) var(--spring);
  }

  .body {
    flex: 1;
  }

  .scroll {
    overflow-y: auto;
    scrollbar-gutter: stable;
  }

  .page {
    display: flex;
    flex-direction: column;
    gap: var(--sp-5);
    max-width: calc(var(--col) + 2 * var(--sp-5));
    margin-inline: auto;
    padding: var(--sp-5) var(--sp-5) var(--sp-6);
  }

  /* A group is rows and space; its heading is the only thing that marks it. */
  .page :global(.group) {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .page :global(.group .field) {
    min-height: 44px;
    padding: var(--sp-1) 0;
  }

  .page :global(.field .title) {
    font-weight: 500;
  }

  .page :global(.group-title) {
    margin: 0 0 var(--sp-1);
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--faint);
  }

  .page :global(.field .slider) {
    width: 180px;
  }

  .page :global(.field select) {
    max-width: 260px;
  }

  /* Something the page can't work around: a reason and maybe one action. */
  .page :global(.banner) {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--r-sm);
    background: var(--warn-soft);
    color: var(--warn);
    font-size: var(--fs-control);
    line-height: 1.45;
  }

  /* A line for what a change will do. Stays quiet, stays there. */
  .page :global(.note) {
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }

  .page :global(.value) {
    min-width: 52px;
    text-align: right;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-sm);
    color: var(--text);
  }

  .failed {
    display: flex;
    gap: var(--sp-3);
    width: 100%;
    max-width: calc(var(--col) + 2 * var(--sp-5));
    margin: var(--sp-5) auto;
    padding: 0 calc(var(--sp-5) + var(--gutter)) 0 var(--sp-5);
    color: var(--danger);
  }

  .failed p {
    margin: 0;
    line-height: 1.45;
    user-select: text;
  }

  .loading {
    width: 100%;
    gap: var(--sp-5);
  }
</style>
