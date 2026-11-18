<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api, errorText } from "../lib/api";
  import { notify } from "../lib/store.svelte";
  import Button from "../lib/components/Button.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import type { JavaExited, JavaProgress, JavaVersionInfo } from "../lib/types";

  let versions = $state<JavaVersionInfo[]>([]);
  let loading = $state(true);
  let error = $state("");
  let installing = $state<string | null>(null);
  let starting = $state<string | null>(null);
  let progress = $state<JavaProgress | null>(null);
  let player = $state(localStorage.getItem("gera.java.name") ?? "Player");
  let folder = $state("");
  let checking = $state<string | null>(null);

  const nameOk = $derived(
    player.trim().length >= 3 &&
      player.trim().length <= 16 &&
      /^[A-Za-z0-9_]+$/.test(player.trim()),
  );

  async function load() {
    loading = true;
    error = "";
    try {
      versions = await api.javaVersions();
      folder = await api.javaFolder();
    } catch (caught) {
      error = errorText(caught);
    } finally {
      loading = false;
    }
  }

  async function install(version: JavaVersionInfo): Promise<boolean> {
    if (installing) return false;
    installing = version.id;
    progress = { stage: "meta", label: "Начинаем…", done: 0, total: 0, filesDone: 0, filesTotal: 0, speed: 0 };
    try {
      await api.javaInstall(version.id);
      await load();
      notify(`Minecraft ${version.id} установлен.`, "success");
      return true;
    } catch (caught) {
      notify(errorText(caught), "error");
      return false;
    } finally {
      installing = null;
      progress = null;
    }
  }

  /// One click for the common case: install the newest version if needed and
  /// start the game right away.
  async function quickStart() {
    const latest = versions[0];
    if (!latest || installing || starting) return;
    if (!nameOk) {
      notify("Введите ник: 3–16 символов, латиница, цифры и _", "error");
      return;
    }
    if (!latest.installed) {
      const ok = await install(latest);
      if (!ok) return;
    }
    await play(latest);
  }

  async function cancel() {
    try {
      await api.javaCancel();
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function check(version: JavaVersionInfo) {
    if (checking) return;
    checking = version.id;
    try {
      const result = await api.javaCheck(version.id);
      if (result.ok) {
        notify(result.detail, "success");
      } else {
        const lines = result.missing.slice(0, 5).join("\n");
        notify(
          `${result.detail}\n${lines}${result.missing.length > 5 ? "\n…" : ""}`,
          "error",
        );
      }
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      checking = null;
    }
  }

  async function play(version: JavaVersionInfo) {
    if (!nameOk || starting) return;
    localStorage.setItem("gera.java.name", player.trim());
    starting = version.id;
    try {
      const result = await api.javaLaunch(version.id, player.trim());
      notify(
        `Minecraft ${version.id} запущен (PID ${result.pid}). Игра открывается — это занимает несколько секунд.`,
        "success",
      );
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      starting = null;
    }
  }

  const percent = $derived(
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.done / progress.total) * 100))
      : 0,
  );

  function megabytes(bytes: number): string {
    if (!bytes) return "0 МБ";
    return `${(bytes / 1024 / 1024).toFixed(0)} МБ`;
  }

  function speed(bytes: number): string {
    if (!bytes) return "";
    const value = bytes / 1024 / 1024;
    return value >= 1 ? `${value.toFixed(1)} МБ/с` : `${Math.round(bytes / 1024)} КБ/с`;
  }

  function stageName(stage: string): string {
    const names: Record<string, string> = {
      meta: "Метаданные",
      client: "Клиент",
      libraries: "Библиотеки",
      natives: "Нативные файлы",
      index: "Asset Index",
      assets: "Ресурсы",
      verify: "Проверка",
      java: "Java",
      virtual: "Старые ресурсы",
      done: "Готово",
    };
    return names[stage] ?? stage;
  }

  function date(iso: string): string {
    if (!iso) return "";
    const parsed = new Date(iso);
    if (Number.isNaN(parsed.getTime())) return "";
    return parsed.toLocaleDateString("ru-RU", { day: "numeric", month: "short", year: "numeric" });
  }

  onMount(() => {
    void load();
    let off: (() => void) | undefined;
    void listen<JavaProgress>("java-progress", (event) => {
      progress = event.payload;
    }).then((unlisten) => (off = unlisten));
    let offExit: (() => void) | undefined;
    void listen<JavaExited>("java-exited", (event) => {
      const info = event.payload;
      if (info.early) {
        const tail = info.tail.split("\n").slice(-4).join("\n");
        notify(
          `Minecraft закрылся сразу после запуска (код ${info.code ?? "?"}).\n${tail}\nЛог: ${info.logPath}`,
          "error",
        );
      } else if (info.code !== 0 && info.code !== null) {
        notify(`Minecraft закрылся с кодом ${info.code}. Лог: ${info.logPath}`, "error");
      }
    }).then((unlisten) => (offExit = unlisten));
    return () => {
      off?.();
      offExit?.();
    };
  });
</script>

