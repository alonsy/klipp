<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { flushSync, onMount, tick, untrack } from "svelte";
  import * as api from "./lib/api";
  import ArrivingPill from "./lib/ArrivingPill.svelte";
  import { installByItself } from "./lib/autoupdate";
  import CloseNotice from "./lib/CloseNotice.svelte";
  import CreatorCenter from "./lib/creator/CreatorCenter.svelte";
  import ShortEditor from "./lib/creator/ShortEditor.svelte";
  import Editor from "./lib/Editor.svelte";
  import Library from "./lib/Library.svelte";
  import { viewTransition } from "./lib/motion";
  import Setup from "./lib/setup/Setup.svelte";
  import SettingsPage from "./lib/settings/SettingsPage.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Toasts from "./lib/Toasts.svelte";
  import { installTooltips } from "./lib/ui/tooltip";
  import WindowControls from "./lib/WindowControls.svelte";
  import { library, loadPrefs, openClip, openPage, prefs, recorder, refreshRecorder, reload, setArrivals, setPref, toast, ui } from "./lib/state.svelte";

  let version = $state("");
  /** Setup, or the PC test after an update, is done: it isn't shown again for this version. */
  const tested = () => version && setPref("pcTestShownFor", version);

  // What state asks for, and what's on screen. They differ for a moment on every
  // change: the old view has to still be there for the transition to picture it.
  const wanted = $derived(
    ui.page === "setup" || ui.page === "pctest" ? ui.page : ui.short ? `short:${ui.short.path}` : ui.clip ? `editor:${ui.clip.path}` : ui.page,
  );
  let view = $state(untrack(() => wanted));
  /** The clip stays in the editor until the view has changed, even with ui.clip gone. */
  let editing = $state.raw(untrack(() => ui.clip));
  const shownClip = $derived(ui.clip ?? editing);
  /** The same for the short editor. */
  let shorting = $state.raw(untrack(() => ui.short));
  const shownShort = $derived(ui.short ?? shorting);
  /** What to show first is known (a fresh install starts in setup, not the library). */
  let ready = $state(false);

  /** The clip's image in the library is in place, for the way back from the editor. */
  async function heroLanded() {
    const until = performance.now() + 120;
    while (performance.now() < until) {
      const img = document.querySelector<HTMLImageElement>("[data-hero] img");
      if (img?.complete) return;
      await new Promise((r) => setTimeout(r, 12));
    }
  }

  $effect(() => {
    const next = wanted;
    if (next === untrack(() => view)) return;
    const change = () => {
      editing = ui.clip;
      shorting = ui.short;
      view = untrack(() => wanted);
    };
    if (!ready) {
      change();
      return;
    }
    const from = untrack(() => view);
    // A clip's picture only travels between the library and the editor. Shorts
    // cover the window too, so the sidebar slides away for them as well.
    const closing = from.startsWith("editor:") && !next.startsWith("short:");
    const hero = /^(editor|short):/.test(from) || /^(editor|short):/.test(next);
    viewTransition(async () => {
      change();
      if (!closing) return;
      flushSync();
      await tick();
      await heroLanded();
    }, hero ? "hero" : "page").then(() => {
      if (!ui.clip) ui.hero = null;
    });
  });

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
    const prefsLoaded = loadPrefs();
    refreshRecorder();
    const needed = api.setupNeeded();
    needed.then((needed) => {
      recorder.autoRestart = !needed;
      if (needed) {
        ui.setupRerun = false;
        ui.page = "setup";
      }
    });
    const request = api.startupRequest();
    Promise.all([reload(), request]).then(([, request]) => handle(request));
    // Updated from a version without the PC test: it shows once,
    // unless the window was opened for something else (a clip, an update).
    const firstPage = Promise.all([needed, request, prefsLoaded, getVersion()]).then(([needed, request, , v]) => {
      version = v;
      if (!needed && !request.clip && !request.page && !prefs.pcTestShownFor) ui.page = "pctest";
    });
    firstPage.catch(() => {}).finally(() => (ready = true));
    api.arriving().then(setArrivals, () => {});
    const offs = [api.onOpenRequest(handle), api.onCloseRequested(() => (ui.closeNotice = true)), api.onArrivals(setArrivals)];
    // Pick up clips saved while Klipp was in the background.
    const onFocus = () => {
      reload();
      refreshRecorder();
    };
    window.addEventListener("focus", onFocus);
    const poll = setInterval(refreshRecorder, 5000);
    const offTooltips = installTooltips();
    return () => {
      window.removeEventListener("focus", onFocus);
      clearInterval(poll);
      offs.forEach((p) => p.then((off) => off()));
      offTooltips();
    };
  });
</script>

{#if !ready}
  <div class="boot" data-tauri-drag-region></div>
{:else if view === "setup"}
  <Setup rerun={ui.setupRerun} ondone={tested} />
{:else if view === "pctest"}
  <Setup pcOnly ondone={tested} />
{:else if view.startsWith("short:") && shownShort}
  {#key view}
    <ShortEditor clip={shownShort} from={ui.shortFrom} />
  {/key}
{:else if view.startsWith("editor:") && shownClip}
  {#key view}
    <Editor clip={shownClip} />
  {/key}
{:else}
  <div class="app">
    <Sidebar />
    <main>
      <!-- The title bar: empty space drags the window. -->
      <div class="topbar" data-tauri-drag-region><WindowControls /></div>
      <div class="page shell-page">
        {#if view === "settings"}
          <SettingsPage />
        {:else if view === "creator"}
          <CreatorCenter />
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
  .boot {
    height: 100%;
  }

  .app {
    display: grid;
    grid-template-columns: var(--sidebar) minmax(0, 1fr);
    height: 100%;
    animation: fade var(--dur) var(--ease-out) backwards;
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
