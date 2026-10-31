<script lang="ts">
  // A line that opens what's under it: shortcuts, a release's notes, clips by game.
  import type { Snippet } from "svelte";
  import Icon from "../Icon.svelte";
  import Reveal from "./Reveal.svelte";

  interface Props {
    label: string;
    open?: boolean;
    /** After the label, like a tag. */
    meta?: Snippet;
    children: Snippet;
  }
  let { label, open = $bindable(false), meta, children }: Props = $props();
</script>

<div class="disclosure">
  <button class="ghost line" class:open aria-expanded={open} onclick={() => (open = !open)}>
    <span class="chev"><Icon name="chevronRight" size={14} /></span>
    <span class="name">{label}</span>
    {#if meta}{@render meta()}{/if}
  </button>
  <Reveal {open}>{@render children()}</Reveal>
</div>

<style>
  .line {
    justify-content: flex-start;
    gap: var(--sp-2);
    width: calc(100% + 2 * var(--sp-2));
    min-height: 40px;
    margin-inline: calc(-1 * var(--sp-2));
    padding: var(--sp-1) var(--sp-2);
    font-weight: 550;
    text-align: left;
  }

  .line:active {
    transform: none;
  }

  .chev {
    display: inline-flex;
    color: var(--faint);
    transition: transform var(--dur) var(--ease-out);
  }

  .open .chev {
    transform: rotate(90deg);
  }
</style>
