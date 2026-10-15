<script lang="ts">
  // Click, then press the new combination. Escape cancels.
  let { value, onchange }: { value: string; onchange: (hotkey: string) => void } = $props();

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

<button class="hotkey" class:listening onclick={() => (listening = !listening)} onblur={() => (listening = false)}>
  {#if listening}
    <span class="wait">Press the new keys…</span><kbd>Esc</kbd>
  {:else}
    {#each value.split("+") as part, i (i)}{#if i > 0}<span class="plus">+</span>{/if}<kbd>{part}</kbd>{/each}
  {/if}
</button>

<style>
  .hotkey {
    justify-content: flex-start;
    gap: 5px;
    min-width: 200px;
    padding: 6px 10px;
  }

  .hotkey kbd {
    font-size: 12px;
    padding: 2px 8px;
  }

  .plus {
    color: var(--faint);
    font-size: 12px;
  }

  .listening {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  .wait {
    margin-right: auto;
    color: var(--muted);
  }
</style>
