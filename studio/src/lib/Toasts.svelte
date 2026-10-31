<script lang="ts">
  import { flip } from "svelte/animate";
  import Icon from "./Icon.svelte";
  import { DUR, enter, leave, reduced, spring } from "./motion";
  import { dismiss, toasts } from "./state.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each toasts as t (t.id)}
    <div
      class="toast"
      class:error={t.tone === "error"}
      animate:flip={{ duration: reduced() ? 0 : DUR.base }}
      in:enter={{ y: 16, duration: DUR.spring, easing: spring }}
      out:leave={{ x: 16 }}
    >
      <Icon name={t.tone === "error" ? "warning" : "check"} size={16} />
      <span class="text">{t.text}</span>
      {#if t.action}<button class="ghost action" onclick={t.action.run}>{t.action.label}</button>{/if}
      <button class="ghost close" aria-label="Dismiss" onclick={() => dismiss(t.id)}><Icon name="x" size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: var(--sp-5);
    bottom: var(--sp-5);
    z-index: 60;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: var(--sp-2);
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    max-width: 440px;
    min-height: 40px;
    padding: var(--sp-1) var(--sp-1) var(--sp-1) var(--sp-3);
    border-radius: var(--r-sm);
    background: var(--surface-3);
    box-shadow: var(--shadow-2);
    pointer-events: auto;
    user-select: text;
  }

  .toast > :global(.icon) {
    color: var(--good);
  }

  .toast.error > :global(.icon) {
    color: var(--danger);
  }

  .text {
    margin-right: var(--sp-1);
    line-height: 1.4;
  }

  .action {
    padding: 4px 10px;
    font-weight: 600;
    color: var(--accent);
    white-space: nowrap;
  }

  .close {
    flex-shrink: 0;
    width: 28px;
    height: 28px;
    padding: 0;
    color: var(--muted);
    opacity: 0.6;
  }

  .toast:hover .close,
  .close:focus-visible {
    opacity: 1;
  }
</style>
