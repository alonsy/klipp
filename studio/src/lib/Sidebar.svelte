<script lang="ts">
  import * as api from "./api";
  import { size } from "./format";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import LowSpaceCard from "./LowSpaceCard.svelte";
  import UpdateCard from "./UpdateCard.svelte";
  import { filters, library, openPage, recorder, refreshRecorder, toast, ui } from "./state.svelte";

  const DESKTOP = "Desktop";

  /** Game folders, biggest first, with Desktop last. */
  let folders = $derived.by(() => {
    const m = new Map<string, number>();
    for (const c of library.clips) m.set(c.game, (m.get(c.game) ?? 0) + 1);
    return [...m].sort((a, b) => (a[0] === DESKTOP ? 1 : b[0] === DESKTOP ? -1 : b[1] - a[1]));
  });

  let favorites = $derived(library.clips.filter((c) => c.favorite).length);
  let totalSize = $derived(library.clips.reduce((s, c) => s + c.sizeBytes, 0));
  let onClips = $derived(ui.page === "clips");

  /** Which row is current: "all", "favorites", a game, or null outside the clips page. */
  let current = $derived(!onClips ? null : filters.game ? filters.game : filters.kind === "favorites" ? "favorites" : "all");

  // One highlight slides between rows. Rows are a fixed height, so this only measures once per change.
  let list = $state<HTMLElement>();
  let pill = $state<{ y: number; h: number } | null>(null);
  let settled = $state(false);

  $effect(() => {
    void current;
    void folders;
    const row = list?.querySelector<HTMLElement>(".item.active");
    pill = row ? { y: row.offsetTop, h: row.offsetHeight } : null;
    if (row) requestAnimationFrame(() => (settled = true));
  });

  function show(game: string | null, favoritesOnly = false) {
    filters.game = game;
    filters.event = null;
    filters.kind = favoritesOnly ? "favorites" : filters.kind === "favorites" ? "all" : filters.kind;
    openPage("clips");
  }

  function folderIcon(game: string) {
    if (game === DESKTOP) return "monitor";
    if (game === "Counter-Strike 2") return "crosshair";
    return "gamepad";
  }

  async function start() {
    try {
      await api.startRecorder();
      setTimeout(refreshRecorder, 1200);
    } catch (e) {
      toast(`Couldn't start recording: ${e}`);
    }
  }

  const clipsWord = (n: number) => (n === 1 ? "1 clip" : `${n} clips`);
</script>

