<script lang="ts">
  import { onMount } from "svelte";
  import type { TransitionConfig } from "svelte/transition";
  import * as api from "./api";
  import type { Clip } from "./api";
  import { captures, clipTitle, clock, duration, eventRank, killSummary, mapName, when } from "./format";
  import Icon from "./Icon.svelte";
  import { isHero, shownThumbs } from "./state.svelte";

  const still = (_: Element): TransitionConfig => ({ duration: 0 });

  interface Props {
    clip: Clip;
    selected: boolean;
    /** It just arrived: slides in and glows for a moment. */
    fresh?: boolean;
    /** Something is selected, so a click changes the selection instead of opening. */
    selecting: boolean;
    x: number;
    y: number;
    w: number;
    /** Its place in the list, for the staggered entry. */
    index?: number;
    /** A number worth seeing without hovering: what the list is sorted by. */
    stat?: string;
    /** No session header says which day this is, so the card does. */
    dated?: boolean;
    /** The list decides when a card animates: not while scrolling. */
    appear?: (node: Element, params: { index: number }) => TransitionConfig;
    vanish?: (node: Element) => TransitionConfig;
    /** A click on the card: opens it, or selects (see `selecting`, Ctrl, Shift). */
    onselect: (e: MouseEvent) => void;
    ontoggle: () => void;
    onopen: (e?: MouseEvent) => void;
    onfavorite: () => void;
  }
  let {
    clip,
    selected,
    fresh = false,
    selecting,
    x,
    y,
    w,
    index = 0,
    stat,
    dated = false,
    appear = still,
    vanish = still,
    onselect,
    ontoggle,
    onopen,
    onfavorite,
  }: Props = $props();

  let thumb = $state<string | null>(null);
  let asked = $state(false);
  let loaded = $state(false);

  onMount(() => {
    let mounted = true;
    api.thumbnail(clip, () => mounted).then((url) => {
      if (url) shownThumbs.set(clip.path, url);
      if (!mounted) return;
      thumb = url;
      asked = true;
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
  let hero = $derived(isHero(clip.path));
</script>

<div
  class="card"
  class:selected
  class:fresh
  class:selecting
  class:hero
  style="transform: translate({x}px, {y}px); width: {w}px"
  role="option"
  tabindex="-1"
  aria-selected={selected}
  onclick={onselect}
  ondblclick={(e) => {
    // Two quick clicks on the star or the select box are two clicks on those.
    if (!(e.target as Element).closest("button")) onopen(e);
  }}
  onkeydown={(e) => e.key === "Enter" && onopen()}
  in:appear|global={{ index }}
  out:vanish|global
>
  <div class="thumb" data-hero={hero ? "" : undefined}>
    {#if thumb}<img src={thumb} alt="" draggable="false" class:loaded onload={() => (loaded = true)} />{/if}
    {#if !asked && !hero}<div class="waiting skeleton"></div>{/if}
    <div class="shade"></div>

    {#if !selecting}
      <button
        class="play"
        aria-label="Open"
        title="Open"
        onclick={(e) => {
          e.stopPropagation();
          onopen(e);
        }}><Icon name="play" size={20} /></button
      >
    {/if}
    <button
      class="pick"
      class:on={selected}
      aria-label={selected ? "Deselect" : "Select"}
      title={selected ? "Deselect" : "Select"}
      onclick={(e) => {
        e.stopPropagation();
        ontoggle();
      }}>{#if selected}<Icon name="check" size={14} />{/if}</button
    >
    <button
      class="star"
      class:on={clip.favorite}
      aria-label={clip.favorite ? "Remove from favorites" : "Add to favorites"}
      title={clip.favorite ? "Remove from favorites" : "Add to favorites"}
      onclick={(e) => {
        e.stopPropagation();
        onfavorite();
      }}><Icon name="star" size={16} /></button
    >

    <!-- The name rests on the bottom edge; hovering lifts it to show the rest. -->
    <div class="meta">
      <div class="line">
        <span class="name">
          {#if badge}<Icon event={badge} size={16} />{/if}
          <span class="text">{clipTitle(clip)}</span>
        </span>
        {#if extras.length > 0}
          <span class="extras">
            {#each extras as [kind, n] (kind)}
              <span class="extra"><Icon event={kind} size={14} />{#if n > 1}<span class="mono">{n}</span>{/if}</span>
            {/each}
          </span>
        {/if}
        {#if stat}<span class="stat mono">{stat}</span>{/if}
        {#if clip.camera}<span class="camera" title="Has your camera"><Icon name="camera" size={14} /></span>{/if}
      </div>
      <div class="more">
        <span class="summary">{summary}</span>
        <span class="when mono">{dated ? when(clip.created) : clock(clip.created)}{#if clip.durationSecs != null}&nbsp;· {duration(clip.durationSecs)}{/if}</span>
      </div>
    </div>
  </div>
</div>

<style>
  .card {
    position: absolute;
    top: 0;
    left: 0;
    cursor: pointer;
    contain: layout;
    border-radius: var(--r-md);
    /* The list keeps cards it can: a filter or a new clip glides them to their place. */
    transition: transform var(--dur-spring) var(--spring);
  }

  .thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    background: var(--surface);
    overflow: hidden;
    outline: 2px solid transparent;
    outline-offset: 2px;
    transition:
      transform var(--dur) var(--ease-out),
      outline-color var(--dur-fast);
  }

  .card:hover .thumb {
    transform: translateY(-2px);
  }

  .card:active .thumb {
    transform: scale(0.99);
  }

  .card.selected .thumb {
    outline-color: var(--accent);
  }

  .card:focus-visible {
    outline: none;
  }

  .card:focus-visible .thumb {
    outline-color: var(--text);
  }

  /* A new clip takes the placeholder's spot; the one you came back to only glows. */
  .card.fresh .thumb {
    animation:
      arrive var(--dur-spring) var(--spring) backwards,
      landed 2.4s var(--ease-out);
  }

  .card.fresh.hero .thumb {
    animation: landed 2.4s var(--ease-out);
  }

  @keyframes arrive {
    from {
      opacity: 0;
      transform: translateX(-16px) scale(0.97);
    }
  }

  @keyframes landed {
    0%,
    60% {
      outline-color: var(--accent);
    }
  }

  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    opacity: 0;
    transition:
      opacity var(--dur),
      transform var(--dur-slow) var(--ease-out);
  }

  img.loaded {
    opacity: 1;
  }

  /* It's what the player shrinks into: there from the first frame. */
  .hero img {
    opacity: 1;
    transition: none;
  }

  .card:hover img {
    transform: scale(1.04);
  }

  .waiting {
    position: absolute;
    inset: 0;
    border-radius: 0;
  }

  /* The bottom edge stays dark enough to read the name; hovering deepens the picture. */
  .shade {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: linear-gradient(to top, color-mix(in srgb, var(--stage) 72%, transparent), transparent 42%);
  }

  .shade::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(to top, color-mix(in srgb, var(--stage) 82%, transparent), color-mix(in srgb, var(--stage) 18%, transparent) 70%);
    opacity: 0;
    transition: opacity var(--dur);
  }

  .card:hover .shade::after,
  .card:has(:focus-visible) .shade::after {
    opacity: 1;
  }

  .play {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 44px;
    height: 44px;
    margin: -26px 0 0 -22px;
    padding: 0;
    border-radius: 50%;
    background: var(--glass);
    backdrop-filter: blur(6px);
    opacity: 0;
    transform: scale(0.88);
    transition:
      opacity var(--dur-fast),
      transform var(--dur) var(--ease-out),
      background var(--dur-fast),
      color var(--dur-fast);
  }

  .card:hover .play,
  .play:focus-visible {
    opacity: 1;
    transform: none;
  }

  .play:hover {
    background: var(--accent);
    color: var(--on-accent);
  }

  .play:active {
    transform: scale(0.94);
  }

  /* Quick actions, in the corners. What's set stays when the pointer leaves. */
  .pick,
  .star {
    position: absolute;
    top: var(--sp-2);
    padding: 0;
    opacity: 0;
    transition:
      opacity var(--dur-fast),
      background var(--dur-fast),
      color var(--dur-fast),
      transform var(--dur) var(--ease-out);
  }

  .pick {
    left: var(--sp-2);
    width: 24px;
    height: 24px;
    border-radius: var(--r-xs);
    background: var(--glass);
    box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--text) 80%, transparent);
    color: var(--on-accent);
  }

  .pick:hover {
    background: var(--glass);
    box-shadow: inset 0 0 0 1.5px var(--text);
  }

  .card:hover .pick,
  .selecting .pick,
  .pick:focus-visible {
    opacity: 1;
  }

  .pick.on,
  .pick.on:hover {
    opacity: 1;
    background: var(--accent);
    box-shadow: none;
  }

  .star {
    right: var(--sp-2);
    width: 28px;
    height: 28px;
    margin: -2px -2px 0 0;
    border-radius: 50%;
    background: transparent;
    color: var(--text);
    filter: drop-shadow(0 1px 2px color-mix(in srgb, var(--stage) 70%, transparent));
  }

  .star:hover {
    background: var(--glass);
  }

  .card:hover .star,
  .star:focus-visible,
  .star.on {
    opacity: 1;
  }

  .star.on {
    color: var(--accent);
  }

  .star.on :global(path) {
    fill: currentColor;
  }

  .meta {
    --reveal: 20px;
    position: absolute;
    left: var(--sp-3);
    right: var(--sp-3);
    bottom: 9px;
    pointer-events: none;
    transform: translateY(var(--reveal));
    transition: transform var(--dur) var(--ease-out);
  }

  .card:hover .meta,
  .card:has(:focus-visible) .meta {
    transform: none;
  }

  .line {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    height: 20px;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    font-weight: 600;
    font-size: var(--fs-control);
    text-shadow: 0 1px 2px color-mix(in srgb, var(--stage) 60%, transparent);
  }

  .text {
    /* Room for the first letter's edge inside the clipped line: on scaled
       displays (125 %, 150 %) text lands between device pixels. */
    padding-left: 2px;
    margin-left: -2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .extras {
    display: flex;
    gap: var(--sp-2);
    flex-shrink: 0;
    opacity: 0;
    transition: opacity var(--dur-fast);
  }

  .extra {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--fs-xs);
  }

  .stat {
    flex-shrink: 0;
    font-size: var(--fs-xs);
    text-shadow: 0 1px 2px color-mix(in srgb, var(--stage) 60%, transparent);
  }

  .more {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    height: 20px;
    font-size: var(--fs-sm);
    color: color-mix(in srgb, var(--text) 78%, transparent);
    opacity: 0;
    transition: opacity var(--dur-fast);
  }

  .card:hover .more,
  .card:hover .extras,
  .card:has(:focus-visible) .more,
  .card:has(:focus-visible) .extras {
    opacity: 1;
    transition-duration: var(--dur);
  }

  /* Part of the card at rest: it says which clips can have you in their short. */
  .camera {
    display: inline-flex;
    flex-shrink: 0;
    pointer-events: auto;
    filter: drop-shadow(0 1px 2px color-mix(in srgb, var(--stage) 60%, transparent));
  }

  .summary {
    flex: 1;
    min-width: 0;
    padding-left: 2px;
    margin-left: -2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .when {
    flex-shrink: 0;
    font-size: var(--fs-xs);
  }
</style>
