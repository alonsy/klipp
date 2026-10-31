<script lang="ts">
  // A number that eases to its new value instead of jumping. Lands on the value
  // anyway when animation frames don't run (the window hidden or covered).
  import { untrack } from "svelte";
  import { DUR, easeOut, reduced } from "../motion";

  let { value, format = (x: number) => String(Math.round(x)) }: { value: number; format?: (x: number) => string } = $props();

  let shown = $state(untrack(() => value));

  $effect(() => {
    const to = value;
    const from = untrack(() => shown);
    if (from === to) return;
    if (reduced()) {
      shown = to;
      return;
    }
    const start = performance.now();
    let frame = 0;
    const step = (now: number) => {
      const k = easeOut(Math.min(1, (now - start) / DUR.spring));
      shown = from + (to - from) * k;
      if (k < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    const land = setTimeout(() => (shown = to), DUR.spring + 180);
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(land);
    };
  });
</script>

{format(shown)}