<div class="head">
  <div>
    <h1>Minecraft Java</h1>
    <p>Официальная Java Edition от Mojang: скачивается и запускается прямо из GERA.</p>
  </div>
  <div class="head-actions">
    <Button icon="play" disabled={!nameOk || !!installing || !!starting} onclick={quickStart}>
      Играть
    </Button>
    <Button variant="text" icon="folder" onclick={() => api.openPath(folder)}>Папка игры</Button>
    <Button variant="tonal" icon="refresh" disabled={loading || !!installing} onclick={load}>
      Обновить
    </Button>
  </div>
</div>

<section class="card">
  <div class="card-head">
    <span class="head-icon primary"><Icon name="java" size={20} /></span>
    <div class="grow">
      <strong>Ваш ник</strong>
      <span>Для одиночной игры и серверов без лицензии. Вход в аккаунт Microsoft не нужен.</span>
    </div>
  </div>
  <div class="row">
    <input
      class="input"
      maxlength="16"
      bind:value={player}
      oninput={() => localStorage.setItem("gera.java.name", player)}
      placeholder="Например: gera_2347"
      aria-label="Ник в Minecraft"
    />
    {#if !nameOk}
      <span class="hint error">3–16 символов: латинские буквы, цифры и _</span>
    {/if}
  </div>
</section>

{#if installing && progress}
  <section class="card progress-card">
    <div class="card-head">
      <span class="head-icon primary"><Icon name="download" size={20} /></span>
      <div class="grow">
        <strong>Устанавливаем Minecraft {installing}</strong>
        <span>{stageName(progress.stage)} · {progress.label}</span>
      </div>
      <span class="mono">{percent}%</span>
      <Button variant="tonal" size="sm" onclick={cancel}>Отменить</Button>
    </div>
    <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
      <span style={`width:${percent}%`}></span>
    </div>
    <p class="hint">
      {#if progress.filesTotal > 0}
        Файлов: {progress.filesDone} из {progress.filesTotal} ·{" "}
      {/if}
      {#if progress.total > 0}
        {megabytes(progress.done)} из {megabytes(progress.total)}
      {/if}
      {#if progress.speed > 0}
        · {speed(progress.speed)}
      {/if}
    </p>
    <p class="hint">
      Ресурсы и Java скачиваются один раз. При следующих установках они переиспользуются.
    </p>
  </section>
{/if}

<section class="card">
  <div class="card-head">
    <span class="head-icon primary"><Icon name="builds" size={20} /></span>
    <div class="grow">
      <strong>Версии</strong>
      <span>Последние стабильные выпуски Mojang. Установка занимает 5–15 минут в зависимости от сети.</span>
    </div>
  </div>

  {#if loading}
    <p class="hint">Загружаем список версий…</p>
  {:else if error}
    <div class="empty">
      <strong>Не удалось получить список</strong>
      <span>{error}</span>
      <Button variant="tonal" icon="refresh" onclick={load}>Повторить</Button>
    </div>
  {:else}
    <div class="versions">
      {#each versions as version (version.id)}
        <div class="version" class:current={version.installed}>
          <div class="version-text">
            <strong>Minecraft {version.id}</strong>
            <span>{date(version.released)}{version.installed ? " · установлена" : ""}</span>
          </div>
          {#if version.installed}
            <Button
              size="sm"
              variant="text"
              icon="check"
              disabled={!!checking || !!installing}
              onclick={() => check(version)}
            >
              {checking === version.id ? "Проверяем…" : "Проверить"}
            </Button>
            <Button
              size="sm"
              icon="play"
              disabled={!nameOk || !!starting || !!installing}
              onclick={() => play(version)}
            >
              {starting === version.id ? "Запускаем…" : "Играть"}
            </Button>
          {:else}
            <Button
              size="sm"
              variant="tonal"
              icon="download"
              disabled={!!installing || !!starting}
              onclick={() => install(version)}
            >
              {installing === version.id ? "Установка…" : "Установить"}
            </Button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</section>

<p class="hint legal">
  Нужна лицензионная копия Minecraft Java Edition. GERA не связан с Mojang Studios и Microsoft.
  Ник без входа в аккаунт подходит для одиночной игры и серверов в offline-режиме.
</p>

<style>
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
    margin-bottom: 18px;
  }
  h1 {
    margin: 0;
    font-size: 30px;
    letter-spacing: -0.02em;
  }
  .head p {
    margin: 4px 0 0;
    color: var(--on-surface-variant);
    font-size: 13.5px;
  }
  .head-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .versions {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .version {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border-radius: var(--shape-md);
    transition: background-color var(--motion-fast) var(--ease-standard);
  }
  .version:hover {
    background: var(--surface-container);
  }
  .version.current {
    background: color-mix(in srgb, var(--primary-container) 22%, transparent);
  }
  .version-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .version-text strong {
    font-size: 14px;
  }
  .version-text span {
    color: var(--on-surface-variant);
    font-size: 12.5px;
  }
  .progress-card .bar {
    height: 8px;
    border-radius: 999px;
    background: var(--surface-container-highest);
    overflow: hidden;
    margin: 6px 0 4px;
  }
  .progress-card .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--primary);
    transition: width 160ms linear;
  }
  .mono {
    font-variant-numeric: tabular-nums;
    font-weight: 650;
  }
  .hint.error {
    color: var(--error);
  }
  .version :global(.btn) {
    flex: none;
  }
  .legal {
    margin-top: 18px;
  }
</style>
