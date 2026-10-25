<script lang="ts">
  // Where a clip will appear: shown from the hotkey press (or the moment an
  // auto-clip qualifies) until the file lands in the library.
  import type { Arrival } from "./api";
  import { eventName } from "./format";
  import Icon from "./Icon.svelte";

  let { arrival, w }: { arrival: Arrival; w: number } = $props();

  let detail = $derived(
    arrival.kind === "auto" && arrival.label
      ? `${eventName(arrival.label, arrival.game)} · ${arrival.game}`
      : arrival.presses > 1
        ? `Saved ${arrival.presses} times so far`
        : arrival.game,
  );
</script>

<div class="sk" style="width: {w}px" aria-live="polite">
  <div class="thumb">
    <div class="shine"></div>
    <div class="center">
      <span class="icon"><Icon event={arrival.label ?? "manual"} size={22} /></span>
      <span class="title">A clip is arriving</span>
      <span class="detail">{detail}</span>
    </div>
  </div>
  <div class="info">
    <span class="bar"></span>
    <span class="bar short"></span>
  </div>
</div>

<style>
  .sk {
    flex-shrink: 0;
    animation: appear 0.3s var(--ease);
  }

  .thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px dashed var(--line-strong);
    overflow: hidden;
  }

  /* A soft band sweeping across, like a placeholder loading. */
  .shine {
    position: absolute;
    inset: 0;
    background: linear-gradient(100deg, transparent 20%, rgba(244, 242, 236, 0.06) 50%, transparent 80%);
    background-size: 220% 100%;
    animation: sweep 1.6s linear infinite;
  }

  .center {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    text-align: center;
    padding: 0 12px;
  }

  .icon {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    margin-bottom: 4px;
    border-radius: 50%;
    background: var(--accent-soft);
    animation: breathe 1.6s ease-in-out infinite;
  }

  .title {
    font-weight: 600;
  }

  .detail {
    font-size: 12px;
    color: var(--muted);
  }

  .info {
    display: flex;
    flex-direction: column;
    gap: 7px;
    padding: 13px 2px 0;
  }

  .bar {
    height: 10px;
    width: 55%;
    border-radius: 5px;
    background: var(--surface);
  }

  .bar.short {
    width: 35%;
  }

  @keyframes sweep {
    from {
      background-position: 120% 0;
    }
    to {
      background-position: -120% 0;
    }
  }

  @keyframes breathe {
    50% {
      transform: scale(1.08);
      background: color-mix(in srgb, var(--accent) 26%, transparent);
    }
  }

  @keyframes appear {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
