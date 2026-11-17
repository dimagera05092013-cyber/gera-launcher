<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "../icons";

  let {
    variant = "filled",
    size = "md",
    disabled = false,
    type = "button",
    title,
    icon,
    trailingIcon,
    full = false,
    onclick,
    children,
  }: {
    variant?: "filled" | "tonal" | "elevated" | "outlined" | "text" | "danger";
    size?: "sm" | "md" | "lg";
    disabled?: boolean;
    type?: "button" | "submit";
    title?: string;
    icon?: IconName;
    trailingIcon?: IconName;
    full?: boolean;
    onclick?: (event: MouseEvent) => void;
    children?: Snippet;
  } = $props();

  const iconSize = $derived(size === "lg" ? 24 : size === "sm" ? 16 : 18);
</script>

<button
  class="btn {variant} {size}"
  class:full
  {disabled}
  {type}
  {title}
  {onclick}
  aria-label={title}
>
  {#if icon}<Icon name={icon} size={iconSize} />{/if}
  {#if children}<span class="label">{@render children()}</span>{/if}
  {#if trailingIcon}<Icon name={trailingIcon} size={iconSize} />{/if}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 9px;
    border: none;
    border-radius: var(--shape-full);
    font: inherit;
    font-weight: 650;
    letter-spacing: 0.01em;
    cursor: pointer;
    white-space: nowrap;
    -webkit-tap-highlight-color: transparent;
    transition:
      background-color var(--motion-fast) var(--ease-standard),
      color var(--motion-fast) var(--ease-standard),
      box-shadow var(--motion-fast) var(--ease-standard),
      transform var(--motion-fast) var(--ease-standard),
      filter var(--motion-fast) var(--ease-standard);
  }
  .btn:focus-visible {
    outline: 2px solid var(--primary);
    outline-offset: 2px;
  }
  .btn:active:not(:disabled) {
    transform: scale(0.97);
  }
  .btn:disabled {
    cursor: not-allowed;
    opacity: 0.38;
  }
  .btn.full {
    width: 100%;
  }
  .sm {
    height: 36px;
    padding: 0 14px;
    font-size: 12.5px;
  }
  .md {
    height: 44px;
    padding: 0 22px;
    font-size: 13.5px;
  }
  .lg {
    height: 60px;
    padding: 0 30px;
    font-size: 16px;
    font-weight: 700;
    gap: 12px;
  }
  .filled {
    background: var(--primary);
    color: var(--on-primary);
  }
  .filled:hover:not(:disabled) {
    filter: brightness(1.08);
    box-shadow: var(--elevation-1);
  }
  .tonal {
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  .tonal:hover:not(:disabled) {
    filter: brightness(1.04);
  }
  .elevated {
    background: var(--surface-container-low);
    color: var(--primary);
    box-shadow: var(--elevation-1);
  }
  .elevated:hover:not(:disabled) {
    background: var(--surface-container);
  }
  .outlined {
    background: transparent;
    color: var(--primary);
    box-shadow: inset 0 0 0 1px var(--outline);
  }
  .outlined:hover:not(:disabled) {
    background: color-mix(in srgb, var(--primary) 8%, transparent);
  }
  .text {
    background: transparent;
    color: var(--primary);
    padding-inline: 14px;
  }
  .text:hover:not(:disabled) {
    background: color-mix(in srgb, var(--primary) 9%, transparent);
  }
  .danger {
    background: var(--error-container);
    color: var(--on-error-container);
  }
  .danger:hover:not(:disabled) {
    filter: brightness(1.04);
  }
  .label {
    display: inline-flex;
    align-items: center;
  }
</style>
