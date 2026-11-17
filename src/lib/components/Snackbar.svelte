<script lang="ts">
  import { ui } from "../store.svelte";
  import Icon from "./Icon.svelte";

  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const toast = ui.toast;
    if (!toast) return;
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (ui.toast?.id === toast.id) ui.toast = null;
    }, toast.kind === "error" ? 7000 : 3800);
    return () => clearTimeout(timer);
  });
</script>

{#if ui.toast}
  <div class="snackbar {ui.toast.kind}" role="status" aria-live="polite">
    <span class="lead">
      <Icon
        name={ui.toast.kind === "error" ? "warning" : ui.toast.kind === "success" ? "check" : "info"}
        size={18}
      />
    </span>
    <span class="text">{ui.toast.text}</span>
    <button type="button" aria-label="Скрыть" onclick={() => (ui.toast = null)}>
      <Icon name="close" size={16} />
    </button>
  </div>
{/if}

<style>
  .snackbar {
    position: fixed;
    left: 50%;
    bottom: 24px;
    transform: translateX(-50%);
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 280px;
    max-width: min(640px, calc(100vw - 32px));
    padding: 10px 10px 10px 12px;
    border-radius: var(--shape-md);
    background: var(--inverse-surface);
    color: var(--inverse-on-surface);
    box-shadow: var(--elevation-3);
    font-size: 13.5px;
    animation: rise var(--motion) var(--ease-emph);
  }
  .lead {
    width: 32px;
    height: 32px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--inverse-on-surface) 10%, transparent);
  }
  .snackbar.error .lead {
    color: var(--error);
  }
  .snackbar.success .lead {
    color: var(--primary);
  }
  .text {
    flex: 1;
    line-height: 1.4;
  }
  button {
    border: none;
    background: transparent;
    color: inherit;
    opacity: 0.7;
    cursor: pointer;
    padding: 8px;
    display: grid;
    place-items: center;
    border-radius: 50%;
  }
  button:hover {
    opacity: 1;
    background: color-mix(in srgb, var(--inverse-on-surface) 10%, transparent);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, 12px);
    }
  }
  @media (max-width: 760px) {
    .snackbar {
      bottom: 86px;
    }
  }
</style>
