<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { ui, saveSettings, reload, refreshHorizon, notify, openGuide } from "../lib/store.svelte";
  import { api, errorText } from "../lib/api";
  import { applyTheme } from "../lib/theme";
  import { checkUpdates, updater } from "../lib/update.svelte";
  import type { ClientRelease, Diagnostics, NativeStatus, Profile } from "../lib/types";
  import Button from "../lib/components/Button.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Switch from "../lib/components/Switch.svelte";

  const boot = $derived(ui.boot);
  const settings = $derived(boot?.settings);
  const profile = $derived(boot?.activeProfile ?? null);
  const versions = $derived(boot?.horizon.versions ?? []);
  // Only GERA's own clients are writable; versions found on disk belong to
  // another launcher and stay read-only.
  const importTargets = $derived(versions.filter((version) => version.source === "gera"));

  const accents = [
    { name: "Фиолетовый", value: "#6750A4" },
    { name: "Синий", value: "#0B57D0" },
    { name: "Бирюзовый", value: "#006A6A" },
    { name: "Зелёный", value: "#386A20" },
    { name: "Янтарный", value: "#7D5700" },
    { name: "Красный", value: "#BA1A1A" },
    { name: "Розовый", value: "#984061" },
    { name: "Графит", value: "#4A4458" },
  ];

  let customDir = $state(untrack(() => settings?.customHorizonDir ?? ""));
  let minecraftDir = $state(untrack(() => settings?.minecraftDir ?? ""));
  let serverJar = $state(untrack(() => settings?.serverJar ?? ""));
  let diag = $state<Diagnostics | null>(null);
  let diagBusy = $state(false);
  let logs = $state<{ folder: string; files: string } | null>(null);
  let showPaths = $state(false);
  let native = $state<NativeStatus | null>(null);
  let reducedMotion = $state(false);

  let installOpen = $state(false);
  let releases = $state<ClientRelease[]>([]);
  let releasesBusy = $state(false);
  let releasesError = $state("");
  let installing = $state("");
  let preparing = $state("");
  let importDir = $state("");
  let importTarget = $state("");
  let importing = $state(false);
  let dropFolder = $state("");

  $effect(() => {
    if (!importTargets.length) {
      importTarget = "";
      return;
    }
    if (!importTargets.some((version) => version.dir === importTarget)) {
      importTarget = (importTargets.find((version) => !version.ready) ?? importTargets[0]).dir;
    }
  });

  async function loadNative() {
    try {
      native = await api.nativeStatus();
    } catch {
      native = null;
    }
  }

  onMount(() => {
    void loadNative();
    void api
      .importFolder()
      .then((path) => (dropFolder = path))
      .catch(() => (dropFolder = ""));
    reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  });

  async function setUiMode(mode: "light" | "heavy") {
    await saveSettings({ uiMode: mode });
  }

  async function setTheme(theme: "system" | "light" | "dark") {
    await saveSettings({ theme });
  }

  async function setAccent(accent: string) {
    await saveSettings({ accent });
  }

  // The colour picker fires `input` while the palette is still open: show the
  // result at once, then save on `change`.
  function previewAccent(event: Event) {
    if (!settings) return;
    const value = (event.currentTarget as HTMLInputElement).value;
    applyTheme(value, settings.theme, settings.uiMode, settings.gradients);
  }

  function restoreAccent(event: Event) {
    if (!settings) return;
    const value = (event.currentTarget as HTMLInputElement).value;
    if (value.toLowerCase() === settings.accent.toLowerCase()) return;
    applyTheme(settings.accent, settings.theme, settings.uiMode, settings.gradients);
  }

  async function setGradients(value: boolean) {
    await saveSettings({ gradients: value });
  }

  async function setCloseOnLaunch(value: boolean) {
    await saveSettings({ closeOnLaunch: value });
  }

  async function saveDirs() {
    try {
      await saveSettings({
        customHorizonDir: customDir.trim() || null,
        minecraftDir: minecraftDir.trim() || null,
        serverJar: serverJar.trim() || null,
      });
      await refreshHorizon();
      notify("Пути сохранены.", "success");
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function setProfileVersion(value: string) {
    if (!profile) return;
    const next: Profile = { ...profile, horizonVersion: value || null };
    try {
      await api.saveProfile(next);
      await reload();
      notify("Версия сборки обновлена.", "success");
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function openInstall() {
    installOpen = true;
    if (releases.length || releasesBusy) return;
    await loadReleases();
  }

  async function loadReleases() {
    releasesBusy = true;
    releasesError = "";
    try {
      releases = await api.horizonReleases();
    } catch (caught) {
      releasesError = errorText(caught);
    } finally {
      releasesBusy = false;
    }
  }

  async function installRelease(release: ClientRelease) {
    if (installing) return;
    installing = release.folder;
    try {
      const folder = await api.horizonInstall(release.tag);
      notify(`Клиент ${folder} установлен и проверен.`, "success");
      installOpen = false;
      releases = [];
      await refreshHorizon();
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      installing = "";
    }
  }

  async function prepare(folder: string) {
    if (preparing) return;
    if (!minecraftDir.trim()) {
      notify("Укажите папку Minecraft Java Edition — клиент берёт ресурсы только оттуда.", "error");
      return;
    }
    preparing = folder;
    try {
      await api.horizonPrepare(folder, minecraftDir.trim(), serverJar.trim() || null);
      notify("Ресурсы подготовлены.", "success");
      await refreshHorizon();
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      preparing = "";
    }
  }

  async function importResources() {
    if (importing) return;
    if (!importDir.trim()) {
      notify("Укажите папку с готовыми ресурсами.", "error");
      return;
    }
    const target = importTarget || importTargets[0]?.dir;
    if (!target) {
      notify("Сначала установите клиент Horizon.", "error");
      return;
    }
    importing = true;
    try {
      const result = await api.horizonImport(target, importDir.trim());
      notify(
        `Ресурсы Minecraft ${result.minecraft} импортированы (${shortSize(result.bytes)}).`,
        "success",
      );
      importDir = "";
      await refreshHorizon();
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      importing = false;
    }
  }

  async function openDropFolder() {
    if (!dropFolder) return;
    try {
      await api.openPath(dropFolder);
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function runDiagnostics() {
    diagBusy = true;
    try {
      diag = await api.diagnostics();
      logs = await api.logsInfo();
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      diagBusy = false;
    }
  }

  async function openLogs() {
    try {
      await api.openLogsFolder();
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function openLastLog() {
    try {
      await api.openLastLog();
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function copyReport() {
    const lines: string[] = [];
    lines.push(`GERA Launcher ${ui.boot?.version ?? "?"}`);
    lines.push(`Платформа: ${navigator.userAgent}`);
    if (diag) {
      lines.push(`Состояние: ${diag.ok ? "всё в порядке" : "есть проблемы"}`);
      for (const item of diag.items) {
        lines.push(`- ${item.label} [${item.status}]: ${item.detail}`);
      }
      if (diag.launchApi) {
        lines.push(
          `Horizon ${diag.launchApi.horizon}, Minecraft ${diag.launchApi.minecraft}, API ${diag.launchApi.launchApi}`,
        );
      }
    } else {
      lines.push("Диагностика ещё не запускалась.");
    }
    if (logs) {
      lines.push(`Логи: ${logs.folder}`);
      lines.push(`Файлы: ${logs.files || "нет"}`);
    }
    const text = lines.join("\n");
    try {
      await navigator.clipboard.writeText(text);
      notify("Отчёт скопирован в буфер обмена.", "success");
    } catch {
      const area = document.createElement("textarea");
      area.value = text;
      document.body.appendChild(area);
      area.select();
      document.execCommand("copy");
      area.remove();
      notify("Отчёт скопирован.", "success");
    }
  }

  async function openPath(path: string) {
    try {
      await api.openPath(path);
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  function statusIcon(status: string): "check" | "warning" | "info" {
    if (status === "ok") return "check";
    if (status === "warn" || status === "error") return "warning";
    return "info";
  }

  function sourceLabel(source: string): string {
    if (source === "gera") return "GERA";
    if (source === "legacy") return "на диске";
    return "своя папка";
  }

  function shortDate(value: string): string {
    return value ? value.slice(0, 10) : "—";
  }

  function shortSize(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} МБ`;
    if (bytes >= 1024) return `${Math.round(bytes / 1024)} КБ`;
    return `${bytes} Б`;
  }
</script>

<svelte:head><title>Настройки · GERA Launcher</title></svelte:head>

<div class="page-stack settings">
  <section class="card">
    <div class="card-head">
      <span class="head-icon primary"><Icon name="palette" size={22} /></span>
      <div>
        <h2>Внешний вид</h2>
        <p>Тема и цветовая схема лаунчера. Minecraft это не меняет.</p>
      </div>
    </div>

    <div class="setting">
      <div class="setting-copy">
        <strong>Тема</strong>
        <span>Светлая, тёмная или как в системе.</span>
      </div>
      <div class="segmented" role="group" aria-label="Тема">
        <button
          type="button"
          class:active={settings?.theme === "system"}
          onclick={() => setTheme("system")}
        >
          Системная
        </button>
        <button
          type="button"
          class:active={settings?.theme === "light"}
          onclick={() => setTheme("light")}
        >
          Светлая
        </button>
        <button
          type="button"
          class:active={settings?.theme === "dark"}
          onclick={() => setTheme("dark")}
        >
          Тёмная
        </button>
      </div>
    </div>

    <div class="setting">
      <div class="setting-copy">
        <strong>Градиенты</strong>
        <span>Плавные переходы цветов в шапке, плитках и значках. Выключите для плоских заливок.</span>
      </div>
      <Switch
        checked={settings?.gradients ?? false}
        label={settings?.gradients ? "Включены" : "Выключены"}
        onchange={setGradients}
      />
    </div>

    <div class="palette-preview" aria-label="Текущая палитра">
      <div class="tone primary"><span>Primary</span></div>
      <div class="tone primary-container"><span>Primary container</span></div>
      <div class="tone secondary"><span>Secondary</span></div>
      <div class="tone secondary-container"><span>Secondary container</span></div>
      <div class="tone tertiary"><span>Tertiary</span></div>
      <div class="tone tertiary-container"><span>Tertiary container</span></div>
      <div class="tone surface"><span>Surface</span></div>
      <div class="tone outline"><span>Outline</span></div>
    </div>
  </section>

  <section class="card">
    <div class="card-head">
      <span class="head-icon secondary"><Icon name="tune" size={22} /></span>
      <div>
        <h2>Цвета</h2>
        <p>Выберите готовую палитру или задайте свой цвет. Выбор сохраняется в настройках GERA.</p>
      </div>
    </div>

    <div class="palette-grid">
      {#each accents as accent (accent.value)}
        <button
          type="button"
          class="palette"
          class:active={settings?.accent?.toLowerCase() === accent.value.toLowerCase()}
          onclick={() => setAccent(accent.value)}
        >
          <span class="palette-dot" style={`background:${accent.value}`}>
            {#if settings?.accent?.toLowerCase() === accent.value.toLowerCase()}
              <Icon name="check" size={16} />
            {/if}
          </span>
          <span class="palette-name">{accent.name}</span>
          <span class="palette-hex mono">{accent.value}</span>
        </button>
      {/each}
    </div>

    <div class="setting">
      <div class="setting-copy">
        <strong>Свой цвет</strong>
        <span>Любой оттенок: из него Material You построит Primary, Secondary, Tertiary и поверхности.</span>
      </div>
      <div class="custom-color">
        <input
          class="color-input"
          type="color"
          aria-label="Свой акцентный цвет"
          value={settings?.accent ?? "#6750A4"}
          oninput={previewAccent}
          onchange={(event) => setAccent((event.currentTarget as HTMLInputElement).value)}
          onblur={restoreAccent}
        />
        <span class="mono">{settings?.accent ?? "#6750A4"}</span>
      </div>
    </div>
  </section>

  <section class="card">
    <div class="card-head">
      <span class="head-icon tertiary"><Icon name="memory" size={22} /></span>
      <div>
        <h2>Производительность интерфейса</h2>
        <p>Влияет только на GERA. Игра запускается одинаково в обоих режимах.</p>
      </div>
    </div>

    <div class="mode-grid">
      <button
        type="button"
        class="mode"
        class:active={settings?.uiMode === "light"}
        onclick={() => setUiMode("light")}
      >
        <span class="mode-top">
          <Icon name="memory" size={22} />
          {#if settings?.uiMode === "light"}<Icon name="check" size={18} />{/if}
        </span>
        <strong>Light</strong>
        <span>Минимум анимаций и эффектов, меньше RAM. Рекомендуется для слабых ПК.</span>
      </button>

      <button
        type="button"
        class="mode"
        class:active={settings?.uiMode === "heavy"}
        onclick={() => setUiMode("heavy")}
      >
        <span class="mode-top">
          <Icon name="heart" size={22} />
          {#if settings?.uiMode === "heavy"}<Icon name="check" size={18} />{/if}
        </span>
        <strong>Heavy</strong>
        <span>Полные анимации, размытия и выразительные переходы. Больше нагрузки на GPU.</span>
      </button>
    </div>

    <p class="mode-note">
      {settings?.uiMode === "heavy"
        ? "Сейчас включён Heavy: карточки появляются плавно, панели размываются."
        : "Сейчас включён Light: переходы мгновенные, размытий и теней нет."}
      {#if reducedMotion}
        Система просит уменьшить движение — анимации отключены в любом режиме.
      {/if}
    </p>
  </section>

  <section class="card">
    <div class="card-head">
      <span class="head-icon primary"><Icon name="launch" size={22} /></span>
      <div>
        <h2>Игровые настройки</h2>
        <p>Версия клиента для активной сборки и ресурсы для подготовки.</p>
      </div>
    </div>

    <div class="setting">
      <div class="setting-copy">
        <strong>Версия для активной сборки</strong>
        <span>{profile ? `Сборка «${profile.name}»` : "Сборка не выбрана"}</span>
      </div>
      <select
        class="select compact"
        disabled={!profile}
        value={profile?.horizonVersion ?? ""}
        onchange={(event) => setProfileVersion((event.currentTarget as HTMLSelectElement).value)}
      >
        <option value="">Авто — новейшая готовая</option>
        {#each versions as version (version.tag)}
          <option value={version.tag}>
            {version.tag}{version.ready ? "" : " — без ресурсов"}
          </option>
        {/each}
      </select>
    </div>

    <div class="setting">
      <div class="setting-copy">
        <strong>Закрывать лаунчер при запуске игры</strong>
        <span>
          Окно GERA скроется сразу после старта игры и закроется через секунду. Игра продолжит
          работать, а лаунчер не будет отнимать процессор у загрузки ресурсов.
        </span>
      </div>
      <Switch
        checked={settings?.closeOnLaunch ?? false}
        label={settings?.closeOnLaunch ? "Включено" : "Выключено"}
        onchange={setCloseOnLaunch}
      />
    </div>

    <div class="field">
      <label for="mc-dir">Папка Minecraft Java Edition</label>
      <div class="inline">
        <input
          id="mc-dir"
          class="input mono"
          bind:value={minecraftDir}
          placeholder="~/.minecraft"
          spellcheck="false"
        />
        <Button variant="tonal" onclick={saveDirs}>Сохранить</Button>
      </div>
      <span class="hint">
        Нужна только для подготовки ресурсов. GERA не скачивает файлы игры и не обходит проверку
        владения.
      </span>
    </div>

    <div class="field">
      <label for="server-jar">Server jar Minecraft (необязательно)</label>
      <div class="inline">
        <input
          id="server-jar"
          class="input mono"
          bind:value={serverJar}
          placeholder="/path/to/server.jar"
          spellcheck="false"
        />
        <Button variant="tonal" onclick={saveDirs}>Сохранить</Button>
      </div>
    </div>

    <div class="setting">
      <div class="setting-copy">
        <strong>Сборщик нативных модов</strong>
        <span>
          {#if native?.rustc && native?.cargo}
            Готов: {native.rustc} · свободно {native.diskFreeGb} ГБ
          {:else}
            Не хватает: {!native?.rustc ? "rustc " : ""}{!native?.cargo ? "cargo" : ""}
          {/if}
        </span>
      </div>
      <Button size="sm" variant="tonal" icon="refresh" onclick={loadNative}>Проверить</Button>
    </div>

    <div class="row">
      <Button size="sm" variant="text" icon="open" onclick={() => openPath(`${boot?.dataDir}/backups/mods`)}>
        Резервные копии модов
      </Button>
      <Button size="sm" variant="text" icon="open" onclick={() => openPath(`${boot?.dataDir}/native`)}>
        Папка нативных сборок
      </Button>
    </div>
  </section>

  <section class="card">
    <div class="card-head">
      <span class="head-icon secondary"><Icon name="folder" size={22} /></span>
      <div>
        <h2>Хранилище</h2>
        <p>Папки GERA и перенос готовых ресурсов между компьютерами.</p>
      </div>
    </div>

    <div class="list">
      <div class="list-row">
        <Icon name="folder" size={18} />
        <div class="grow">
          <strong>Папка лаунчера</strong>
          <span class="mono">{boot?.dataDir}</span>
        </div>
        <Button
          size="sm"
          variant="text"
          icon="open"
          title="Открыть"
          onclick={() => openPath(boot?.dataDir ?? "")}
        />
      </div>
    </div>
    <p class="hint" style="margin:10px 0 0">
      Настройки, сборки, клиенты и логи GERA хранятся отдельно от игровых папок. Миры Horizon лежат в
      игровой папке сборки, и GERA их не переносит.
    </p>

    <hr class="divider" style="margin:20px 0" />

    <div class="field">
      <label for="import-dir">Импорт готовых ресурсов</label>
      <div class="inline">
        <input
          id="import-dir"
          class="input mono"
          bind:value={importDir}
          placeholder="/path/to/26.3 или 26.3.zip"
          spellcheck="false"
        />
        {#if !importTargets.length}
          <span class="hint">Сначала установите клиент — импортировать можно только в сборки GERA.</span>
        {:else}
          <select class="select compact" bind:value={importTarget}>
            {#each importTargets as version (version.dir)}
              <option value={version.dir}>
                {version.tag}{version.ready ? " — уже есть" : ""}
              </option>
            {/each}
          </select>
        {/if}
        <Button variant="tonal" disabled={importing} onclick={importResources}>
          {importing ? "Импортируем…" : "Импортировать"}
        </Button>
      </div>
      <span class="hint">
        Укажите папку с <span class="mono">asset-index.json</span> или zip-архив с ней: саму
        <span class="mono">26.3</span>, папку <span class="mono">horizon-client</span> или папку
        версии. Ресурсы скопируются без повторной подготовки. Папку
        <span class="mono">.minecraft</span> указывайте выше и жмите «Подготовить».
      </span>
      {#if dropFolder}
        <div class="inline" style="margin-top:6px">
          <span class="hint mono grow" style="overflow-wrap:anywhere">
            Или положите папку или zip в {dropFolder} и нажмите «Играть» на главной.
          </span>
          <Button size="sm" variant="text" icon="folder" onclick={openDropFolder}>
            Открыть папку
          </Button>
        </div>
      {/if}
    </div>

    <div class="field" style="margin-top:16px">
      <label for="custom-dir">Своя папка версии Horizon</label>
      <div class="inline">
        <input
          id="custom-dir"
          class="input mono"
          bind:value={customDir}
          placeholder="/path/to/nightly-…"
          spellcheck="false"
        />
        <Button variant="tonal" onclick={saveDirs}>Сохранить</Button>
      </div>
      <span class="hint">Папка должна содержать horizon-client. Игровые миры не меняются.</span>
    </div>
  </section>

  <section class="card">
    <div class="card-head">
      <span class="head-icon tertiary"><Icon name="update" size={22} /></span>
      <div>
        <h2>Обновления</h2>
        <p>Официальные сборки клиента Horizon из GitHub Releases.</p>
      </div>
      <Button size="sm" variant="tonal" icon="download" onclick={openInstall}>
        Установить сборку
      </Button>
    </div>

    <div class="versions">
      {#each versions as version (version.tag)}
        <div class="version">
          <span class="dot" class:ok={version.ready} class:warn={!version.ready}></span>
          <div class="grow">
            <strong>{version.tag}</strong>
            <span class="muted small">
              {sourceLabel(version.source)} · Minecraft {version.minecraft || "—"}
              {#if showPaths}
                · <span class="mono">{version.dir}</span>
              {/if}
            </span>
          </div>
          <span class="badge" class:ok={version.ready} class:warn={!version.ready}>
            {version.ready ? "ресурсы готовы" : "нет ресурсов"}
          </span>
          {#if !version.ready}
            <Button
              size="sm"
              variant="tonal"
              icon="launch"
              disabled={!!preparing}
              onclick={() => prepare(version.dir)}
            >
              {preparing === version.dir ? "Готовим…" : "Подготовить"}
            </Button>
          {/if}
        </div>
      {:else}
        <div class="empty">
          <strong>Клиент не найден</strong>
          <span>Нажмите «Установить сборку» — GERA загрузит официальный клиент Horizon.</span>
        </div>
      {/each}
    </div>

    <div class="row" style="margin-top:14px">
      <button type="button" class="link-button" onclick={() => (showPaths = !showPaths)}>
        {showPaths ? "Скрыть пути" : "Показать пути"}
      </button>
      <span class="spacer"></span>
      <Button size="sm" variant="text" icon="refresh" disabled={ui.busy} onclick={refreshHorizon}>
        Проверить снова
      </Button>
    </div>
  </section>

  <section class="card">
    <div class="card-head">
      <span class="head-icon primary"><Icon name="info" size={22} /></span>
      <div>
        <h2>О программе</h2>
        <p>GERA Launcher {boot?.version ?? "0.9.0"} · {boot?.appInfo.target ?? "linux-x86_64"}</p>
      </div>
    </div>

    <div class="about-head">
      <span class="mark">G</span>
      <div class="about-copy">
        <strong>GERA Launcher</strong>
        <span class="muted small">
          Самостоятельный лаунчер для Minecraft Rust Client Horizon. GERA сам управляет клиентом,
          сборками, игровыми папками и модами. Официальный Horizon Launcher не требуется.
        </span>
      </div>
    </div>

    <div class="list">
      <div class="list-row">
        <Icon name="chip" size={18} />
        <span class="grow"><strong>Клиент</strong><span>Horizon — Minecraft: Java Edition на Rust</span></span>
        <Button
          size="sm"
          variant="text"
          icon="open"
          onclick={() => api.openUrl("https://github.com/horizon-rust/horizon")}
        >
          GitHub
        </Button>
      </div>
      <div class="list-row">
        <Icon name="mods" size={18} />
        <span class="grow"><strong>Каталог модов</strong><span>Horizon Mods Registry</span></span>
        <Button
          size="sm"
          variant="text"
          icon="open"
          onclick={() => api.openUrl("https://github.com/horizon-rust/horizon-mods")}
        >
          GitHub
        </Button>
      </div>
      <div class="list-row">
        <Icon name="info" size={18} />
        <span class="grow">
          <strong>Лицензии</strong>
          <span>
            GERA распространяется по MIT License, Horizon — по LGPL-3.0. Minecraft — торговая
            марка Mojang Studios и Microsoft.
          </span>
        </span>
      </div>
      <div class="list-row">
        <Icon name="update" size={18} />
        <span class="grow">
          <strong>Обновления</strong>
          <span>Версия {boot?.version ?? "0.9.0"} · проверка через GitHub Releases</span>
        </span>
        <Button
          size="sm"
          variant="tonal"
          icon="refresh"
          disabled={updater.checking}
          onclick={() => checkUpdates(true)}
        >
          {updater.checking ? "Проверяем…" : "Проверить"}
        </Button>
      </div>
    </div>

    <p class="hint" style="margin-top:10px">
      GERA не связан с Mojang Studios и Microsoft и не обходит проверку владения игрой. Для запуска
      нужна легальная копия Minecraft Java Edition.
    </p>

    <hr class="divider" style="margin:20px 0" />

    <div class="diag-head">
      <div>
        <strong>Диагностика</strong>
        <span class="hint">Проверки запускаются только по кнопке: фоновых опросов нет.</span>
      </div>
      <div class="row">
        {#if diag}
          <span class="badge" class:ok={diag.ok} class:warn={!diag.ok}>
            {diag.ok ? "всё в порядке" : "есть проблемы"}
          </span>
        {/if}
        <Button variant="tonal" size="sm" icon="check" onclick={runDiagnostics} disabled={diagBusy}>
          {diagBusy ? "Проверяем…" : "Запустить проверку"}
        </Button>
      </div>
    </div>

    <div class="row log-actions">
      <Button variant="text" size="sm" icon="folder" onclick={openLogs}>Открыть папку логов</Button>
      <Button variant="text" size="sm" icon="terminal" onclick={openLastLog}>Открыть последний лог</Button>
      <Button variant="text" size="sm" icon="copy" onclick={copyReport}>Скопировать отчёт</Button>
    </div>
    {#if logs}
      <p class="hint">Логи: <span class="mono">{logs.folder}</span></p>
      {#if logs.files}<p class="hint">Файлы: {logs.files}</p>{/if}
    {/if}

    {#if diag}
      <div class="list diag-list">
        {#each diag.items as item (item.label)}
          <div class="list-row">
            <span class="diag-icon {item.status}">
              <Icon name={statusIcon(item.status)} size={16} />
            </span>
            <div class="grow">
              <strong>{item.label}</strong>
              <span>{item.detail}</span>
            </div>
          </div>
        {/each}
      </div>
      {#if diag.launchApi}
        <div class="api">
          <span class="muted small">Launch API</span>
          <strong>{diag.launchApi.launchApi}</strong>
          <span class="muted small">
            Horizon {diag.launchApi.horizon} · Minecraft {diag.launchApi.minecraft}
          </span>
        </div>
      {/if}
    {/if}
  </section>

  <section class="card">
    <div class="card-head">
      <span class="head-icon tertiary"><Icon name="info" size={22} /></span>
      <div>
        <h2>Документы и помощь</h2>
        <p>Условия использования, политика конфиденциальности и лицензия проекта.</p>
      </div>
    </div>

    <div class="list">
      <div class="list-row">
        <Icon name="play" size={18} />
        <span class="grow">
          <strong>Как подготовить Minecraft</strong>
          <span>Короткий гайд: Java, клиент Horizon, сборка и первый запуск</span>
        </span>
        <Button size="sm" variant="tonal" icon="open" onclick={openGuide}>Открыть</Button>
      </div>
      <div class="list-row">
        <Icon name="info" size={18} />
        <span class="grow">
          <strong>Условия использования</strong>
          <span>Версия 1.0 · правила использования GERA Launcher</span>
        </span>
        <Button
          size="sm"
          variant="text"
          icon="open"
          onclick={() => api.openUrl("https://github.com/gera2347/gera-launcher/blob/main/docs/TERMS.md")}
        >
          Открыть
        </Button>
      </div>
      <div class="list-row">
        <Icon name="info" size={18} />
        <span class="grow">
          <strong>Политика конфиденциальности</strong>
          <span>Какие данные GERA хранит на компьютере и куда обращается</span>
        </span>
        <Button
          size="sm"
          variant="text"
          icon="open"
          onclick={() => api.openUrl("https://github.com/gera2347/gera-launcher/blob/main/docs/PRIVACY.md")}
        >
          Открыть
        </Button>
      </div>
      <div class="list-row">
        <Icon name="info" size={18} />
        <span class="grow">
          <strong>Лицензия</strong>
          <span>MIT License — исходный код GERA Launcher</span>
        </span>
        <Button
          size="sm"
          variant="text"
          icon="open"
          onclick={() => api.openUrl("https://github.com/gera2347/gera-launcher/blob/main/LICENSE")}
        >
          Открыть
        </Button>
      </div>
    </div>
  </section>
</div>

<Dialog open={installOpen} title="Установить клиент Horizon" wide onclose={() => (installOpen = false)}>
  <p class="muted small">
    Официальные сборки из GitHub Releases. Файл проверяется по SHA-256 и распаковывается в папку
    клиентов GERA.
  </p>

  {#if releasesBusy}
    <p class="muted small">Запрашиваем GitHub…</p>
  {:else if releasesError}
    <div class="banner" role="alert">
      <Icon name="warning" size={18} />
      <span>{releasesError}</span>
      <button type="button" class="link-button" onclick={loadReleases}>Повторить</button>
    </div>
  {:else}
    <div class="release-list">
      {#each releases as release (release.folder)}
        <div class="release">
          <div class="grow">
            <strong>
              {release.nightly ? "Nightly" : release.tag}
              {#if release.nightly}<span class="chip primary">ночная</span>{/if}
            </strong>
            <span class="muted small">{shortDate(release.published)} · {shortSize(release.size)}</span>
          </div>
          {#if release.installed}
            <span class="chip ok">установлен</span>
          {:else}
            <Button
              size="sm"
              variant="tonal"
              icon="add"
              disabled={!!installing}
              onclick={() => installRelease(release)}
            >
              {installing === release.folder ? "Устанавливаем…" : "Установить"}
            </Button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <p class="muted small" style="margin-top:14px">
    После установки нажмите «Подготовить» у версии — horizon-prepare соберёт ресурсы из вашей копии
    Minecraft.
  </p>
</Dialog>

<style>
  .log-actions {
    margin-top: 10px;
    flex-wrap: wrap;
  }
  .settings {
    max-width: 920px;
  }

  .card-head {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    margin-bottom: 8px;
  }
  .card-head > div {
    flex: 1;
    min-width: 0;
  }
  .card-head h2 {
    margin: 0;
    font-size: 19px;
    font-weight: 750;
    letter-spacing: -0.02em;
  }
  .card-head p {
    margin: 3px 0 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--on-surface-variant);
  }
  .card-head > :global(.btn) {
    flex: 0 0 auto;
  }
  .head-icon {
    width: 44px;
    height: 44px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: var(--shape-md);
  }
  .head-icon.primary {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .head-icon.secondary {
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  .head-icon.tertiary {
    background: var(--tertiary-container);
    color: var(--on-tertiary-container);
  }

  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 14px 0;
    border-top: 1px solid var(--outline-variant);
    margin-top: 8px;
  }
  .setting:first-of-type {
    border-top: none;
  }
  .setting.column {
    flex-direction: column;
    align-items: stretch;
    gap: 12px;
  }
  .setting-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .setting-copy strong {
    font-size: 13.5px;
    font-weight: 650;
  }
  .setting-copy span {
    font-size: 12px;
    color: var(--on-surface-variant);
    max-width: 56ch;
  }

  .palette-preview {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 6px;
    margin-top: 16px;
  }
  .tone {
    min-height: 58px;
    padding: 10px 12px;
    border-radius: var(--shape-md);
    display: flex;
    align-items: flex-end;
    font-size: 10.5px;
    font-weight: 650;
    letter-spacing: 0.02em;
  }
  .tone.primary {
    background: var(--primary);
    color: var(--on-primary);
  }
  .tone.primary-container {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .tone.secondary {
    background: var(--secondary);
    color: var(--on-secondary);
  }
  .tone.secondary-container {
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  .tone.tertiary {
    background: var(--tertiary);
    color: var(--on-tertiary);
  }
  .tone.tertiary-container {
    background: var(--tertiary-container);
    color: var(--on-tertiary-container);
  }
  .tone.surface {
    background: var(--surface-container);
    color: var(--on-surface);
  }
  .tone.outline {
    background: var(--outline);
    color: var(--surface);
  }

  .palette-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 8px;
    margin: 14px 0 6px;
  }
  .palette {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 12px;
    border: none;
    border-radius: var(--shape-lg);
    background: var(--surface-container);
    color: var(--on-surface);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background-color var(--motion-fast) var(--ease-standard),
      transform var(--motion-fast) var(--ease-standard);
  }
  .palette:hover {
    background: var(--surface-container-high);
  }
  .palette.active {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .palette.active .palette-hex {
    color: var(--on-primary-container);
    opacity: 0.75;
  }
  .palette.active:hover {
    background: color-mix(in srgb, var(--on-primary-container) 8%, var(--primary-container));
  }
  .palette-dot {
    width: 38px;
    height: 38px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: #fff;
    margin-bottom: 6px;
  }
  .palette-name {
    font-size: 12.5px;
    font-weight: 650;
  }
  .palette-hex {
    font-size: 10.5px;
    color: var(--on-surface-variant);
  }
  .custom-color {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .custom-color .color-input {
    width: 56px;
    height: 44px;
    border-radius: var(--shape-sm);
  }

  .mode-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    margin-top: 14px;
  }
  .mode {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 16px;
    border: none;
    border-radius: var(--shape-xl);
    background: var(--surface-container);
    color: var(--on-surface);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background-color var(--motion-fast) var(--ease-standard),
      transform var(--motion-fast) var(--ease-standard);
  }
  .mode:hover {
    background: var(--surface-container-high);
  }
  .mode.active {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .mode-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .mode strong {
    font-size: 15px;
    font-weight: 750;
  }
  .mode > span:last-child {
    font-size: 11.5px;
    line-height: 1.45;
    opacity: 0.8;
  }
  .mode-note {
    margin: 12px 0 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--on-surface-variant);
  }

  .inline {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .inline .input {
    flex: 1 1 260px;
  }

  .versions {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 10px;
  }
  .version {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
    flex-wrap: wrap;
  }
  .version .grow {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .version strong {
    font-size: 12.5px;
    font-weight: 650;
  }
  .version .mono {
    font-size: 10.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .about-head {
    display: flex;
    align-items: center;
    gap: 14px;
    margin: 14px 0 6px;
  }
  .mark {
    width: 52px;
    height: 52px;
    display: grid;
    place-items: center;
    border-radius: 18px;
    font-size: 23px;
    font-weight: 900;
    color: var(--on-primary-container);
    background: var(--brand-gradient);
  }
  .about-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .about-copy strong {
    font-size: 16px;
    font-weight: 750;
  }

  .diag-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }
  .diag-head > div:first-child {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .diag-head strong {
    font-size: 13.5px;
    font-weight: 650;
  }
  .diag-list {
    margin-top: 8px;
  }
  .diag-icon {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--surface-container-high);
    color: var(--on-surface-variant);
    flex: 0 0 auto;
  }
  .diag-icon.ok {
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  .diag-icon.warn,
  .diag-icon.error {
    background: var(--error-container);
    color: var(--on-error-container);
  }
  .api {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 12px;
    padding: 12px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
  }
  .api strong {
    font-size: 13px;
  }

  .release-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
  }
  .release {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
  }
  .release .grow {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .release strong {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13.5px;
    font-weight: 650;
  }

  @media (max-width: 760px) {
    .setting {
      flex-direction: column;
      align-items: stretch;
    }
    .select.compact {
      max-width: none;
      width: 100%;
    }
    .palette-preview {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .mode-grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .card-head {
      flex-wrap: wrap;
    }
  }
</style>
