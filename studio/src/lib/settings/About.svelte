<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount } from "svelte";
  import * as api from "../api";
  import Icon from "../Icon.svelte";
  import Logo from "../Logo.svelte";
  import { library } from "../state.svelte";

  let version = $state("");
  onMount(() => {
    getVersion().then((v) => (version = v), () => {});
  });
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

  .credits {
    margin: 0 4px;
    font-size: 12px;
    color: var(--faint);
  }
</style>
