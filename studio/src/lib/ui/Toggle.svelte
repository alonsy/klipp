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
    width: 40px;
    height: 22px;
    padding: 0;
    border-radius: 11px;
    border: 1px solid var(--line-strong);
    background: var(--surface-3);
    transition: background 0.18s;
  }

  .toggle:hover:not(:disabled) {
    background: #484846;
  }

  .toggle:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .toggle.on {
    background: var(--accent);
    border-color: var(--accent);
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.4);
    transition: transform 0.18s var(--ease);
  }

  .on .knob {
    transform: translateX(18px);
    background: var(--on-accent);
  }
</style>
