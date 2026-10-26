<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "./lib/api";
  import ArrivingPill from "./lib/ArrivingPill.svelte";
  import { installByItself } from "./lib/autoupdate";
  import CloseNotice from "./lib/CloseNotice.svelte";
  import Editor from "./lib/Editor.svelte";
  import Library from "./lib/Library.svelte";
  import Setup from "./lib/setup/Setup.svelte";
  import SettingsPage from "./lib/settings/SettingsPage.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Toasts from "./lib/Toasts.svelte";
  import WindowControls from "./lib/WindowControls.svelte";
  import { library, loadPrefs, openClip, openPage, recorder, refreshRecorder, reload, setArrivals, toast, ui } from "./lib/state.svelte";

  async function handle(request: api.OpenRequest) {
    if (request.page === "settings") openPage("settings");
    // The tray's "Update to Klipp …": the update card (sidebar) checks now.
    if (request.page === "update") {
      openPage("clips");
      window.dispatchEvent(new Event("klipp:check-update"));
    }
    // A test build's recorder, between games: install the next one without asking.
    if (request.page === "install-update") installByItself();
    if (request.clip) {
      await reload();
      const want = request.clip.toLowerCase();
      const clip = library.clips.find((c) => c.path.toLowerCase() === want);
      if (clip) openClip(clip);
      else toast("That clip isn't in your clips folder.");
    }
  }

  onMount(() => {
    loadPrefs();
    refreshRecorder();
    api.setupNeeded().then((needed) => {
      recorder.autoRestart = !needed;
      if (needed) {
        ui.setupRerun = false;
        ui.page = "setup";
      }
    });
    Promise.all([reload(), api.startupRequest()]).then(([, request]) => handle(request));
    api.arriving().then(setArrivals, () => {});
    const offs = [api.onOpenRequest(handle), api.onCloseRequested(() => (ui.closeNotice = true)), api.onArrivals(setArrivals)];
    // Pick up clips saved while Klipp was in the background.
    const onFocus = () => {
      reload();
      refreshRecorder();
    };
    window.addEventListener("focus", onFocus);
    const poll = setInterval(refreshRecorder, 5000);
    return () => {
      window.removeEventListener("focus", onFocus);
      clearInterval(poll);
      offs.forEach((p) => p.then((off) => off()));
    };
  });
</script>

{#if ui.page === "setup"}
  <Setup rerun={ui.setupRerun} />
{:else if ui.clip}
  {#key ui.clip.path}
    <Editor clip={ui.clip} />
  {/key}
{:else}
  <div class="app">
    <Sidebar />
    <main>
      <!-- The title bar: empty space drags the window. -->
      <div class="topbar" data-tauri-drag-region><WindowControls /></div>
      <div class="page">
        {#if ui.page === "settings"}
          <SettingsPage />
        {:else}
          <Library />
        {/if}
      </div>
    </main>
  </div>
{/if}
<ArrivingPill />
<Toasts />
{#if ui.closeNotice}
  <CloseNotice />
{/if}

<style>
  .app {
    display: grid;
    grid-template-columns: 236px 1fr;
    height: 100%;
  }

  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    height: 100%;
    overflow: hidden;
  }

  .topbar {
    display: flex;
    justify-content: flex-end;
    flex-shrink: 0;
    height: 36px;
  }

  .page {
    flex: 1;
    min-height: 0;
  }
</style>
