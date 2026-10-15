<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "./api";
  import type { Clip } from "./api";
  import { captures, clipTitle, clock, duration, eventRank, isMultiKill, killSummary, mapName } from "./format";
  import Icon from "./Icon.svelte";

  interface Props {
    clip: Clip;
    selected: boolean;
    /** It just arrived: glows for a moment. */
    fresh?: boolean;
    /** Something is selected, so a click on the thumbnail selects instead of opening. */
    selecting: boolean;
    x: number;
    y: number;
    w: number;
    onselect: (e: MouseEvent) => void;
    ontoggle: () => void;
    onopen: () => void;
    onfavorite: () => void;
  }
  let { clip, selected, fresh = false, selecting, x, y, w, onselect, ontoggle, onopen, onfavorite }: Props = $props();

  let thumb = $state<string | null>(null);
  let loaded = $state(false);

  onMount(() => {
    let mounted = true;
    api.thumbnail(clip, () => mounted).then((url) => {
      if (mounted) thumb = url;
    });
    return () => (mounted = false);
  });

  /** Other notable things in the clip, as small icons. */
  let extras = $derived.by(() => {
    const seen = new Map<string, number>();
    for (const e of clip.events) {
      if (e.kind === clip.label || e.kind === "kill" || /^\d+k$/.test(e.kind)) continue;
      seen.set(e.kind, (seen.get(e.kind) ?? 0) + 1);
    }
    // One press is every hotkey clip; only a burst of presses is worth showing.
    if (seen.get("capture") === 1) seen.delete("capture");
    return [...seen].sort((a, b) => eventRank(b[0]) - eventRank(a[0])).slice(0, 4);
  });

  let place = $derived(clip.map ? mapName(clip.map) : clip.game);
  let presses = $derived(captures(clip));
  let summary = $derived(killSummary(clip) || (presses > 1 ? `Saved ${presses} times · ${place}` : place));
  let badge = $derived(clip.label ?? (clip.kind === "auto" ? null : "manual"));
</script>

<div
  class="card"
  class:selected
  class:fresh
  style="transform: translate({x}px, {y}px); width: {w}px"
  role="option"
  tabindex="-1"
  aria-selected={selected}
  onclick={onselect}
  ondblclick={onopen}
  onkeydown={(e) => e.key === "Enter" && onopen()}
