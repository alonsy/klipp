<script lang="ts">
  // Click, then press the new combination. Escape cancels. `big` draws real
  // keycaps (setup); `phase` is HotkeyTest's, so the key reacts to its test.
  let {
    value,
    onchange,
    big = false,
    phase = "idle",
  }: { value: string; onchange: (hotkey: string) => void; big?: boolean; phase?: "idle" | "ready" | "press" | "works" | "failed" } = $props();

  let listening = $state(false);

  function key(e: KeyboardEvent): string | null {
    const c = e.code;
    if (/^Key[A-Z]$/.test(c)) return c.slice(3);
    if (/^Digit\d$/.test(c)) return c.slice(5);
    if (/^F\d{1,2}$/.test(c)) return c;
    if (/^Numpad\d$/.test(c)) return c;
    const named: Record<string, string> = {
      PrintScreen: "PrintScreen",
      Pause: "Pause",
      ScrollLock: "ScrollLock",
      Insert: "Insert",
      Delete: "Delete",
      Home: "Home",
      End: "End",
      PageUp: "PageUp",
      PageDown: "PageDown",
      Space: "Space",
    };
    return named[c] ?? null;
  }

  function onkeydown(e: KeyboardEvent) {
    if (!listening) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      listening = false;
      return;
    }
    const k = key(e);
    if (!k) return; // Only a modifier so far; wait for the key.
    const parts = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win", k].filter(Boolean);
    listening = false;
    onchange(parts.join("+"));
  }
</script>

<svelte:window onkeydowncapture={onkeydown} />

<button
  class="hotkey"
  class:big
  class:listening
  class:press={phase === "press"}
  class:works={phase === "works"}
  class:failed={phase === "failed"}
  data-tip={listening ? undefined : "Change the key"}
  onclick={() => (listening = !listening)}
  onblur={() => (listening = false)}
>
  {#if listening}
    <span class="wait">Press the new keys…</span><kbd>Esc</kbd>
  {:else}
    {#key value}
      <span class="keys">
        {#each value.split("+") as part, i (i)}{#if i > 0}<span class="plus">+</span>{/if}<kbd>{part}</kbd>{/each}
      </span>
    {/key}
  {/if}
</button>

<style>
  .hotkey {
    justify-content: flex-start;
    gap: var(--sp-1);
    min-width: 200px;
    padding: 6px 10px;
  }

  .keys {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    animation: pop var(--dur-spring) var(--spring) backwards;
  }

  .hotkey kbd {
    font-size: var(--fs-sm);
    padding: 2px 8px;
  }

  .plus {
    color: var(--faint);
    font-size: var(--fs-sm);
  }

  .listening {
    background: var(--accent-soft);
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .wait {
    margin-right: auto;
    color: var(--text);
    animation: fade var(--dur) var(--ease-out) backwards;
  }

  /* Setup: the key is the thing to press, so it looks like one. */
  .big {
    min-width: 0;
    min-height: 64px;
    padding: 0;
    background: transparent;
  }

  .big:hover {
    background: transparent;
  }

  .big:active {
    transform: none;
  }

  .big .keys {
    gap: var(--sp-2);
  }

  .big kbd {
    min-width: 64px;
    padding: 14px 16px 15px;
    border-radius: var(--r-sm);
    background: var(--surface-2);
    box-shadow:
      0 3px 0 var(--sunken),
      0 4px 12px rgba(0, 0, 0, 0.3);
    font-size: var(--fs-lg);
    font-weight: 600;
    line-height: 1.2;
    transition:
      transform var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out),
      background var(--dur) var(--ease-out),
      color var(--dur) var(--ease-out);
  }

  .big:hover kbd {
    background: var(--surface-3);
  }

  .big:active kbd {
    transform: translateY(2px);
    box-shadow:
      0 1px 0 var(--sunken),
      0 2px 6px rgba(0, 0, 0, 0.3);
  }

  .big .plus {
    font-size: var(--fs-md);
  }

  .big.listening {
    min-width: 240px;
    padding: 0 var(--sp-3) 0 var(--sp-4);
    border-radius: var(--r-sm);
    gap: var(--sp-4);
  }

  .big.listening kbd {
    min-width: 0;
    padding: 2px 8px;
    box-shadow: none;
    font-size: var(--fs-sm);
    font-weight: 400;
  }

  /* The test: it glows while it waits, and sinks in like a press when it lands. */
  .big.press kbd {
    background: var(--accent);
    color: var(--on-accent);
    animation: glow 1s var(--ease-out) infinite alternate;
  }

  .big.works kbd {
    background: var(--good);
    color: var(--on-accent);
    animation: landed var(--dur-spring) var(--spring);
  }

  .big.failed kbd {
    background: var(--danger-soft);
    color: var(--danger);
  }

  @keyframes glow {
    to {
      opacity: 0.72;
    }
  }

  @keyframes landed {
    30% {
      transform: translateY(3px);
    }
  }
</style>
