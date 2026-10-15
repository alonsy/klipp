<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import * as api from "./api";
  import type { Clip } from "./api";
  import ArrivingCard from "./ArrivingCard.svelte";
  import ClipCard from "./ClipCard.svelte";
  import { clock, day, eventName, eventRank, mapName, modeName, size } from "./format";
  import Icon from "./Icon.svelte";
  import { groupSessions, type Session } from "./sessions";
  import {
    deleteClips,
    filters,
    incoming,
    inScope,
    library,
    matches,
    openClip,
    prefs,
    recorder,
    reload,
    setFavorite,
    setPref,
    sorted,
    type Kind,
    type Sort,
  } from "./state.svelte";

  // Layout (px).
  const PAD = 28;
  const GAP = 16;
  const MIN_CARD = 250;
  const INFO_H = 54;
  const HEADER_H = 58;
  const SESSION_SPACE = 14;

  let visible = $derived(sorted(library.clips.filter(matches)));
  /** Sessions only make sense in time order. */
  let grouped = $derived(prefs.groupSessions && (filters.sort === "newest" || filters.sort === "oldest"));
  let sessions = $derived<Session[]>(
    grouped
      ? groupSessions(visible, filters.sort === "newest")
      : [{ key: "", game: "", map: "", mode: "", start: 0, end: 0, clips: visible }],
  );
  /** Clips on their way, shown first while the newest are on top. */
  let arriving = $derived(
    filters.sort !== "newest" || filters.kind === "favorites"
      ? []
      : incoming.list.filter((a) => (!filters.game || a.game === filters.game) && (filters.kind === "all" || a.kind === filters.kind)),
  );
  /** Clips in the order they're shown, for Shift-click ranges. */
  let flat = $derived(sessions.flatMap((s) => s.clips));

  // Event chips: what's in the current folder/kind/search, most notable first.
  const CHIP_EVENTS = /^(\d+k|ace|headshot|knife|zeus|mvp|death)$/;
  let chips = $derived.by(() => {
    const m = new Map<string, number>();
    for (const c of library.clips) {
      if (!inScope(c)) continue;
      const kinds = new Set([...(c.label ? [c.label] : []), ...c.events.map((e) => e.kind)]);
      for (const k of kinds) if (CHIP_EVENTS.test(k)) m.set(k, (m.get(k) ?? 0) + 1);
    }
    return [...m].sort((a, b) => eventRank(b[0]) - eventRank(a[0]));
  });

  const selected = new SvelteSet<string>();
  let anchor: number | null = null;

  // Forget selections that were deleted or filtered away.
  $effect(() => {
    const shown = new Set(flat.map((c) => c.path));
    for (const p of [...selected]) if (!shown.has(p)) selected.delete(p);
  });

  // Rows of cards and session headers; only those in view are rendered.
  let scroller: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);
  let cols = $derived(Math.max(1, Math.floor((width - 2 * PAD + GAP) / (MIN_CARD + GAP))));
  // Whole pixels: text on a fractional position renders blurry and its edges get clipped.
  let cardW = $derived(Math.max(1, Math.floor((width - 2 * PAD - GAP * (cols - 1)) / cols)));
  let cardH = $derived(Math.round((cardW * 9) / 16) + INFO_H);

  type Row =
    | { kind: "arriving"; key: string; y: number; h: number }
    | { kind: "header"; key: string; y: number; h: number; session: Session }
    | { kind: "cards"; key: string; y: number; h: number; clips: Clip[]; first: number };

  let layout = $derived.by(() => {
    const rows: Row[] = [];
    let y = grouped ? 6 : PAD - 6;
    let index = 0;
    if (arriving.length > 0) {
      const h = (grouped ? HEADER_H : 0) + cardH + GAP + (grouped ? SESSION_SPACE : 0);
      rows.push({ kind: "arriving", key: "arriving", y, h });
      y += h;
    }
    for (const s of sessions) {
      if (grouped) {
        rows.push({ kind: "header", key: `h:${s.key}`, y, h: HEADER_H, session: s });
        y += HEADER_H;
      }
      for (let i = 0; i < s.clips.length; i += cols) {
        rows.push({ kind: "cards", key: `r:${s.key}:${i}`, y, h: cardH + GAP, clips: s.clips.slice(i, i + cols), first: index + i });
        y += cardH + GAP;
      }
      index += s.clips.length;
      y += grouped ? SESSION_SPACE : 0;
    }
    return { rows, height: y + PAD };
  });

  let inView = $derived(layout.rows.filter((r) => r.y + r.h >= scrollTop - 200 && r.y <= scrollTop + height + 200));
  // Cards are keyed by clip, not by row, so a filter or resize moves the
  // cards that stay (they glide to their new place) instead of rebuilding them.
  type Item =
    | { kind: "arriving"; key: string; y: number }
    | { kind: "header"; key: string; y: number; session: Session }
    | { kind: "card"; key: string; y: number; x: number; clip: Clip; index: number };
  let items = $derived(
    inView.flatMap((row): Item[] =>
      row.kind === "arriving"
        ? [{ kind: "arriving", key: row.key, y: row.y }]
        : row.kind === "header"
        ? [{ kind: "header", key: row.key, y: row.y, session: row.session }]
        : row.clips.map((clip, col) => ({ kind: "card", key: clip.path, y: row.y, x: PAD + col * (cardW + GAP), clip, index: row.first + col })),
    ),
  );

  let selectedClips = $derived(flat.filter((c) => selected.has(c.path)));
  let selectedSize = $derived(selectedClips.reduce((s, c) => s + c.sizeBytes, 0));
  let allFavorite = $derived(selectedClips.length > 0 && selectedClips.every((c) => c.favorite));

  let title = $derived(filters.game ?? (filters.kind === "favorites" ? "Favorites" : "All clips"));

  function select(e: MouseEvent, i: number) {
    const path = flat[i].path;
    if (e.shiftKey && anchor != null) {
      if (!e.ctrlKey) selected.clear();
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      for (let k = a; k <= b; k++) selected.add(flat[k].path);
    } else if (e.ctrlKey) {
      toggle(i);
    } else {
      selected.clear();
      selected.add(path);
      anchor = i;
    }
  }

  function toggle(i: number) {
    const path = flat[i].path;
    if (selected.has(path)) selected.delete(path);
    else selected.add(path);
    anchor = i;
  }

  function selectSession(s: Session) {
    const all = s.clips.every((c) => selected.has(c.path));
    for (const c of s.clips) {
      if (all) selected.delete(c.path);
      else selected.add(c.path);
    }
  }

  // Drag a box over empty space to select (hold Ctrl to add to the selection).
  let drag = $state<{ x0: number; y0: number; x1: number; y1: number; base: string[] } | null>(null);

  function contentPoint(e: PointerEvent) {
    const r = scroller.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top + scroller.scrollTop };
  }

  function dragStart(e: PointerEvent) {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".card, button")) return;
    const p = contentPoint(e);
    drag = { x0: p.x, y0: p.y, x1: p.x, y1: p.y, base: e.ctrlKey ? [...selected] : [] };
    if (!e.ctrlKey) selected.clear();
    scroller.setPointerCapture(e.pointerId);
  }

  function dragMove(e: PointerEvent) {
    if (!drag) return;
    const r = scroller.getBoundingClientRect();
    // Scroll when dragging near the top or bottom edge.
    if (e.clientY > r.bottom - 40) scroller.scrollTop += 24;
    else if (e.clientY < r.top + 40) scroller.scrollTop -= 24;
    const p = contentPoint(e);
    drag.x1 = p.x;
    drag.y1 = p.y;
    const [left, right] = [Math.min(drag.x0, drag.x1), Math.max(drag.x0, drag.x1)];
    const [top, bottom] = [Math.min(drag.y0, drag.y1), Math.max(drag.y0, drag.y1)];
    selected.clear();
    for (const p of drag.base) selected.add(p);
    for (const row of layout.rows) {
      if (row.kind !== "cards" || row.y > bottom || row.y + cardH < top) continue;
      row.clips.forEach((c, col) => {
        const x = PAD + col * (cardW + GAP);
        if (x < right && x + cardW > left) selected.add(c.path);
      });
    }
  }

  function dragEnd(e: PointerEvent) {
    if (!drag) return;
    drag = null;
    scroller.releasePointerCapture(e.pointerId);
  }

  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === "INPUT" || tag === "SELECT") return;
    if (e.ctrlKey && e.key.toLowerCase() === "a") {
      e.preventDefault();
      for (const c of flat) selected.add(c.path);
    } else if (e.key === "Delete") {
      deleteClips([...selected]);
    } else if (e.key === "Escape") {
      selected.clear();
    } else if (e.key === "Enter" && selectedClips.length > 0) {
      openClip(selectedClips[0]);
    }
  }

  function sessionTitle(s: Session) {
    return s.map ? mapName(s.map) : s.game;
  }

  function sessionTime(s: Session) {
    const start = clock(s.start);
    const end = clock(s.end);
    return start === end ? `${day(s.start)} · ${start}` : `${day(s.start)} · ${start} – ${end}`;
  }

  const kinds: [Kind, string][] = [
    ["all", "All"],
    ["auto", "Auto"],
    ["manual", "Manual"],
  ];
