<script lang="ts">
  // "Klipp 0.2.1 is ready": checks GitHub releases when the window opens (and
  // every few hours while it stays open, or right away when the tray's
  // "Update to Klipp …" opened it). One click downloads the signed installer,
  // which runs without a window of its own (the updater's "quiet" mode): this
  // card shows the download and then "Installing…", Klipp closes, and it
  // opens again on the new version. Updating restarts the recorder, which
  // clears the replay buffer, so the card says so, and warns while CS2 is running.
  // On the test channel (Settings → About) test builds come here too, and
  // leaving it offers the latest normal release.
  import type { Update } from "@tauri-apps/plugin-updater";
  import { onMount } from "svelte";
  import * as api from "./api";
  import Icon from "./Icon.svelte";
  import { recorder, toast } from "./state.svelte";

  const RECHECK_MS = 6 * 60 * 60 * 1000;

  let update = $state<Update | null>(null);
  let progress = $state<number | null>(null);
  let installing = $state(false);
  let inGame = $state(false);

  onMount(() => {
    // Development builds aren't released, so there's nothing to compare against.
    if (!import.meta.env.PROD) return;
    const look = () =>
      api.checkUpdate().then(
        (u) => {
          // A channel switch can take back an offer that hasn't started downloading.
          if (progress == null) update = u;
        },
        () => {},
      );
    look();
    const timer = setInterval(look, RECHECK_MS);
    window.addEventListener("klipp:check-update", look);
    return () => {
      clearInterval(timer);
      window.removeEventListener("klipp:check-update", look);
    };
  });

  // Only while the card is up.
  $effect(() => {
    if (!update) return;
    const poll = () => api.gameRunning().then((r) => (inGame = r), () => {});
    poll();
    const timer = setInterval(poll, 15000);
    return () => clearInterval(timer);
  });

  const isTest = (v: string) => v.includes("-test");
  // From a test build to a normal release (often a lower number).
  const leavingTests = $derived(update != null && isTest(update.currentVersion) && !isTest(update.version));

  async function install() {
    if (!update) return;
    let total = 0;
    let received = 0;
    progress = 0;
    try {
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") {
          received += e.data.chunkLength;
          progress = total ? received / total : 0;
        } else if (e.event === "Finished") {
          progress = 1;
          installing = true;
        }
      });
    } catch (e) {
      progress = null;
      installing = false;
      toast(`Couldn't update: ${e}`);
    }
  }
</script>

{#if update}
  <div class="update">
    <div class="head">
      <Icon name="bolt" size={16} />
      <b>{isTest(update.version) ? `Test build ${update.version}` : `Klipp ${update.version}`} is ready</b>
    </div>
    {#if leavingTests && progress == null}
      <span>Back to normal releases, instead of test build {update.currentVersion}.</span>
    {/if}
    {#if progress == null}
      {#if inGame}
        <span class="warn">CS2 is running. Updating restarts recording and clears the last {recorder.lengthSecs} seconds, so it's best after your match.</span>
        <button onclick={install}>Update anyway</button>
      {:else}
        <span>Takes a few seconds. Recording restarts, so the last {recorder.lengthSecs} seconds are cleared.</span>
        <button class="primary" onclick={install}>Update now</button>
      {/if}
    {:else if installing}
      <span>Installing… Klipp closes and opens again in a few seconds.</span>
      <div class="bar"><div class="fill busy"></div></div>
    {:else}
      <span class="mono">Downloading… {Math.round(progress * 100)}%</span>
      <div class="bar"><div class="fill" style="width: {progress * 100}%"></div></div>
    {/if}
  </div>
{/if}

<style>
  .update {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-radius: 12px;
    background: var(--accent-soft);
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
    animation: in 0.3s var(--ease);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
  }

  .head :global(.icon) {
    color: var(--accent);
  }

  span {
    font-size: 12px;
    color: var(--muted);
  }

  span.warn {
    color: var(--text);
  }

  button {
    padding: 6px 12px;
    font-size: 13px;
  }

  .bar {
    height: 4px;
    border-radius: 2px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s linear;
  }

  /* Installing has no progress to report: a sweep instead. */
  .fill.busy {
    width: 35%;
    animation: sweep 1.1s ease-in-out infinite;
  }

  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(290%);
    }
  }

  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
