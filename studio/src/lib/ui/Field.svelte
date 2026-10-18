<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    hint?: string;
    /** A small label after the title, like "Experimental". */
    badge?: string;
    children: Snippet;
  }
  let { title, hint, badge, children }: Props = $props();
</script>

<div class="field">
  <div class="text">
    <span class="title">{title}{#if badge}<span class="badge">{badge}</span>{/if}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .field {
    display: flex;
    align-items: center;
    gap: 24px;
    padding: 14px 18px;
  }

  .field + :global(.field) {
    border-top: 1px solid var(--line);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }

  .title {
    font-weight: 550;
  }

  .badge {
    margin-left: 8px;
    padding: 2px 6px;
    border-radius: 5px;
    background: var(--accent-soft);
    color: var(--accent);
    font: 600 10px/1.4 var(--font-mono);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    vertical-align: 1px;
  }

  .hint {
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.45;
  }

  .control {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }
</style>
