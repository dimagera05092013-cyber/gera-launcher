<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { ui, notify, reload } from "../lib/store.svelte";
  import { go } from "../lib/router.svelte";
  import { api, errorText } from "../lib/api";
  import type {
    BuiltinMod,
    InstalledMod,
    ModProject,
    ModVersion,
    NativeBuild,
    NativeCheck,
    NativeStatus,
  } from "../lib/types";
  import Icon from "../lib/components/Icon.svelte";
  import IconButton from "../lib/components/IconButton.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";

  const profiles = $derived(ui.boot?.profiles ?? []);
  const activeId = $derived(ui.boot?.activeProfile?.id ?? null);
  let selectedId = $state("");
  const selected = $derived(profiles.find((item) => item.id === selectedId) ?? null);

  // Default to the active build and keep the choice valid if builds change.
  $effect(() => {
    if (!profiles.length) {
      selectedId = "";
      return;
    }
    if (!profiles.some((item) => item.id === selectedId)) {
      selectedId = activeId ?? profiles[0].id;
    }
  });

  // Installed mods belong to a build: reload the list when the picker changes.
  $effect(() => {
    const id = selectedId;
    if (!id) {
      installed = [];
      builtin = [];
      return;
    }
    void loadInstalled();
  });

  let projects = $state<ModProject[]>([]);
  let installed = $state<InstalledMod[]>([]);
  let builtin = $state<BuiltinMod[]>([]);
  let loading = $state(true);
  let refreshing = $state(false);
  let error = $state("");
  let query = $state("");
  let kind = $state<"mod" | "native-mod">("mod");
  let category = $state("");
  let sort = $state<"popular" | "name" | "updated">("popular");
  let onlyInstalled = $state(false);
  let busyId = $state("");
  let detail = $state<ModProject | null>(null);
  let removing = $state<InstalledMod | null>(null);
  let removeBusy = $state(false);
  let failedIcons = $state<Record<string, boolean>>({});
  let native = $state<NativeStatus | null>(null);
  let check = $state<NativeCheck | null>(null);
  let target = $state<{ project: ModProject; version: ModVersion } | null>(null);
  let checking = $state(false);
  let allowMismatch = $state(false);
  let showJob = $state(false);
  let removingBuild = $state<NativeBuild | null>(null);
  let buildBusy = $state(false);
  let timer: ReturnType<typeof setInterval> | null = null;

  const contentMods = $derived(projects.filter((p) => p.kind === "mod"));
  const nativeMods = $derived(projects.filter((p) => p.kind === "native-mod"));
  const current = $derived(kind === "mod" ? contentMods : nativeMods);

  const categories = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const project of current) {
      for (const name of project.categories) {
        counts.set(name, (counts.get(name) ?? 0) + 1);
      }
    }
    return [...counts.entries()]
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .slice(0, 12);
  });

  const visible = $derived.by(() => {
    const text = query.trim().toLowerCase();
    const list = current
      .filter((p) => !category || p.categories.includes(category))
      .filter((p) => {
        if (!text) return true;
        return (
          p.name.toLowerCase().includes(text) ||
          p.summary.toLowerCase().includes(text) ||
          p.summaryRu.toLowerCase().includes(text) ||
          p.authors.join(" ").toLowerCase().includes(text)
        );
      })
      .filter((p) => !onlyInstalled || (kind === "mod" && !!installedFileOf(p)));
    return list.sort((a, b) => {
      if (sort === "name") return a.name.localeCompare(b.name, "ru");
      if (sort === "updated") {
        return (b.versions[0]?.published ?? "").localeCompare(a.versions[0]?.published ?? "");
      }
      return b.downloads - a.downloads || a.name.localeCompare(b.name, "ru");
    });
  });

  function installedFileOf(project: ModProject): string | null {
    const names = new Set(project.versions.map((v) => v.file.name));
    return installed.find((m) => names.has(m.file))?.file ?? null;
  }

  function installedRecord(project: ModProject): InstalledMod | null {
    const file = installedFileOf(project);
    return file ? installed.find((m) => m.file === file) ?? null : null;
  }

  function hasUpdate(project: ModProject): boolean {
    const record = installedRecord(project);
    const latest = project.versions[0]?.version;
    return !!record && !!latest && record.version !== latest;
  }

  function categoryLabel(name: string): string {
    return name.replace(/-/g, " ");
  }

  function sizeText(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} МБ`;
    if (bytes >= 1024) return `${Math.round(bytes / 1024)} КБ`;
    return `${bytes} Б`;
  }

  function downloadsText(value: number): string {
    if (value >= 1000000) return `${(value / 1000000).toFixed(1)} млн`;
    if (value >= 1000) return `${Math.round(value / 1000)} тыс.`;
    return String(value);
  }

  function authorText(project: ModProject): string {
    return project.authors.length ? project.authors.join(", ") : "неизвестный автор";
  }

  async function load(force = false) {
    if (force) refreshing = true;
    else loading = true;
    error = "";
    try {
      projects = await api.modsCatalog(force);
    } catch (caught) {
      error = errorText(caught);
    } finally {
      loading = false;
      refreshing = false;
    }
  }

  async function loadInstalled() {
    if (!selectedId) {
      installed = [];
      builtin = [];
      return;
    }
    try {
      const [, list] = await api.modsInstalled(selectedId);
      installed = list;
      builtin = await api.modsBuiltin(selectedId);
    } catch (caught) {
      installed = [];
      builtin = [];
      notify(errorText(caught), "error");
    }
  }

  onMount(() => {
    void load();
    void loadNative();
    timer = setInterval(() => {
      if (native?.job.running) void pollJob();
    }, 1500);
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  async function loadNative() {
    try {
      native = await api.nativeStatus();
      if (native.job.running) showJob = true;
    } catch {
      /* the build panel simply stays empty */
    }
  }

  async function pollJob() {
    if (!native) return;
    try {
      const wasRunning = native.job.running;
      const job = await api.nativeLog();
      native = { ...native, job };
      if (wasRunning && !job.running) await loadNative();
    } catch {
      /* the next tick tries again */
    }
  }

  function builtOf(project: ModProject): NativeBuild | null {
    return native?.builds.find((build) => build.projectId === project.id) ?? null;
  }

  async function openBuild(project: ModProject, version?: ModVersion) {
    if (!selected) {
      notify("Сначала выберите сборку.", "error");
      return;
    }
    if (checking) return;
    checking = true;
    busyId = project.id;
    allowMismatch = false;
    try {
      const wanted = version ?? project.versions[0];
      const result = await api.nativeCheck(selected.id, project.id, wanted.version);
      target = { project, version: wanted };
      check = result;
      if (result.built) showJob = true;
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      checking = false;
      busyId = "";
    }
  }

  async function startBuild() {
    if (!selected || !target || buildBusy) return;
    buildBusy = true;
    try {
      await api.nativeBuild(
        selected.id,
        target.project.id,
        target.version.version,
        allowMismatch,
      );
      target = null;
      check = null;
      showJob = true;
      await loadNative();
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      buildBusy = false;
    }
  }

  async function cancelBuild() {
    try {
      await api.nativeCancel();
      await loadNative();
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function useBuild(folder: string) {
    if (!selected) return;
    try {
      await api.nativeUse(selected.id, folder);
      await reload();
      notify("Сборка переключена на нативный клиент.", "success");
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function confirmRemoveBuild() {
    if (!removingBuild) return;
    try {
      const builds = await api.nativeRemove(removingBuild.folder);
      native = native ? { ...native, builds } : native;
      notify("Сборка удалена. Базовая версия и миры не тронуты.", "success");
      removingBuild = null;
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  function jobPercent(): number {
    if (!native) return 0;
    const job = native.job;
    if (job.compileTotal && job.compileDone) {
      return Math.min(99, Math.round((job.compileDone / job.compileTotal) * 100));
    }
    return Math.round((job.step / Math.max(1, job.total)) * 100);
  }

  async function install(project: ModProject, version?: ModVersion) {
    if (busyId) return;
    if (!selected) {
      notify("Сначала выберите сборку — моды ставятся в её игровую папку.", "error");
      return;
    }
    busyId = project.id;
    try {
      await api.modsInstall(selectedId, project.id, version?.version ?? null);
      await loadInstalled();
      notify(`«${project.name}» установлен.`, "success");
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      busyId = "";
    }
  }

  async function confirmRemove() {
    if (!removing) return;
    removeBusy = true;
    try {
      installed = await api.modsRemove(selectedId, removing.file);
      notify("Мод удалён, копия сохранена в резервной папке GERA.", "success");
      removing = null;
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      removeBusy = false;
    }
  }

  function badge(name: string) {
    failedIcons = { ...failedIcons, [name]: true };
  }

  function stop(event: MouseEvent) {
    event.stopPropagation();
  }

  function openDetail(project: ModProject) {
    detail = project;
  }

  function onCardKey(event: KeyboardEvent, project: ModProject) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      openDetail(project);
    }
  }
</script>

<svelte:head><title>Моды · GERA Launcher</title></svelte:head>

<div class="page-stack">
  <div class="page-head">
    <div>
      <h2>Каталог модов</h2>
      <p>
        {#if selected}
          Установка в сборку <strong>{selected.name}</strong> · {selected.playerName}. Русские описания —
          перевод GERA.
        {:else}
          Нет сборок — установка станет доступна после её создания.
        {/if}
      </p>
    </div>
    <div class="row">
      {#if profiles.length}
        <select class="select compact build-select" bind:value={selectedId} aria-label="Сборка для установки">
          {#each profiles as item (item.id)}
            <option value={item.id}>{item.name}{item.id === activeId ? " · активная" : ""}</option>
          {/each}
        </select>
      {:else}
        <Button variant="tonal" icon="builds" onclick={() => go("builds")}>Сборки</Button>
      {/if}
      <IconButton
        icon="refresh"
        label="Обновить каталог"
        variant="tonal"
        disabled={refreshing}
        onclick={() => load(true)}
      />
    </div>
  </div>

  {#if showJob && native}
    {@const job = native.job}
    <section class="card job" class:failed={!!job.error}>
      <div class="job-head">
        <div>
          <p class="eyebrow">Сборка нативного клиента</p>
          <strong>{job.error ? "Сборка не удалась" : job.running ? job.stage : "Готово"}</strong>
          <p class="muted small">{job.detail}</p>
        </div>
        <div class="row">
          {#if job.running}
            <Button variant="text" size="sm" onclick={cancelBuild}>Отмена</Button>
          {:else}
            <IconButton
              icon="close"
              label="Скрыть"
              variant="text"
              size="sm"
              onclick={() => (showJob = false)}
            />
          {/if}
        </div>
      </div>
      {#if job.running}
        <div class="bar" aria-hidden="true"><span style={`width:${jobPercent()}%`}></span></div>
        <p class="muted small">
          Шаг {Math.min(job.step, job.total)} из {job.total}
          {#if job.compileTotal && job.compileDone}
            · крейтов {job.compileDone} из {job.compileTotal}
          {/if}
          {#if job.startedAt}
            · прошло {Math.max(1, Math.round((Date.now() - job.startedAt) / 60000))} мин
          {/if}
        </p>
      {:else if job.error}
        <pre class="log error">{job.error}</pre>
      {/if}
      {#if job.log.length}
        <pre class="log">{job.log.slice(-12).join("\n")}</pre>
      {/if}
      {#if job.folder && !job.running && !job.error}
        <div class="job-done">
          <span class="mono small">{job.folder}</span>
          <Button variant="filled" size="sm" icon="play" onclick={() => useBuild(job.folder!)}>
            Использовать в сборке
          </Button>
        </div>
      {/if}
    </section>
  {/if}

  {#if native?.builds.length}
    <section class="card">
      <div class="section-head">
        <h2>Нативные сборки</h2>
        <span class="count">{native.builds.length}</span>
      </div>
      <div class="rows">
        {#each native.builds as build (build.folder)}
          <div class="row-item">
            <span class="row-icon"><Icon name="chip" size={18} /></span>
            <div class="row-text">
              <strong>{build.name} {build.version}</strong>
              <span>
                Horizon {build.horizon.slice(0, 7)} · база {build.base} · {build.folder}
              </span>
            </div>
            {#if selected}
              <button type="button" class="link-button" onclick={() => useBuild(build.folder)}>
                В сборку
              </button>
            {/if}
            <IconButton
              icon="delete"
              label="Удалить сборку"
              variant="text"
              size="sm"
              onclick={() => (removingBuild = build)}
            />
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if builtin.length}
    <section class="card">
      <div class="section-head">
        <h2>Встроенные моды</h2>
        <span class="count">{builtin.filter((mod) => mod.enabled).length}/{builtin.length}</span>
      </div>
      <div class="rows">
        {#each builtin as mod (mod.id)}
          <div class="row-item">
            <span class="row-icon secondary"><Icon name="mods" size={18} /></span>
            <div class="row-text">
              <strong>{mod.title}</strong>
              <span>встроен в клиент Horizon · {mod.enabled ? "включён" : "выключен"}</span>
            </div>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if installed.length}
    <section class="card">
      <div class="section-head">
        <h2>Установлено</h2>
        <span class="count">{installed.length}</span>
      </div>
      <div class="rows">
        {#each installed as mod (mod.file)}
          <div class="row-item">
            <span class="row-icon secondary"><Icon name="mods" size={18} /></span>
            <div class="row-text">
              <strong>{mod.name ?? mod.file}</strong>
              <span>
                {mod.version ? `версия ${mod.version}` : "установлен вручную"} · {mod.file}
              </span>
            </div>
            <IconButton
              icon="delete"
              label="Удалить мод"
              variant="text"
              size="sm"
              onclick={() => (removing = mod)}
            />
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <div class="toolbar">
    <label class="search">
      <Icon name="search" size={19} />
      <input type="search" placeholder="Поиск по названию, описанию или автору" bind:value={query} />
      {#if query}
        <IconButton
          icon="close"
          label="Очистить поиск"
          variant="text"
          size="sm"
          onclick={() => (query = "")}
        />
      {/if}
    </label>
    <div class="segmented" role="tablist" aria-label="Тип мода">
      <button
        type="button"
        role="tab"
        aria-selected={kind === "mod"}
        class:active={kind === "mod"}
        onclick={() => {
          kind = "mod";
          category = "";
        }}
      >
        Контентные <span>{contentMods.length}</span>
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={kind === "native-mod"}
        class:active={kind === "native-mod"}
        onclick={() => {
          kind = "native-mod";
          category = "";
          onlyInstalled = false;
        }}
      >
        Нативные <span>{nativeMods.length}</span>
      </button>
    </div>
  </div>

  <div class="filters">
    {#if categories.length}
      <div class="scroll-x cats">
        <button
          type="button"
          class="chip"
          class:on={!category}
          onclick={() => (category = "")}
        >
          Все
        </button>
        {#each categories as [name, count] (name)}
          <button
            type="button"
            class="chip"
            class:on={category === name}
            onclick={() => (category = category === name ? "" : name)}
          >
            {categoryLabel(name)} <span class="cat-count">{count}</span>
          </button>
        {/each}
      </div>
    {/if}

    <div class="filter-actions">
      {#if kind === "mod"}
        <button
          type="button"
          class="chip"
          class:on={onlyInstalled}
          onclick={() => (onlyInstalled = !onlyInstalled)}
        >
          <Icon name="check" size={14} /> Установленные
        </button>
      {/if}
      <select class="select compact sort" bind:value={sort} aria-label="Сортировка">
        <option value="popular">По популярности</option>
        <option value="name">По названию</option>
        <option value="updated">По обновлению</option>
      </select>
    </div>
  </div>

  <p class="hint-line">
    Установленные моды по умолчанию выключены — включите их в игре: <strong>Options → Mods</strong>.
    Миры GERA не трогает. Полные тексты реестра остаются на английском.
  </p>

  {#if kind === "native-mod"}
    <div class="banner info">
      <Icon name="info" size={20} />
      <div>
        <strong>Нативные моды (.hnmod)</strong>
        меняют исходники Horizon, поэтому GERA собирает под них отдельный клиент: скачивает нужный
        коммит Horizon и horizon-mixins, применяет мод и компилирует игру. Сборка идёт на вашем
        компьютере. Первая сборка занимает десятки минут и 10–20 ГБ.
      </div>
    </div>
  {/if}

  {#if error}
    <div class="banner" role="alert">
      <Icon name="warning" size={18} />
      <span>{error}</span>
      <button class="link-button" type="button" onclick={() => load(true)}>Повторить</button>
    </div>
  {/if}

  {#if loading}
    <div class="mod-grid" aria-hidden="true">
      {#each Array(6) as _, index (index)}
        <div class="skeleton"></div>
      {/each}
    </div>
  {:else if !visible.length}
    <div class="empty">
      <Icon name="mods" size={30} />
      {#if kind === "native-mod" && !nativeMods.length}
        <strong>Нативные моды временно недоступны</strong>
        <span>
          Это не проблема GERA: Horizon переводит каталог модов на новый формат, и нативные
          моды ещё не перенесены. Ждём исправления на стороне Horizon. Спасибо за понимание :)
        </span>
      {:else}
        <strong>Ничего не найдено</strong>
        <span>Измените запрос, снимите фильтр или выберите другую категорию.</span>
      {/if}
    </div>
  {:else}
    <div class="mod-grid">
      {#each visible as project (project.id)}
        {@const record = installedRecord(project)}
        <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
        <article
          class="mod-card"
          class:installed={!!record}
          role="button"
          tabindex="0"
          onclick={() => openDetail(project)}
          onkeydown={(event) => onCardKey(event, project)}
        >
          <div class="mod-top">
            {#if project.icon && !failedIcons[project.id]}
              <img
                class="mod-icon"
                src={project.icon}
                alt=""
                loading="lazy"
                onerror={() => badge(project.id)}
              />
            {:else}
              <span class="mod-icon fallback">{project.name.slice(0, 1).toUpperCase()}</span>
            {/if}
            <div class="mod-titles">
              <h3>{project.name}</h3>
              <p>{authorText(project)}</p>
            </div>
            {#if record}
              <span class="badge ok">{hasUpdate(project) ? "обновление" : "установлен"}</span>
            {/if}
          </div>

          <p class="mod-summary">{project.summaryRu || project.summary || "Без описания."}</p>

          <div class="mod-meta">
            <span class="chip">v{project.versions[0]?.version ?? "—"}</span>
            <span class="chip">MC {project.versions[0]?.game || "26.3"}</span>
            {#if project.categories.length}
              <span class="chip outlined">{categoryLabel(project.categories[0])}</span>
            {/if}
          </div>

          <div class="mod-foot">
            <span class="downloads">
              <Icon name="download" size={14} />
              {downloadsText(project.downloads)}
            </span>
            <div class="spacer"></div>
            {#if kind === "mod"}
              {#if busyId === project.id}
                <Button variant="tonal" size="sm" disabled>Устанавливаем…</Button>
              {:else if record && hasUpdate(project)}
                <Button
                  variant="tonal"
                  size="sm"
                  icon="refresh"
                  onclick={(event) => {
                    stop(event);
                    install(project);
                  }}
                >
                  Обновить
                </Button>
              {:else if record}
                <Button variant="text" size="sm" icon="check" disabled>Установлен</Button>
              {:else}
                <Button
                  variant="filled"
                  size="sm"
                  icon="add"
                  onclick={(event) => {
                    stop(event);
                    install(project);
                  }}
                >
                  Установить
                </Button>
              {/if}
            {:else if builtOf(project)}
              <span class="chip ok">собран</span>
            {:else}
              <Button
                variant="filled"
                size="sm"
                icon="play"
                disabled={checking && busyId === project.id}
                onclick={(event) => {
                  stop(event);
                  openBuild(project);
                }}
              >
                {checking && busyId === project.id ? "Проверяем…" : "Собрать"}
              </Button>
            {/if}
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>

<Dialog open={!!detail} title={detail?.name ?? "Мод"} wide onclose={() => (detail = null)}>
  {#if detail}
    {@const item = detail}
    <div class="detail">
      <div class="detail-head">
        {#if item.icon && !failedIcons[item.id]}
          <img class="mod-icon big" src={item.icon} alt="" onerror={() => badge(item.id)} />
        {:else}
          <span class="mod-icon big fallback">{item.name.slice(0, 1).toUpperCase()}</span>
        {/if}
        <div class="detail-names">
          <p class="muted small">{authorText(detail)}</p>
          <div class="mod-meta">
            <span class="chip primary">
              {item.kind === "mod" ? "контентный .hmod" : "нативный .hnmod"}
            </span>
            {#each item.categories.slice(0, 4) as name (name)}
              <span class="chip">{categoryLabel(name)}</span>
            {/each}
          </div>
        </div>
      </div>

      {#if item.descriptionRu}
        <p class="description">{item.descriptionRu}</p>
      {:else}
        <p class="description">{item.summaryRu || item.summary || "Без описания."}</p>
        {#if item.description}
          <p class="ru-note">Полное описание из реестра — на английском:</p>
          <p class="description en">{item.description}</p>
        {/if}
      {/if}

      <div class="versions">
        <div class="section-head"><h2>Версии</h2></div>
        {#each item.versions.slice(0, 6) as version (version.version)}
          {@const isCurrent = installed.some((m) => m.file === version.file.name)}
          <div class="version-row">
            <div class="version-text">
              <strong>{version.version}</strong>
              <span class="muted small">
                {version.game ? `Minecraft ${version.game}` : ""}
                {version.published ? ` · ${version.published.slice(0, 10)}` : ""}
              </span>
              <p class="mono muted small">{version.file.name} · {sizeText(version.file.size)}</p>
            </div>
            {#if item.kind === "mod"}
              {#if isCurrent}
                <span class="chip ok">установлена</span>
              {:else}
                <Button
                  variant="tonal"
                  size="sm"
                  icon="add"
                  disabled={busyId === item.id || !selected}
                  onclick={() => install(item, version)}
                >
                  Установить
                </Button>
              {/if}
            {:else}
              <Button
                variant="tonal"
                size="sm"
                icon="play"
                disabled={!selected || checking}
                onclick={() => openBuild(item, version)}
              >
                Собрать
              </Button>
            {/if}
          </div>
        {/each}
      </div>

      {#if item.kind === "mod"}
        <p class="hash">
          SHA-256 проверяется после загрузки:
          <span class="mono">{item.versions[0]?.file.sha256}</span>
        </p>
      {:else}
        <div class="banner info">
          <Icon name="info" size={19} />
          <div>
            Нативный мод меняет исходники самого Horizon: GERA соберёт под него отдельный клиент и
            привяжет его к сборке. Нативный код выполняется с вашими правами.
          </div>
        </div>
      {/if}
    </div>
  {/if}
</Dialog>

<Dialog
  open={!!removing}
  title="Удалить мод?"
  onclose={() => (removeBusy ? null : (removing = null))}
>
  <p>
    Файл <span class="mono">{removing?.file}</span> будет убран из папки
    <span class="mono">horizon-mods</span>. Резервная копия останется в данных GERA, мир не
    затрагивается.
  </p>
  {#snippet actions()}
    <Button variant="text" onclick={() => (removing = null)} disabled={removeBusy}>Отмена</Button>
    <Button variant="danger" onclick={confirmRemove} disabled={removeBusy}>
      {removeBusy ? "Удаляем…" : "Удалить"}
    </Button>
  {/snippet}
</Dialog>

<Dialog
  open={!!target && !!check}
  title="Собрать нативный клиент?"
  wide
  onclose={() => {
    if (buildBusy) return;
    target = null;
    check = null;
  }}
>
  {#if target && check}
    <div class="detail">
      <p class="muted small">
        {target.project.name} {target.version.version} — мод перепишет исходники Horizon, и GERA
        соберёт отдельный клиент.
      </p>
      <div class="build-info">
        <div><span>Horizon</span><strong class="mono">{check.horizon.slice(0, 7)}</strong></div>
        <div><span>horizon-mixins</span><strong class="mono">{check.mixins.slice(0, 7)}</strong></div>
        <div><span>Базовая сборка</span><strong>{check.baseTag}</strong></div>
        <div><span>Minecraft</span><strong>{check.game || "26.3"}</strong></div>
        <div><span>Свободно</span><strong>{native?.diskFreeGb ?? "—"} ГБ</strong></div>
      </div>

      {#if check.built}
        <div class="banner info">
          <Icon name="check" size={19} />
          <div>
            Эта сборка уже есть: <span class="mono">{check.built}</span>. Можно сразу включить её
            сборке.
          </div>
        </div>
      {:else if check.traitApi}
        <div class="banner">
          <Icon name="warning" size={18} />
          <div>
            Этот мод использует trait-API Horizon (<span class="mono">[api]</span>). GERA пока
            собирает только mixin-моды.
          </div>
        </div>
      {:else}
        <div class="banner">
          <Icon name="warning" size={20} />
          <div>
            <strong>Это нативный код.</strong>
            Мод компилируется в игру и выполняется с правами вашего пользователя. Первая сборка может
            занять 30–60 минут и до 20 ГБ. Отменить можно на любом шаге. Horizon при этом не
            меняется — GERA делает отдельную папку.
          </div>
        </div>

        {#if !check.matchCommit}
          <label class="check-row">
            <input type="checkbox" bind:checked={allowMismatch} />
            <span>
              Собрать всё равно: мод сделан для Horizon
              <span class="mono">{check.horizon.slice(0, 7)}</span>, а базовая сборка —
              <span class="mono">{check.baseRevision ?? "другой коммит"}</span>. Мод может не
              примениться или не скомпилироваться.
            </span>
          </label>
        {/if}
      {/if}
    </div>
  {/if}

  {#snippet actions()}
    {#if target && check}
      {#if check.built}
        <Button variant="text" onclick={() => (target = null)}>Закрыть</Button>
        <Button variant="filled" onclick={() => useBuild(check!.built!)}>
          Использовать в сборке
        </Button>
      {:else if check.traitApi}
        <Button variant="text" onclick={() => (target = null)}>Закрыть</Button>
      {:else}
        <Button
          variant="text"
          onclick={() => {
            target = null;
            check = null;
          }}
          disabled={buildBusy}
        >
          Отмена
        </Button>
        <Button
          variant="filled"
          icon="play"
          onclick={startBuild}
          disabled={buildBusy || (!check.matchCommit && !allowMismatch)}
        >
          {buildBusy ? "Запускаем…" : "Начать сборку"}
        </Button>
      {/if}
    {/if}
  {/snippet}
</Dialog>

<Dialog
  open={!!removingBuild}
  title="Удалить нативную сборку?"
  onclose={() => (removingBuild = null)}
>
  <p>
    Папка <span class="mono">{removingBuild?.folder}</span> будет удалена. Базовая версия Horizon,
    ресурсы и миры останутся на месте; мод можно собрать снова.
  </p>
  {#snippet actions()}
    <Button variant="text" onclick={() => (removingBuild = null)}>Отмена</Button>
    <Button variant="danger" onclick={confirmRemoveBuild}>Удалить</Button>
  {/snippet}
</Dialog>

<style>
  .job {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .job.failed {
    box-shadow: inset 0 0 0 1px var(--error);
  }
  .job-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }
  .job-head strong {
    display: block;
    font-size: 15px;
    font-weight: 700;
  }
  .job-head p {
    margin: 2px 0 0;
  }
  .bar {
    height: 10px;
    border-radius: var(--shape-full);
    background: var(--surface-container-highest);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--primary);
    transition: width var(--motion-slow) var(--ease-standard);
  }
  .log {
    margin: 0;
    padding: 12px 14px;
    max-height: 220px;
    overflow: auto;
    border-radius: var(--shape-md);
    background: var(--surface-container-lowest);
    color: var(--on-surface-variant);
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-size: 11.5px;
    line-height: 1.5;
    white-space: pre-wrap;
  }
  .log.error {
    color: var(--error);
  }
  .job-done {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  .rows {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 8px;
  }
  .row-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border-radius: var(--shape-lg);
    background: var(--surface-container-low);
  }
  .row-icon {
    width: 40px;
    height: 40px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: var(--shape-md);
    background: var(--tertiary-container);
    color: var(--on-tertiary-container);
  }
  .row-icon.secondary {
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  .row-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }
  .row-text strong {
    font-size: 13.5px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-text span {
    font-size: 11.5px;
    color: var(--on-surface-variant);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .filters {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .cats {
    flex: 1 1 320px;
  }
  .chip.on {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  button.chip {
    cursor: pointer;
    border: 1px solid transparent;
    font-family: inherit;
  }
  button.chip:hover {
    filter: brightness(1.05);
  }
  .cat-count {
    opacity: 0.7;
    font-size: 11px;
  }
  .filter-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .sort {
    min-width: 190px;
    min-height: 44px;
  }
  .hint-line {
    margin: 0;
    font-size: 12px;
    line-height: 1.55;
    color: var(--on-surface-variant);
  }

  .mod-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: var(--space-4);
  }
  .mod-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 18px;
    border-radius: var(--shape-xl);
    background: var(--surface-container-low);
    cursor: pointer;
    transition:
      background-color var(--motion) var(--ease-standard),
      transform var(--motion) var(--ease-standard),
      box-shadow var(--motion) var(--ease-standard);
  }
  .mod-card:hover {
    background: var(--surface-container);
  }
  :global([data-ui="heavy"]) .mod-card:hover {
    transform: translateY(-2px);
    box-shadow: var(--elevation-1);
  }
  .mod-card.installed {
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--primary) 40%, transparent);
  }
  .mod-top {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .mod-icon {
    width: 56px;
    height: 56px;
    flex: 0 0 auto;
    border-radius: var(--shape-md);
    object-fit: cover;
    background: var(--surface-container-high);
  }
  .mod-icon.fallback {
    display: grid;
    place-items: center;
    font-size: 22px;
    font-weight: 800;
    background: var(--tertiary-container);
    color: var(--on-tertiary-container);
  }
  .mod-icon.big {
    width: 76px;
    height: 76px;
    border-radius: var(--shape-lg);
  }
  .mod-titles {
    flex: 1;
    min-width: 0;
  }
  .mod-titles h3 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.01em;
    line-height: 1.25;
    overflow: hidden;
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .mod-titles p {
    margin: 3px 0 0;
    font-size: 12px;
    color: var(--on-surface-variant);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mod-summary {
    margin: 0;
    min-height: 38px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--on-surface-variant);
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .mod-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .mod-foot {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: auto;
  }
  .downloads {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    color: var(--on-surface-variant);
  }

  .detail {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .detail-head {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .detail-names {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .description {
    margin: 0;
    font-size: 13px;
    line-height: 1.6;
    color: var(--on-surface-variant);
  }
  .description.en {
    opacity: 0.8;
  }
  .ru-note {
    margin: 0;
    font-size: 11.5px;
    font-weight: 650;
    color: var(--on-surface-variant);
  }
  .versions {
    display: flex;
    flex-direction: column;
  }
  .version-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--outline-variant);
  }
  .version-row:last-child {
    border-bottom: none;
  }
  .version-text {
    flex: 1;
    min-width: 0;
  }
  .version-text strong {
    font-size: 13.5px;
    font-weight: 700;
    color: var(--on-surface);
  }
  .version-text p {
    margin: 2px 0 0;
    overflow-wrap: anywhere;
  }
  .hash {
    margin: 0;
    font-size: 11.5px;
    color: var(--on-surface-variant);
    overflow-wrap: anywhere;
  }
  .build-info {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 8px;
  }
  .build-info div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 12px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
  }
  .build-info span {
    font-size: 11px;
    color: var(--on-surface-variant);
  }
  .build-info strong {
    font-size: 12.5px;
    color: var(--on-surface);
    overflow-wrap: anywhere;
  }
  .check-row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
    font-size: 12px;
    line-height: 1.5;
    cursor: pointer;
  }
  .check-row input {
    margin-top: 3px;
    accent-color: var(--primary);
  }

  @media (max-width: 620px) {
    .mod-grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .sort {
      min-width: 0;
      width: 100%;
    }
  }
</style>