>
  <div class="thumb">
    {#if thumb}<img src={thumb} alt="" draggable="false" class:loaded onload={() => (loaded = true)} />{/if}
    <div class="shade"></div>
    {#if badge}
      <span class="badge" class:hot={isMultiKill(badge)}>
        <Icon event={badge} size={15} />
        {clipTitle(clip)}
      </span>
    {/if}
    {#if !selecting}
      <button
        class="play"
        aria-label="Open"
        onclick={(e) => {
          e.stopPropagation();
          onopen();
        }}><Icon name="play" size={22} /></button
      >
    {/if}
    <button
      class="check"
      class:on={selected}
      aria-label={selected ? "Deselect" : "Select"}
      onclick={(e) => {
        e.stopPropagation();
        ontoggle();
      }}>{#if selected}<Icon name="check" size={14} />{/if}</button
    >
    {#if extras.length > 0}
      <span class="extras">
        {#each extras as [kind, n] (kind)}
          <span class="extra"><Icon event={kind} size={14} />{#if n > 1}<span class="mono">{n}</span>{/if}</span>
        {/each}
      </span>
    {/if}
    {#if clip.durationSecs != null}<span class="duration mono">{duration(clip.durationSecs)}</span>{/if}
  </div>
  <div class="info">
    <div class="line">
      <span class="name">{clipTitle(clip)}</span>
      <button
        class="star"
        class:on={clip.favorite}
        aria-label={clip.favorite ? "Remove from favorites" : "Add to favorites"}
        onclick={(e) => {
          e.stopPropagation();
          onfavorite();
        }}><Icon name="star" size={15} /></button
      >
      <span class="time mono">{clock(clip.created)}</span>
    </div>
    <div class="summary">{summary}</div>
  </div>
</div>

<style>
  .card {
    position: absolute;
    top: 0;
    left: 0;
    cursor: pointer;
    /* Layout only: paint containment would clip the card to its rounded
       shape, shaving the corner off the summary's first letter (the text
       sits at the card's bottom edge). The thumbnail clips itself. */
    contain: layout;
    border-radius: var(--radius);
    outline: 2px solid transparent;
    outline-offset: 3px;
    transition:
      outline-color 0.15s,
      transform 0.3s var(--ease);
  }

  .card.selected {
    outline-color: var(--accent);
  }

  .card.fresh {
    animation: landed 2.4s var(--ease);
  }

  @keyframes landed {
    0% {
      outline-color: var(--accent);
      filter: brightness(1.25);
    }
    60% {
      outline-color: var(--accent);
    }
  }

  .thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--radius);
    background: var(--surface);
    overflow: hidden;
  }

  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    opacity: 0;
    transition:
      opacity 0.25s,
      transform 0.35s var(--ease);
  }

  img.loaded {
    opacity: 1;
  }

  .card:hover img {
    transform: scale(1.035);
  }

  .shade {
    position: absolute;
    inset: 0;
    background: linear-gradient(to bottom, rgba(0, 0, 0, 0.35), transparent 32%, transparent 62%, rgba(0, 0, 0, 0.55));
    pointer-events: none;
  }

  .badge {
    position: absolute;
    top: 9px;
    left: 9px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 9px 4px 6px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 650;
    background: var(--glass);
    backdrop-filter: blur(6px);
  }

  .badge.hot {
    background: var(--accent);
    color: var(--on-accent);
  }

  /* On the accent badge the whole icon is ink. */
  .badge.hot :global(.icon) {
    --accent: var(--on-accent);
  }

  .play {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 48px;
    height: 48px;
    padding: 0;
    border-radius: 50%;
    border: none;
    background: var(--glass);
    backdrop-filter: blur(6px);
    opacity: 0;
    transform: translate(-50%, -50%) scale(0.85);
    transition:
      opacity 0.18s,
      transform 0.18s var(--ease),
      background 0.15s;
  }

  .card:hover .play {
    opacity: 1;
    transform: translate(-50%, -50%) scale(1);
  }

  .play:hover {
    background: var(--accent);
    color: var(--on-accent);
  }

  .check {
    position: absolute;
    top: 9px;
    right: 9px;
    width: 24px;
    height: 24px;
    padding: 0;
    border-radius: 7px;
    border: 2px solid rgba(244, 242, 236, 0.85);
    background: rgba(0, 0, 0, 0.3);
    color: var(--on-accent);
    opacity: 0;
  }

  .card:hover .check,
  .check.on {
    opacity: 1;
  }

  .check.on {
    background: var(--accent);
    border-color: var(--accent);
  }

  .extras {
    position: absolute;
    left: 9px;
    bottom: 9px;
    display: flex;
    gap: 4px;
  }

  .extra {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 3px 5px;
    border-radius: 6px;
    background: var(--glass);
    font-size: 10px;
  }

  .duration {
    position: absolute;
    right: 9px;
    bottom: 9px;
    padding: 3px 7px;
    border-radius: 6px;
    font-size: 11px;
    background: var(--glass);
  }

  .info {
    padding: 10px 2px 0;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .name {
    flex: 1;
    min-width: 0;
    /* Room for the first letter's edge inside the clipped line (as .summary). */
    padding-left: 2px;
    margin-left: -2px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .star {
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--faint);
    opacity: 0;
  }

  .card:hover .star,
  .star.on {
    opacity: 1;
  }

  .star:hover {
    color: var(--text);
    background: transparent;
  }

  .star.on {
    color: var(--accent);
  }

  .star.on :global(path) {
    fill: currentColor;
  }

  .time {
    font-size: 11px;
    color: var(--muted);
  }

  .summary {
    /* Room for the first letter's edge inside the clipped line: on scaled
       displays (125 %, 150 %) text lands between device pixels and its
       antialiased edge reaches past the box. The card's own edge (2px to the
       left) is the outer limit. */
    padding-left: 2px;
    margin-left: -2px;
    margin-top: 3px;
    font-size: 12px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
