<script lang="ts">
  import { eventSvg, uiSvg } from "./icons";

  interface Props {
    /** An interface icon name ("play", "trash"…). */
    name?: string;
    /** An event name from clip info ("headshot", "2k"…), drawn as a brand event icon. */
    event?: string;
    size?: number;
  }
  let { name, event, size = 18 }: Props = $props();
</script>

<span class="icon" style="width: {size}px; height: {size}px">
  {@html event ? eventSvg(event, size) : uiSvg(name ?? "", size)}
</span>

<style>
  .icon {
    display: inline-flex;
    flex-shrink: 0;
    line-height: 0;
  }
</style>
