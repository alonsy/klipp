<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "../Icon.svelte";

  interface Props {
    title: string;
    /** A line under the title. Only when a person would otherwise get it wrong. */
    hint?: string;
    /** Help that can wait: an (i) after the title, with this as its tooltip. */
    tip?: string;
    /** A small label after the title, like "Experimental". */
    badge?: string;
    children: Snippet;
  }
  let { title, hint, tip, badge, children }: Props = $props();
</script>

<div class="field">
  <div class="text">
    <span class="title">
      {title}{#if badge}<span class="badge">{badge}</span>{/if}{#if tip}<button type="button" class="ghost tip" title={tip} aria-label={tip}><Icon name="info" size={14} /></button>{/if}
    </span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .field {
    display: flex;
    align-items: center;
    gap: var(--sp-5);
    min-height: 48px;
    padding: var(--sp-2) var(--sp-4);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .title {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    font-weight: 550;
  }

  /* A button so the keyboard reaches its tooltip. It does nothing when pressed. */
  button.tip {
    width: 16px;
    height: 16px;
    padding: 0;
    border-radius: 50%;
    color: var(--faint);
    cursor: help;
    transition: color var(--dur-fast);
  }

  button.tip:hover,
  button.tip:active,
  button.tip:focus-visible {
    background: transparent;
    color: var(--text);
    transform: none;
  }

  .badge {
    padding: 1px 6px;
    border-radius: var(--r-xs);
    background: var(--surface-3);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 500;
  }

  .hint {
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.45;
  }

  .control {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    flex-shrink: 0;
  }
</style>
