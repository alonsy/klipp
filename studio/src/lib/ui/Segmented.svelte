<script lang="ts" generics="T extends string | number">
  interface Props {
    value: T;
    options: readonly (readonly [T, string])[];
    label: string;
    onchange: (v: T) => void;
  }
  let { value, options, label, onchange }: Props = $props();
</script>

<div class="segmented" role="radiogroup" aria-label={label}>
  {#each options as [v, name] (v)}
    <button role="radio" aria-checked={value === v} class:on={value === v} onclick={() => value !== v && onchange(v)}>{name}</button>
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    padding: 3px;
    border-radius: 10px;
    background: var(--bg);
    border: 1px solid var(--line);
  }

  button {
    padding: 5px 13px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    font-size: 13px;
    white-space: nowrap;
  }

  button:hover {
    background: transparent;
    color: var(--text);
  }

  button.on {
    background: var(--surface-3);
    color: var(--text);
  }
</style>
