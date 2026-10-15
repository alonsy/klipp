<script lang="ts">
  import * as api from "./api";
  import { size } from "./format";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
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
</script>

<aside>
  <div class="brand" data-tauri-drag-region><Logo size={24} /></div>

  <span class="label heading">Clips</span>
  <nav>
    <button class="item" class:active={onClips && !filters.game && filters.kind !== "favorites"} onclick={() => show(null)}>
      <Icon name="grid" /><span class="name">All clips</span><span class="count mono">{library.clips.length}</span>
    </button>
    {#each folders as [game, n] (game)}
      <button class="item" class:active={onClips && filters.game === game} onclick={() => show(game)}>
        <Icon name={folderIcon(game)} /><span class="name">{game}</span><span class="count mono">{n}</span>
      </button>
    {/each}
    <button class="item" class:active={onClips && !filters.game && filters.kind === "favorites"} onclick={() => show(null, true)}>
      <Icon name="star" /><span class="name">Favorites</span><span class="count mono">{favorites}</span>
    </button>
  </nav>

  <nav class="bottom-nav">
    <button class="item" class:active={ui.page === "settings"} onclick={() => openPage("settings")}>
      <Icon name="sliders" /><span class="name">Settings</span>
    </button>
    <button class="item" onclick={() => library.clipsDir && api.reveal(library.clipsDir)} title={library.clipsDir}>
      <Icon name="folder" /><span class="name">Open clips folder</span>
    </button>
  </nav>

  <footer>
    <UpdateCard />
    {#if recorder.running}
      <button class="status" onclick={() => openPage("settings", "recording")} title="Recording settings">
        <span class="pulse"></span>
        <span class="status-text">
          <b>Recording</b>
          <span class="mono">{recorder.hotkey} saves {recorder.lengthSecs} s</span>
        </span>
      </button>
    {:else}
      <button class="status off" onclick={start}>
        <span class="dot"></span>
        <span class="status-text"><b>Not recording</b><span>Start</span></span>
      </button>
    {/if}
    <div class="usage mono">{library.clips.length} clips · {size(totalSize)}</div>
  </footer>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 0 12px 14px;
    background: var(--bg);
    border-right: 1px solid var(--line);
    overflow-y: auto;
  }

  /* Part of the title bar: drags the window. */
  .brand {
    display: flex;
    align-items: center;
    height: 62px;
    margin-bottom: 10px;
    padding: 0 10px;
  }

  .brand :global(*) {
    pointer-events: none;
  }

  .heading {
    padding: 0 12px 8px;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .bottom-nav {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }

  .item {
    justify-content: flex-start;
    gap: 11px;
    width: 100%;
    padding: 8px 12px;
    border: none;
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
    text-align: left;
  }

  .item:hover {
    background: var(--surface);
    color: var(--text);
  }

  .item.active {
    background: var(--surface);
    color: var(--text);
  }

  .item.active :global(.icon) {
    color: var(--accent);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    font-size: 11px;
    color: var(--faint);
  }

  footer {
    margin-top: auto;
    padding-top: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .status {
    justify-content: flex-start;
    gap: 12px;
    width: 100%;
    padding: 11px 12px;
    border-radius: 12px;
    background: var(--surface);
    border: 1px solid var(--line);
    text-align: left;
  }

  .status-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .status-text b {
    font-weight: 600;
    font-size: 13px;
  }

  .status-text span {
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .status.off .status-text span {
    color: var(--accent);
  }

  .pulse,
  .dot {
    position: relative;
    width: 10px;
    height: 10px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--accent);
  }

  .pulse::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 2s var(--ease) infinite;
  }

  .dot {
    background: var(--faint);
  }

  @keyframes pulse {
    from {
      transform: scale(1);
      opacity: 0.6;
    }
    to {
      transform: scale(2.6);
      opacity: 0;
    }
  }

  .usage {
    padding: 0 12px;
    font-size: 11px;
    color: var(--faint);
  }
</style>
