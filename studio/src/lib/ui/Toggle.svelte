<script lang="ts">
  let {
    checked,
    label,
    disabled = false,
    onchange,
  }: { checked: boolean; label: string; disabled?: boolean; onchange: (v: boolean) => void } = $props();
</script>

<button class="toggle" class:on={checked} role="switch" aria-checked={checked} aria-label={label} {disabled} onclick={() => onchange(!checked)}>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    position: relative;
    flex-shrink: 0;
    width: 38px;
    height: 22px;
    padding: 0;
    border-radius: var(--r-full);
    background: var(--surface-3);
    transition: background var(--dur) var(--ease-out);
  }

  .toggle:hover:not(:disabled) {
    background: var(--surface-4);
  }

  .toggle:active {
    transform: none;
  }

  .toggle:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Bone, not accent: a page of switches shouldn't shout. */
  .toggle.on,
  .toggle.on:hover:not(:disabled) {
    background: var(--text);
  }

  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--text);
    box-shadow: var(--shadow-1);
    transition:
      transform var(--dur-spring) var(--spring),
      background var(--dur) var(--ease-out);
  }

  .on .knob {
    transform: translateX(16px);
    background: var(--on-accent);
    box-shadow: none;
  }

  /* A press squashes the knob towards where it's going. */
  .toggle:active:not(:disabled) .knob {
    transform: scaleX(1.25);
    transition-duration: var(--dur-fast);
  }

  .toggle.on:active:not(:disabled) .knob {
    transform: translateX(16px) scaleX(1.25);
  }
</style>