</script>

<svelte:window onkeydown={onKey} />

<section class="library">
  <header>
    <div class="top">
      <div class="title">
        <h1>{title}</h1>
        <span class="sub mono">{visible.length} {visible.length === 1 ? "clip" : "clips"}</span>
      </div>
      <label class="search">
        <Icon name="search" size={16} />
        <input type="search" placeholder="Search clips" bind:value={filters.search} />
      </label>
      <select bind:value={filters.sort} aria-label="Sort">
        {#each [["newest", "Newest first"], ["oldest", "Oldest first"], ["longest", "Longest first"], ["largest", "Largest first"]] as [v, name] (v)}
          <option value={v as Sort}>{name}</option>
        {/each}
      </select>
      <button
        class="icon"
        class:on={prefs.groupSessions}
        title={prefs.groupSessions ? "Grouped by session" : "Not grouped"}
        onclick={() => setPref("groupSessions", !prefs.groupSessions)}><Icon name="film" /></button
      >
      <button class="icon" onclick={reload} title="Refresh"><Icon name="refresh" /></button>
    </div>
    <div class="filters">
      {#if filters.kind !== "favorites"}
        <div class="segmented" role="radiogroup" aria-label="Clip type">
          {#each kinds as [k, name] (k)}
            <button role="radio" aria-checked={filters.kind === k} class:on={filters.kind === k} onclick={() => (filters.kind = k)}>{name}</button>
          {/each}
        </div>
      {/if}
      {#if chips.length > 0}
        <div class="chips">
          {#each chips as [kind, n] (kind)}
            <button class="chip" class:on={filters.event === kind} onclick={() => (filters.event = filters.event === kind ? null : kind)}>
              <Icon event={kind} size={16} />
              {eventName(kind)}
              <span class="n mono">{n}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </header>

  <div
    class="scroller"
    bind:this={scroller}
    bind:clientWidth={width}
    bind:clientHeight={height}
    onscroll={() => (scrollTop = scroller.scrollTop)}
    onpointerdown={dragStart}
    onpointermove={dragMove}
    onpointerup={dragEnd}
    onpointercancel={dragEnd}
    role="listbox"
    aria-multiselectable="true"
    tabindex="0"
  >
    <div class="content" style="height: {layout.height}px">
      {#each items as item (item.key)}
        {#if item.kind === "arriving"}
          <div class="arriving" style="transform: translateY({item.y}px); left: {PAD}px; right: {PAD}px">
            {#if grouped}
              <div class="session arriving-head" style="height: {HEADER_H}px">
                <span class="live"></span>
                <span class="s-title">Arriving</span>
                <span class="s-time mono">now</span>
              </div>
            {/if}
            <div class="arriving-row" style="gap: {GAP}px">
              {#each arriving.slice(0, cols) as a (a.id)}
                <ArrivingCard arrival={a} w={cardW} />
              {/each}
            </div>
          </div>
        {:else if item.kind === "header"}
          {@const s = item.session}
          <div class="session" style="transform: translateY({item.y}px); left: {PAD}px; right: {PAD}px; height: {HEADER_H}px">
            <span class="s-title">{sessionTitle(s)}</span>
            {#if s.mode}<span class="s-mode label">{modeName(s.mode)}</span>{/if}
            {#if s.map && s.game}<span class="s-game">{s.game}</span>{/if}
            <span class="s-time mono">{sessionTime(s)}</span>
            <span class="s-count mono">{s.clips.length} {s.clips.length === 1 ? "clip" : "clips"}</span>
            <button class="ghost s-select" onclick={() => selectSession(s)}>
              {s.clips.every((c) => selected.has(c.path)) ? "Deselect" : "Select all"}
            </button>
          </div>
        {:else}
          {@const clip = item.clip}
          {@const i = item.index}
          <ClipCard
            {clip}
            x={item.x}
            y={item.y}
            w={cardW}
            selected={selected.has(clip.path)}
            fresh={incoming.fresh.includes(clip.path)}
            selecting={selected.size > 0}
            onselect={(e) => select(e, i)}
            ontoggle={() => toggle(i)}
            onopen={() => openClip(clip)}
            onfavorite={() => setFavorite([clip.path], !clip.favorite)}
          />
        {/if}
      {/each}
      {#if drag}
        <div
          class="marquee"
          style="left: {Math.min(drag.x0, drag.x1)}px; top: {Math.min(drag.y0, drag.y1)}px; width: {Math.abs(drag.x1 - drag.x0)}px; height: {Math.abs(drag.y1 - drag.y0)}px"
        ></div>
      {/if}
    </div>

    {#if !library.loading && visible.length === 0 && arriving.length === 0}
      <div class="empty">
        {#if library.error}
          <Icon name="warning" size={28} />
          <p>Couldn't read the clips folder.<br /><span class="muted">{library.error}</span></p>
        {:else if library.clips.length === 0}
          <Icon event="manual" size={44} />
          <h2>No clips yet</h2>
          <p class="muted">Press <kbd>{recorder.hotkey}</kbd> in a game to save the last {recorder.lengthSecs} seconds.<br />Headshots and multi-kills in CS2 are saved on their own.</p>
        {:else}
          <Icon name="search" size={28} />
          <p class="muted">No clips match these filters.</p>
          <button
            onclick={() => {
              filters.event = null;
              filters.search = "";
              filters.kind = filters.kind === "favorites" ? "favorites" : "all";
            }}>Clear filters</button
          >
        {/if}
      </div>
    {/if}
  </div>

  {#if selected.size > 0}
    <div class="selection">
      <span class="what"><b>{selected.size}</b> selected <span class="mono">· {size(selectedSize)}</span></span>
      <button onclick={() => setFavorite([...selected], !allFavorite)}><Icon name="star" size={16} />{allFavorite ? "Unfavorite" : "Favorite"}</button>
      <button onclick={() => api.reveal(selectedClips[0].path)}><Icon name="folder" size={16} />Show in folder</button>
      <button class="danger" onclick={() => deleteClips([...selected])}><Icon name="trash" size={16} />Delete</button>
      <button class="ghost icon" aria-label="Clear selection" onclick={() => selected.clear()}><Icon name="x" size={16} /></button>
    </div>
  {/if}
</section>

<style>
  .library {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  header {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 2px 28px 14px;
    border-bottom: 1px solid var(--line);
  }

  .top {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .title {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-right: auto;
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 22px;
    letter-spacing: -0.03em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sub {
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--line);
    color: var(--muted);
  }

  .search:focus-within {
    border-color: var(--line-strong);
  }

  .search input {
    width: 190px;
    padding: 7px 0;
    border: none;
    background: transparent;
    outline: none;
  }

  button.icon.on {
    color: var(--accent);
  }

  .filters {
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: 34px;
  }

  .segmented {
    display: flex;
    padding: 3px;
    border-radius: 10px;
    background: var(--surface);
    border: 1px solid var(--line);
  }

  .segmented button {
    padding: 5px 14px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    font-size: 13px;
  }

  .segmented button.on {
    background: var(--surface-3);
    color: var(--text);
  }

  .chips {
    display: flex;
    gap: 6px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .chip {
    gap: 7px;
    padding: 5px 11px 5px 8px;
    border-radius: 999px;
    font-size: 13px;
    white-space: nowrap;
    background: var(--surface);
  }

  .chip .n {
    font-size: 11px;
    color: var(--faint);
  }

  .chip.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .chip.on .n {
    color: var(--text);
  }

  .scroller {
    position: relative;
    flex: 1;
    overflow-y: auto;
    outline: none;
  }

  .content {
    position: relative;
  }

  .session {
    position: absolute;
    top: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding-top: 12px;
  }

  .arriving {
    position: absolute;
    top: 0;
  }

  .arriving-head {
    position: static;
  }

  .arriving-row {
    display: flex;
  }

  .live {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--accent);
    animation: live 1.2s ease-in-out infinite;
  }

  @keyframes live {
    50% {
      opacity: 0.25;
    }
  }

  .s-title {
    font-weight: 650;
    font-size: 16px;
  }

  .s-mode {
    padding: 3px 8px;
    border-radius: 6px;
    background: var(--surface);
    font-size: 10px;
  }

  .s-game,
  .s-time,
  .s-count {
    color: var(--muted);
    font-size: 12px;
  }

  .s-count {
    margin-left: auto;
  }

  .s-select {
    padding: 4px 10px;
    font-size: 12px;
    color: var(--muted);
    opacity: 0;
  }

  .session:hover .s-select {
    opacity: 1;
  }

  .marquee {
    position: absolute;
    border: 1px solid var(--accent);
    background: var(--accent-soft);
    border-radius: 4px;
    pointer-events: none;
  }

  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 40px;
    text-align: center;
    color: var(--text);
  }

  .empty h2 {
    margin: 4px 0 0;
    font-family: var(--font-display);
    font-size: 18px;
  }

  .empty p {
    margin: 0;
    line-height: 1.6;
  }

  .muted {
    color: var(--muted);
  }

  .selection {
    position: absolute;
    left: 50%;
    bottom: 22px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 7px 7px 18px;
    border: 1px solid var(--line-strong);
    border-radius: 14px;
    background: color-mix(in srgb, var(--surface-2) 94%, transparent);
    backdrop-filter: blur(12px);
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.5);
    white-space: nowrap;
    animation: up 0.2s var(--ease);
  }

  .what {
    margin-right: 10px;
    color: var(--muted);
  }

  .what b {
    color: var(--text);
  }

  @keyframes up {
    from {
      opacity: 0;
      transform: translate(-50%, 10px);
    }
  }
</style>
