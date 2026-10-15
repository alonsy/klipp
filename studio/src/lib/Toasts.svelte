<script lang="ts">
  import { flip } from "svelte/animate";
  import { fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { dismiss, toasts } from "./state.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each toasts as t (t.id)}
    <div class="toast" class:error={t.tone === "error"} animate:flip={{ duration: 200 }} transition:fly={{ y: 16, duration: 200 }}>
      <Icon name={t.tone === "error" ? "warning" : "check"} size={16} />
      <span class="text">{t.text}</span>
      {#if t.action}<button class="action" onclick={t.action.run}>{t.action.label}</button>{/if}
      <button class="ghost close" aria-label="Dismiss" onclick={() => dismiss(t.id)}><Icon name="x" size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: 60;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 440px;
    padding: 9px 8px 9px 14px;
    border-radius: 12px;
    background: var(--surface-2);
    border: 1px solid var(--line-strong);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    pointer-events: auto;
  }

  .toast > :global(.icon) {
    color: var(--good);
  }

  .toast.error > :global(.icon) {
    color: var(--danger);
  }

  .text {
    line-height: 1.4;
  }

  .action {
    padding: 4px 10px;
    font-weight: 600;
    color: var(--accent);
    background: transparent;
    border-color: var(--line-strong);
  }

  .close {
    width: 26px;
    height: 26px;
    padding: 0;
    color: var(--muted);
  }
</style>
