<script lang="ts">
  import { icons, type IconName } from "../icons";

  let { name, size = 20, label }: { name: IconName; size?: number; label?: string } = $props();
</script>

<svg
  class="icon"
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="currentColor"
  aria-hidden={label ? undefined : "true"}
  role={label ? "img" : undefined}
  aria-label={label}
>
  <path d={icons[name]} />
</svg>

<style>
  .icon {
    display: block;
    flex: 0 0 auto;
  }
</style>
