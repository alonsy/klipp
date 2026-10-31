<script lang="ts">
  // Where a clip will appear: shown from the hotkey press (or the moment an
  // auto-clip qualifies) until the file lands in the library. It's drawn like
  // the card it turns into, so the clip seems to develop in place. Its outro stays
  // local: a global one keeps the whole library in the page while the editor comes
  // in, and two heroes on screen cancel the view transition.
  import type { Arrival } from "./api";
  import { eventName } from "./format";
  import Icon from "./Icon.svelte";
  import { DUR, enter, leave, spring } from "./motion";

  let { arrival, w }: { arrival: Arrival; w: number } = $props();

  let name = $derived(arrival.kind === "auto" && arrival.label ? eventName(arrival.label, arrival.game) : "Manual clip");
  let detail = $derived(arrival.kind !== "auto" && arrival.presses > 1 ? `Saved ${arrival.presses} times so far` : arrival.game);
</script>

<div
  class="sk"
  style="width: {w}px"
  aria-live="polite"
  aria-label="A clip is arriving"
  role="status"
  in:enter|global={{ x: -16, y: 0, duration: DUR.spring, easing: spring }}
  out:leave
>
  <div class="thumb skeleton">
    <span class="spinner" aria-hidden="true"></span>
    <div class="meta">
      <span class="name"><Icon event={arrival.label ?? "manual"} size={16} />{name}</span>
      <span class="detail">{detail}</span>
    </div>
  </div>
</div>

<style>
  .sk {
    flex-shrink: 0;
  }

  .thumb {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    outline: 1px solid var(--accent-soft);
    outline-offset: -1px;
  }

  .spinner {
    position: absolute;
    top: var(--sp-3);
    right: var(--sp-3);
    color: var(--accent);
  }

  .meta {
    position: absolute;
    left: var(--sp-3);
    right: var(--sp-3);
    bottom: 9px;
    display: flex;
    flex-direction: column;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 20px;
    font-weight: 600;
    font-size: var(--fs-control);
  }

  .detail {
    height: 20px;
    line-height: 20px;
    font-size: var(--fs-sm);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
