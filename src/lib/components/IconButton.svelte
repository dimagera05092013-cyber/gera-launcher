<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { IconName } from "../icons";

  let {
    icon,
    label,
    variant = "standard",
    size = "md",
    disabled = false,
    active = false,
    onclick,
  }: {
    icon: IconName;
    label: string;
    variant?: "standard" | "tonal" | "filled" | "outlined" | "text";
    size?: "sm" | "md" | "lg";
    disabled?: boolean;
    active?: boolean;
    onclick?: (event: MouseEvent) => void;
  } = $props();

  const px = $derived(size === "lg" ? 24 : size === "sm" ? 18 : 20);
</script>

<button
  type="button"
  class="icon-button {variant} {size}"
  class:active
  {disabled}
  title={label}
  aria-label={label}
  {onclick}
>
  <Icon name={icon} size={px} />
</button>

<style>
  .icon-button {
    width: 44px;
    height: 44px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border: none;
    border-radius: 50%;
    background: var(--surface-container-high);
    color: var(--on-surface-variant);
    cursor: pointer;
    transition:
      background-color var(--motion-fast) var(--ease-standard),
      color var(--motion-fast) var(--ease-standard),
      transform var(--motion-fast) var(--ease-standard);
  }
  .icon-button:hover:not(:disabled):not(.tonal):not(.filled):not(.active) {
    background: var(--surface-container-highest);
    color: var(--on-surface);
  }
  .icon-button.tonal:hover:not(:disabled) {
    background: color-mix(in srgb, var(--on-secondary-container) 9%, var(--secondary-container));
  }
  .icon-button.filled:hover:not(:disabled) {
    filter: brightness(1.08);
  }
  .icon-button:active:not(:disabled) {
    transform: scale(0.92);
  }
  .icon-button.tonal {
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  .icon-button.filled {
    background: var(--primary);
    color: var(--on-primary);
  }
  .icon-button.outlined {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--outline-variant);
  }
  .icon-button.text {
    background: transparent;
  }
  .icon-button.text:hover:not(:disabled) {
    background: color-mix(in srgb, var(--on-surface) 8%, transparent);
  }
  .icon-button.active {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .icon-button.active:hover:not(:disabled) {
    background: color-mix(in srgb, var(--on-primary-container) 9%, var(--primary-container));
  }
  .icon-button.sm {
    width: 36px;
    height: 36px;
  }
  .icon-button.lg {
    width: 56px;
    height: 56px;
  }
  .icon-button:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