<aside class="shell-sidebar">
  <div class="brand" data-tauri-drag-region><Logo size={20} /></div>

  <nav bind:this={list} aria-label="Clips">
    {#if pill}<span class="pill" class:settled style="height: {pill.h}px; transform: translateY({pill.y}px)"></span>{/if}
    <button class="item" class:active={current === "all"} title="{clipsWord(library.clips.length)} · {size(totalSize)}" onclick={() => show(null)}>
      <Icon name="grid" size={16} /><span class="name">All clips</span><span class="count mono">{library.clips.length}</span>
    </button>
    <button class="item" class:active={current === "favorites"} onclick={() => show(null, true)}>
      <Icon name="star" size={16} /><span class="name">Favorites</span><span class="count mono">{favorites}</span>
    </button>
    {#if folders.length > 0}<span class="gap"></span>{/if}
    {#each folders as [game, n] (game)}
      <button class="item" class:active={current === game} onclick={() => show(game)}>
        <Icon name={folderIcon(game)} size={16} /><span class="name">{game}</span><span class="count mono">{n}</span>
      </button>
    {/each}
  </nav>

  <footer>
    <UpdateCard />
    <LowSpaceCard />
    {#if recorder.running && recorder.problem === "stopped"}
      <button class="status warn" onclick={() => openPage("settings", "advanced")}>
        <span class="pulse"></span>
        <span class="status-text">
          <b>Restarting video…</b>
          <span>The screen changed or the graphics driver reset.</span>
        </span>
      </button>
    {:else if recorder.running && recorder.problem === "blind"}
      <button class="status warn" onclick={() => openPage("settings", "video")}>
        <span class="pulse"></span>
        <span class="status-text">
          <b>Can't see the screen</b>
          <span>Clips show a still picture. If a game is open, set it to borderless.</span>
        </span>
      </button>
    {:else if recorder.running && recorder.hotkeyTaken}
      <button class="status warn" onclick={() => openPage("settings", "recording")}>
        <span class="pulse"></span>
        <span class="status-text">
          <b>{recorder.hotkey} doesn't work</b>
          <span>Another app uses it. Pick another key.</span>
        </span>
      </button>
    {:else if recorder.running && recorder.problem === "slow"}
      <button class="status warn" onclick={() => openPage("settings", "video")}>
        <span class="pulse"></span>
        <span class="status-text">
          <b>Can't keep up</b>
          <span>Clips may stutter. Cap the game's frame rate, or record at a lower resolution.</span>
        </span>
      </button>
    {:else if recorder.running && recorder.reduced}
      <button
        class="status warn"
        title={recorder.reduced.learned
          ? `Games kept the graphics card too busy for ${recorder.reduced.from} in two sessions, so Klipp starts here now. It tries ${recorder.reduced.from} again after its next update, or when you change the video settings.`
          : `The game kept the graphics card too busy for ${recorder.reduced.from}. Cap its frame rate; Klipp tries ${recorder.reduced.from} again next start.`}
        onclick={() => openPage("settings", "video")}
      >
        <span class="pulse"></span>
        <span class="status-text">
          <b>Recording at {recorder.reduced.to} fps</b>
          <span>Graphics card too busy for {recorder.reduced.from}.{recorder.reduced.learned ? "" : " Cap the game's frame rate."}</span>
        </span>
      </button>
    {:else if recorder.running && !recorder.manual}
      <button class="status" title="Records while a game with auto-clips runs" onclick={() => openPage("settings", "recording")}>
        <span class="pulse"></span>
        <b class="grow">Auto-clips only</b>
      </button>
    {:else if recorder.running}
      <button class="status" title="{recorder.hotkey} saves the last {recorder.lengthSecs} s" onclick={() => openPage("settings", "recording")}>
        <span class="pulse"></span>
        <b class="grow">Recording</b>
        <kbd>{recorder.hotkey}</kbd>
      </button>
    {:else}
      <button class="status off" onclick={start}>
        <span class="dot"></span>
        <b class="grow">Not recording</b>
        <span class="start">Start</span>
      </button>
    {/if}

    <div class="tools">
      <button class="item" class:current={ui.page === "settings"} onclick={() => openPage("settings")}>
        <Icon name="sliders" size={16} /><span class="name">Settings</span>
      </button>
      <button class="ghost icon folder" title="Open clips folder" aria-label="Open clips folder" onclick={() => library.clipsDir && api.reveal(library.clipsDir)}>
        <Icon name="folder" size={16} />
      </button>
    </div>
  </footer>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    height: 100%;
    min-height: 0;
    padding: 0 var(--sp-2) var(--sp-2);
    background: var(--sunken);
  }

  /* Part of the title bar: drags the window. */
  .brand {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    height: 52px;
    /* Edge to edge, so the whole strip drags. */
    margin-inline: calc(-1 * var(--sp-2));
    padding: 0 calc(var(--sp-2) + var(--sp-3));
  }

  .brand :global(*) {
    pointer-events: none;
  }

  nav {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: none;
    /* Only when the rows don't all fit: a soft edge says there's more below. */
    animation: more-below linear both;
    animation-timeline: scroll(self);
  }

  @keyframes more-below {
    0%,
    99% {
      mask-image: linear-gradient(to bottom, #000 calc(100% - 24px), transparent);
    }
    100% {
      mask-image: none;
    }
  }

  .pill {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    border-radius: var(--r-sm);
    background: var(--surface);
  }

  .pill.settled {
    transition: transform var(--dur-spring) var(--spring);
  }

  .gap {
    flex-shrink: 0;
    height: var(--sp-3);
  }

  .item {
    position: relative;
    flex-shrink: 0;
    justify-content: flex-start;
    gap: 10px;
    width: 100%;
    height: 34px;
    padding: 0 var(--sp-3);
    background: transparent;
    color: var(--muted);
    text-align: left;
  }

  .item:active {
    transform: none;
  }

  .item:hover {
    background: var(--hover);
    color: var(--text);
  }

  .item.active {
    background: transparent;
    color: var(--text);
  }

  .item.current {
    background: var(--surface);
    color: var(--text);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Numbers wait until they're looked for. */
  .count {
    font-size: var(--fs-xs);
    color: var(--faint);
    opacity: 0;
    transition: opacity var(--dur-fast);
  }

  .item:hover .count,
  .item.active .count,
  .item:focus-visible .count {
    opacity: 1;
  }

  footer {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  /* Update and low-space cards: a little air above the status. */
  footer > :global(.update),
  footer > :global(.low) {
    margin-bottom: var(--sp-1);
  }

  .status {
    justify-content: flex-start;
    gap: 10px;
    width: 100%;
    min-height: 34px;
    padding: 6px var(--sp-3);
    background: transparent;
    text-align: left;
    font-size: var(--fs-control);
  }

  .status:active {
    transform: none;
  }

  .status:hover {
    background: var(--hover);
  }

  .status b,
  .status-text b {
    font-weight: 550;
  }

  /* All is well: it doesn't need to compete with the clips. */
  .grow {
    flex: 1;
    min-width: 0;
    font-weight: 500 !important;
    color: var(--muted);
    transition: color var(--dur-fast);
  }

  .status:hover .grow {
    color: var(--text);
  }

  .status kbd {
    background: var(--surface-2);
    color: var(--muted);
  }

  .status-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .status-text span {
    font-size: var(--fs-xs);
    line-height: 1.4;
    color: var(--muted);
  }

  .start {
    color: var(--accent);
    font-weight: 600;
  }

  /* The watchdog's warnings: amber, and they keep their words. */
  .status.warn {
    --accent: var(--warn);
    align-items: flex-start;
    padding: 10px var(--sp-3);
    background: var(--warn-soft);
  }

  .status.warn .pulse {
    margin-top: 4px;
  }

  .pulse,
  .dot {
    position: relative;
    width: 8px;
    height: 8px;
    flex-shrink: 0;
    margin: 0 4px;
    border-radius: 50%;
    background: var(--accent);
  }

  .pulse::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 2.4s var(--ease-out) infinite;
  }

  .dot {
    background: var(--faint);
  }

  @keyframes pulse {
    from {
      transform: scale(1);
      opacity: 0.5;
    }
    to {
      transform: scale(2.6);
      opacity: 0;
    }
  }

  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .tools .item {
    flex: 1;
    width: auto;
  }

  .folder {
    flex-shrink: 0;
    color: var(--muted);
  }

  .folder:hover {
    color: var(--text);
  }
</style>
