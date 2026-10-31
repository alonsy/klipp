<script lang="ts" generics="T extends string | number">
  interface Props {
    value: T;
    options: readonly (readonly [T, string])[];
    label: string;
    onchange: (v: T) => void;
  }
  let { value, options, label, onchange }: Props = $props();

  let index = $derived(options.findIndex(([v]) => v === value));

  /** Left and right move the choice, as in a native radio group. */
  function onkeydown(e: KeyboardEvent) {
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (step === 0 || options.length < 2) return;
    e.preventDefault();
    const next = (Math.max(index, 0) + step + options.length) % options.length;
    onchange(options[next][0]);
    const buttons = (e.currentTarget as HTMLElement).querySelectorAll("button");
    buttons[next]?.focus();
  }
</script>

<!-- Every segment is as wide as the widest, so the thumb only ever translates. -->
<div class="segmented" role="radiogroup" aria-label={label} tabindex="-1" style="--n: {options.length}; --i: {index}" {onkeydown}>
  {#if index >= 0}<span class="thumb" aria-hidden="true"></span>{/if}
  {#each options as [v, name] (v)}
    <button role="radio" aria-checked={value === v} class:on={value === v} onclick={() => value !== v && onchange(v)}>{name}</button>
  {/each}
</div>

<style>
  .segmented {
    --pad: 3px;
    position: relative;
    display: inline-grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    padding: var(--pad);
    border-radius: var(--r-sm);
    background: var(--sunken);
  }

  .thumb {
    position: absolute;
    top: var(--pad);
    bottom: var(--pad);
    left: var(--pad);
    width: calc((100% - 2 * var(--pad)) / var(--n));
    border-radius: 7px;
    background: var(--surface-3);
    box-shadow: var(--shadow-1);
    transform: translateX(calc(var(--i) * 100%));
    transition: transform var(--dur-spring) var(--spring);
  }

  button {
    position: relative;
    padding: 5px 13px;
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-control);
    white-space: nowrap;
  }

  button:hover {
    background: transparent;
    color: var(--text);
  }

  button:active {
    transform: none;
  }

  button.on {
    color: var(--text);
  }
</style>
