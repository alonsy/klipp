<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../Icon.svelte";
  import { ui } from "../state.svelte";
  import About from "./About.svelte";
  import Audio from "./Audio.svelte";
  import AutoClips from "./AutoClips.svelte";
  import General from "./General.svelte";
  import Playback from "./Playback.svelte";
  import Recording from "./Recording.svelte";
  import Sounds from "./Sounds.svelte";
  import Storage from "./Storage.svelte";
  import { load, store } from "./store.svelte";
  import Video from "./Video.svelte";

  const sections = [
    { id: "general", name: "General", icon: "sliders" },
    { id: "recording", name: "Recording", icon: "record" },
    { id: "video", name: "Video", icon: "film" },
    { id: "audio", name: "Audio", icon: "mic" },
    { id: "autoclips", name: "Auto-clips", icon: "bolt" },
    { id: "sounds", name: "Sounds", icon: "bell" },
    { id: "storage", name: "Storage", icon: "drive" },
    { id: "playback", name: "Playback", icon: "play" },
    { id: "about", name: "About", icon: "info" },
  ];

  onMount(load);

  let current = $derived(sections.find((s) => s.id === ui.section) ?? sections[0]);
</script>

<section class="settings">
  <nav>
    <h1>Settings</h1>
    {#each sections as s (s.id)}
      <button class="item" class:active={current.id === s.id} onclick={() => (ui.section = s.id)}>
        <Icon name={s.icon} size={17} />{s.name}
      </button>
    {/each}
  </nav>

  <div class="content">
    <header>
      <h2>{current.name}</h2>
    </header>
    {#if store.error}
      <p class="error"><Icon name="warning" size={18} />{store.error}</p>
    {:else if store.s}
      <div class="scroll">
        {#key current.id}
          <div class="page">
            {#if current.id === "general"}<General />
            {:else if current.id === "recording"}<Recording />
            {:else if current.id === "video"}<Video />
            {:else if current.id === "audio"}<Audio />
            {:else if current.id === "autoclips"}<AutoClips />
            {:else if current.id === "sounds"}<Sounds />
            {:else if current.id === "storage"}<Storage />
            {:else if current.id === "playback"}<Playback />
            {:else}<About />{/if}
          </div>
        {/key}
      </div>
    {/if}
  </div>
</section>

<style>
  .settings {
    display: grid;
    grid-template-columns: 210px 1fr;
    height: 100%;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 2px 12px 22px;
    border-right: 1px solid var(--line);
  }

  h1 {
    margin: 0 0 16px 12px;
    font-family: var(--font-display);
    font-size: 22px;
    letter-spacing: -0.03em;
  }

  .item {
    justify-content: flex-start;
    gap: 11px;
    padding: 8px 12px;
    border: none;
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
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

  .content {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  header {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 4px 36px 10px;
  }

  h2 {
    margin: 0;
    font-size: 20px;
    font-weight: 650;
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
  }

  .page {
    display: flex;
    flex-direction: column;
    gap: 22px;
    max-width: 760px;
    padding: 8px 36px 48px;
    animation: in 0.2s var(--ease);
  }

  .page :global(.group) {
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--line);
  }

  .page :global(.group-title) {
    display: block;
    margin: 0 0 -12px 4px;
  }

  .page :global(.banner) {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    border-radius: 12px;
    background: color-mix(in srgb, #f2c84b 9%, transparent);
    border: 1px solid color-mix(in srgb, #f2c84b 25%, transparent);
    color: #f5dc8c;
    font-size: 13px;
    line-height: 1.5;
  }

  .page :global(.value) {
    min-width: 64px;
    text-align: right;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text);
  }

  .error {
    display: flex;
    gap: 10px;
    margin: 20px 36px;
    color: var(--danger);
  }

  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
