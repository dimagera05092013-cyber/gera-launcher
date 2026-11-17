<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import NavRail from "./lib/components/NavRail.svelte";
  import TopBar from "./lib/components/TopBar.svelte";
  import Snackbar from "./lib/components/Snackbar.svelte";
  import UpdateDialog from "./lib/components/UpdateDialog.svelte";
  import Button from "./lib/components/Button.svelte";
  import { ui, init, notify, reload } from "./lib/store.svelte";
  import { api, errorText } from "./lib/api";
  import { checkUpdates } from "./lib/update.svelte";
  import { start } from "./lib/router.svelte";
  import type { GameExited } from "./lib/types";

  let unlisten: (() => void) | undefined;
  let dropHover = $state(false);
  let dropBusy = $state(false);

  /// A folder or zip dragged onto the window is imported straight away, so a
  /// player moving their resources between PCs does not have to find the path.
  async function handleDrop(path: string) {
    if (dropBusy) return;
    dropBusy = true;
    try {
      const scan = await api.importScan(null, path);
      if (scan.imported) {
        notify(
          `Ресурсы Minecraft ${scan.imported.minecraft} импортированы (${Math.round(
            scan.imported.bytes / 1024 / 1024,
          )} МБ).`,
          "success",
        );
        await reload();
      } else {
        notify("Ресурсы уже готовы — импортировать нечего.", "info");
      }
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      dropBusy = false;
    }
  }

  onMount(() => {
    void (async () => {
      await init();
      if (ui.ready) await start("home");
      void checkUpdates();
    })();

    void listen<GameExited>("game-exited", (event) => {
      const code = event.payload.code;
      notify(
        code === 0 || code === null
          ? "Horizon закрыт."
          : `Horizon завершился с кодом ${code}. Лог сохранён.`,
        code === 0 || code === null ? "info" : "error",
      );
      void reload();
    }).then((dispose) => {
      unlisten = dispose;
    });

    let disposeDrop: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          dropHover = true;
        } else if (payload.type === "leave") {
          dropHover = false;
        } else if (payload.type === "drop") {
          dropHover = false;
          const path = payload.paths[0];
          if (path) void handleDrop(path);
        }
      })
      .then((dispose) => {
        disposeDrop = dispose;
      });

    return () => {
      unlisten?.();
      disposeDrop?.();
    };
  });

  const Page = $derived(ui.pageComponent as any);
</script>

<div class="app">
  {#if dropHover}
    <div class="drop-overlay" aria-hidden="true">
      <div class="drop-box">
        <span class="drop-icon">↓</span>
        <strong>Отпустите папку с ресурсами</strong>
        <span>Папка <span class="mono">26.3</span> или zip с ней — GERA импортирует её в клиент.</span>
      </div>
    </div>
  {/if}

  {#if ui.loading}
    <div class="boot">
      <div class="boot-mark">G</div>
      <p>GERA Launcher</p>
      <span>Проверяем Horizon…</span>
    </div>
  {:else if ui.error}
    <div class="boot error">
      <div class="boot-mark">!</div>
      <h2>Не удалось запустить лаунчер</h2>
      <p>{ui.error}</p>
      <Button variant="tonal" icon="refresh" onclick={() => init()}>Повторить</Button>
    </div>
  {:else}
    <NavRail />
    <main class="main">
      <TopBar />
      <div class="content">
        {#if ui.pageLoading && !Page}
          <div class="page-loading" aria-label="Загрузка страницы">
            <span></span><span></span><span></span>
          </div>
        {:else if Page}
          {#key ui.page}
            <div class="page">
              <Page />
            </div>
          {/key}
        {/if}
      </div>
    </main>
  {/if}
  <UpdateDialog />
  <Snackbar />
</div>

<style>
  .drop-overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--surface) 80%, transparent);
    pointer-events: none;
  }
  :global([data-ui="heavy"]) .drop-overlay {
    backdrop-filter: blur(4px);
  }

  .drop-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    max-width: 540px;
    padding: 34px 40px;
    border-radius: var(--shape-3xl);
    border: 2px dashed var(--primary);
    background: var(--surface-container-high);
    color: var(--on-surface);
    text-align: center;
  }
  .drop-icon {
    width: 56px;
    height: 56px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--primary-container);
    color: var(--on-primary-container);
    font-size: 26px;
    font-weight: 700;
  }
  .drop-box strong {
    font-size: 17px;
    font-weight: 700;
  }
  .drop-box > span:last-child {
    font-size: 13px;
    color: var(--on-surface-variant);
  }

  .app {
    min-height: 100vh;
    display: flex;
    background: var(--surface);
    color: var(--on-surface);
    position: relative;
    overflow: hidden;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .content {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 6px 36px 48px;
    scroll-behavior: smooth;
  }
  .page {
    max-width: 1180px;
    margin: 0 auto;
  }
  .boot {
    margin: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
    padding: 32px;
    max-width: 560px;
  }
  .boot-mark {
    width: 68px;
    height: 68px;
    border-radius: 24px;
    display: grid;
    place-items: center;
    font-size: 30px;
    font-weight: 900;
    color: var(--on-primary-container);
    background: var(--brand-gradient);
  }
  .boot p {
    margin: 0;
    font-weight: 650;
  }
  .boot span,
  .boot.error p {
    color: var(--on-surface-variant);
    font-size: 13px;
  }
  .boot.error .boot-mark {
    background: var(--error-container);
    color: var(--on-error-container);
  }
  .boot.error h2 {
    margin: 6px 0 0;
    font-size: 19px;
  }
  .page-loading {
    display: flex;
    gap: 8px;
    padding: 48px 0;
    justify-content: center;
  }
  .page-loading span {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--primary);
    opacity: 0.4;
    animation: pulse 1s infinite ease-in-out;
  }
  .page-loading span:nth-child(2) {
    animation-delay: 120ms;
  }
  .page-loading span:nth-child(3) {
    animation-delay: 240ms;
  }
  @keyframes pulse {
    50% {
      opacity: 1;
      transform: translateY(-3px);
    }
  }
  @media (max-width: 760px) {
    .content {
      padding: 4px 16px 96px;
    }
  }
</style>
