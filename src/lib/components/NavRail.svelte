<script lang="ts">
  import { ui } from "../store.svelte";
  import { go, type PageId } from "../router.svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "../icons";

  const items: Array<{ id: PageId; icon: IconName; label: string }> = [
    { id: "home", icon: "home", label: "Главная" },
    { id: "minecraft", icon: "java", label: "Java" },
    { id: "builds", icon: "builds", label: "Сборки" },
    { id: "servers", icon: "servers", label: "Серверы" },
    { id: "mods", icon: "mods", label: "Моды" },
    { id: "settings", icon: "settings", label: "Настройки" },
  ];

  const running = $derived(ui.boot?.horizon.running ?? null);
</script>

<nav class="rail" aria-label="Разделы">
  <div class="brand" aria-hidden="true">
    <span class="mark">G</span>
  </div>

  <div class="items">
    {#each items as item (item.id)}
      <button
        type="button"
        class="item"
        class:active={ui.page === item.id}
        aria-current={ui.page === item.id ? "page" : undefined}
        onclick={() => go(item.id)}
        title={item.label}
      >
        <span class="indicator">
          <Icon name={item.icon} size={23} />
          {#if item.id === "home" && running}
            <span class="live-dot" aria-hidden="true"></span>
          {/if}
        </span>
        <span class="label">{item.label}</span>
      </button>
    {/each}
  </div>

  <div class="foot">
    <span class="version">v{ui.boot?.version ?? "0.9.0"}</span>
    <span class="phase">beta</span>
  </div>
</nav>

<style>
  .rail {
    width: 104px;
    flex: 0 0 104px;
    height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 28px;
    padding: 20px 8px 18px;
    z-index: 3;
    background: var(--surface);
  }

  .brand {
    display: grid;
    place-items: center;
  }
  .mark {
    width: 48px;
    height: 48px;
    display: grid;
    place-items: center;
    border-radius: 17px;
    font-size: 22px;
    font-weight: 900;
    letter-spacing: -0.03em;
    color: var(--on-primary-container);
    background: var(--brand-gradient);
  }
  :global([data-ui="heavy"]) .mark {
    box-shadow: var(--elevation-1);
  }

  .items {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    width: 100%;
  }

  .item {
    width: 92px;
    padding: 6px 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    border: none;
    border-radius: var(--shape-lg);
    background: transparent;
    color: var(--on-surface-variant);
    font: inherit;
    cursor: pointer;
    transition: color var(--motion-fast) var(--ease-standard);
  }
  .item:hover {
    color: var(--on-surface);
  }
  .item:hover .indicator {
    background: color-mix(in srgb, var(--on-surface) 7%, transparent);
  }
  .item:focus-visible {
    outline: 2px solid var(--primary);
    outline-offset: -2px;
  }

  .indicator {
    position: relative;
    width: 62px;
    height: 34px;
    display: grid;
    place-items: center;
    border-radius: var(--shape-full);
    transition:
      background-color var(--motion) var(--ease-emph),
      color var(--motion) var(--ease-emph);
  }
  .item.active {
    color: var(--on-surface);
  }
  .item.active .indicator {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }

  .live-dot {
    position: absolute;
    top: 6px;
    right: 13px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--primary);
    box-shadow: 0 0 0 3px var(--primary-container);
  }

  .label {
    font-size: 11px;
    font-weight: 650;
    letter-spacing: 0.01em;
    line-height: 1.2;
  }

  .foot {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    color: var(--on-surface-variant);
    font-size: 10.5px;
    opacity: 0.65;
  }
  .version {
    font-weight: 600;
  }
  .phase {
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  @media (max-width: 760px) {
    .rail {
      position: fixed;
      inset: auto 0 0 0;
      width: 100%;
      height: 74px;
      flex-direction: row;
      justify-content: center;
      padding: 6px 12px calc(6px + env(safe-area-inset-bottom));
      background: var(--surface-container);
      border-radius: var(--shape-xl) var(--shape-xl) 0 0;
      z-index: 20;
    }
    .brand,
    .foot {
      display: none;
    }
    .items {
      flex-direction: row;
      justify-content: space-around;
      max-width: 560px;
    }
    .item {
      width: 84px;
      padding: 4px 0;
    }
  }
</style>
