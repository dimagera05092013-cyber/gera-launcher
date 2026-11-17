<script lang="ts">
  import { ui, refreshHorizon } from "../store.svelte";
  import { pageMeta } from "../router.svelte";
  import Icon from "./Icon.svelte";

  const meta = $derived(pageMeta[ui.page]);
  const running = $derived(ui.boot?.horizon.running ?? null);
  const hasHorizon = $derived((ui.boot?.horizon.versions.length ?? 0) > 0);
  const ready = $derived((ui.boot?.horizon.versions ?? []).some((v) => v.ready));

  const status = $derived(
    running
      ? { text: "В игре", tone: "live" as const }
      : !hasHorizon
        ? { text: "Нет Horizon", tone: "warn" as const }
        : !ready
          ? { text: "Нужны ресурсы", tone: "warn" as const }
          : { text: "Готов", tone: "ok" as const },
  );
</script>

<header class="topbar" class:solo={ui.page === "home"}>
  <div class="titles">
    <h1>{meta.title}</h1>
    <p>{meta.subtitle}</p>
  </div>
  <div class="actions">
    <span class="status {status.tone}">
      <span class="dot"></span>
      {status.text}
    </span>
    <button
      type="button"
      class="refresh"
      title="Обновить состояние"
      aria-label="Обновить состояние"
      disabled={ui.busy}
      onclick={refreshHorizon}
    >
      <Icon name="refresh" size={20} />
    </button>
  </div>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 26px 36px 14px;
    position: sticky;
    top: 0;
    z-index: 6;
    background: var(--surface);
  }

  .titles {
    min-width: 0;
  }
  .topbar.solo {
    justify-content: flex-end;
  }
  .topbar.solo .titles {
    display: none;
  }
  h1 {
    margin: 0;
    font-size: var(--type-headline);
    line-height: 1.1;
    font-weight: 800;
    letter-spacing: var(--tracking-title);
  }
  p {
    margin-top: 3px;
    font-size: 12.5px;
    color: var(--on-surface-variant);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 0 auto;
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 14px;
    border-radius: var(--shape-full);
    background: var(--surface-container-high);
    color: var(--on-surface-variant);
    font-size: 12.5px;
    font-weight: 650;
    white-space: nowrap;
  }
  .status .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--outline);
  }
  .status.ok .dot {
    background: var(--primary);
  }
  .status.live {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .status.live .dot {
    background: currentColor;
  }
  .status.warn .dot {
    background: var(--error);
  }

  .refresh {
    width: 44px;
    height: 44px;
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
      transform var(--motion-slow) var(--ease-emph);
  }
  .refresh:hover:not(:disabled) {
    background: var(--surface-container-highest);
    color: var(--on-surface);
  }
  .refresh:active:not(:disabled) {
    transform: rotate(-120deg);
  }
  .refresh:disabled {
    opacity: 0.45;
    cursor: default;
  }

  :global([data-ui="heavy"]) .topbar {
    background: color-mix(in srgb, var(--surface) 84%, transparent);
    backdrop-filter: blur(18px) saturate(1.2);
  }

  @media (max-width: 760px) {
    .topbar {
      padding: 18px 18px 10px;
    }
    p {
      display: none;
    }
    .status {
      display: none;
    }
  }
</style>
