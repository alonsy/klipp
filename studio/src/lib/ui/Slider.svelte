<script lang="ts">
  interface Props {
    value: number;
    min: number;
    max: number;
    step?: number;
    label: string;
    /** While dragging. */
    oninput?: (v: number) => void;
    /** When let go (for settings that shouldn't save on every step). */
    onchange?: (v: number) => void;
  }
  let { value, min, max, step = 1, label, oninput, onchange }: Props = $props();

  let fill = $derived(`${((value - min) / (max - min)) * 100}%`);
</script>

<input
  class="slider"
  type="range"
  {min}
  {max}
  {step}
  {value}
  aria-label={label}
  style="--fill: {fill}"
  oninput={(e) => oninput?.(Number(e.currentTarget.value))}
  onchange={(e) => onchange?.(Number(e.currentTarget.value))}
  ondblclick={() => {
    // Double-click resets a 0–200% level to 100%.
    if (min === 0 && max === 2) {
      oninput?.(1);
      onchange?.(1);
    }
  }}
/>
