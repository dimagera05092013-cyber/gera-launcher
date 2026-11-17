<script lang="ts">
  import { tick } from "svelte";
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let {
    open = false,
    title,
    wide = false,
    onclose,
    children,
    actions,
  }: {
    open?: boolean;
    title: string;
    wide?: boolean;
    onclose?: () => void;
    children?: Snippet;
    actions?: Snippet;
  } = $props();

  function close() {
    onclose?.();
  }

  let dialogEl = $state<HTMLDivElement>();

  // Move focus into the dialog, otherwise Tab keeps walking through the page
  // behind the scrim.
  $effect(() => {
    if (open) {
      void tick().then(() => dialogEl?.focus());
    }
  });

  function onkeydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }
</script>

<svelte:window onkeydown={onkeydown} />

{#if open}
  <div class="scrim">
    <button class="backdrop" type="button" aria-label="Закрыть" onclick={close}></button>
    <div class="dialog" class:wide role="dialog" aria-modal="true" aria-label={title} tabindex="-1" bind:this={dialogEl}>
      <header>
        <h2>{title}</h2>
        <button class="close" type="button" aria-label="Закрыть" onclick={close}>
          <Icon name="close" size={20} />
        </button>
      </header>
      <div class="body">
        {@render children?.()}
      </div>
      {#if actions}
        <footer>
          {@render actions()}
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    border: none;
    background: transparent;
    cursor: default;
  }
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--scrim) 46%, transparent);
    animation: fade var(--motion-fast) var(--ease-standard);
  }
  :global([data-ui="heavy"]) .scrim {
    backdrop-filter: blur(4px);
  }
  .dialog {
    position: relative;
    z-index: 1;
    width: min(480px, 100%);
    max-height: min(84vh, 780px);
    overflow: auto;
    background: var(--surface-container-high);
    color: var(--on-surface);
    border-radius: var(--shape-2xl);
    box-shadow: var(--elevation-3);
    padding: 26px;
    animation: pop var(--motion) var(--ease-emph);
  }
  .wide {
    width: min(640px, 100%);
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
  }
  h2 {
    margin: 0;
    font-size: 21px;
    font-weight: 750;
    letter-spacing: -0.02em;
    line-height: 1.2;
  }
  .body {
    color: var(--on-surface-variant);
    font-size: 13.5px;
    line-height: 1.6;
  }
  .body :global(p) {
    margin: 0 0 10px;
  }
  .body :global(p:last-child) {
    margin-bottom: 0;
  }
  .body :global(strong) {
    color: var(--on-surface);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 24px;
    flex-wrap: wrap;
  }
  .close {
    border: none;
    background: transparent;
    color: var(--on-surface-variant);
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    cursor: pointer;
    flex: 0 0 auto;
    transition: background-color var(--motion-fast) var(--ease-standard);
  }
  .close:hover {
    background: var(--surface-container-highest);
    color: var(--on-surface);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(10px) scale(0.97);
    }
  }
</style>
