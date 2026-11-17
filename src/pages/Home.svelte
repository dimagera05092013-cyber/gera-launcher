<script lang="ts">
  import { onMount } from "svelte";
  import { ui, notify, activateProfile, refreshHorizon } from "../lib/store.svelte";
  import { go } from "../lib/router.svelte";
  import { api, errorText } from "../lib/api";
  import type { InstalledMod, ModProject } from "../lib/types";
  import Icon from "../lib/components/Icon.svelte";
  import Button from "../lib/components/Button.svelte";
  import IconButton from "../lib/components/IconButton.svelte";

  interface ModUpdate {
    name: string;
    from: string;
    to: string;
  }

  let launching = $state(false);
  let switching = $state("");
  let error = $state("");
  let dropFolder = $state("");
  let modsCount = $state<number | null>(null);
  let updates = $state<ModUpdate[]>([]);
  let modsLoading = $state(true);

  onMount(async () => {
    try {
      dropFolder = await api.importFolder();
    } catch {
      dropFolder = "";
    }
    await loadModInfo();
  });

  const boot = $derived(ui.boot);
  const profile = $derived(boot?.activeProfile ?? null);
  const builds = $derived(boot?.profiles ?? []);
  const running = $derived(boot?.horizon.running ?? null);
  const versions = $derived(boot?.horizon.versions ?? []);
  const ready = $derived(versions.filter((v) => v.ready));
  const version = $derived(
    profile?.horizonVersion
      ? versions.find((v) => v.tag === profile.horizonVersion) ?? null
      : ready[0] ?? versions[0] ?? null,
  );
  const canPlay = $derived(!!profile && !!version && !running && !launching && !ui.busy);

  const greeting = $derived.by(() => {
    const hour = new Date().getHours();
    if (hour < 5) return "Доброй ночи";
    if (hour < 12) return "Доброе утро";
    if (hour < 18) return "Добрый день";
    return "Добрый вечер";
  });

  const statusText = $derived(
    running
      ? "Игра запущена"
      : !version
        ? "Клиент не найден"
        : version.ready
          ? "Клиент готов"
          : "Нужна подготовка",
  );

  const minecraft = $derived(version?.minecraft || ready[0]?.minecraft || "26.3");

  function shortTag(tag: string | undefined): string {
    if (!tag) return "—";
    return tag.length > 24 ? `${tag.slice(0, 21)}…` : tag;
  }

  const horizonDate = $derived(
    version?.tag.match(/(\d{4}-\d{2}-\d{2})/)?.[1] ?? shortTag(version?.tag),
  );

  async function loadModInfo() {
    modsLoading = true;
    const active = profile;
    if (!active) {
      modsCount = null;
      updates = [];
      modsLoading = false;
      return;
    }
    let installed: InstalledMod[] = [];
    try {
      const [, list] = await api.modsInstalled(active.id);
      installed = list;
      const builtin = await api.modsBuiltin(active.id);
      modsCount = list.length + builtin.filter((mod) => mod.enabled).length;
    } catch {
      modsCount = null;
    }
    try {
      const catalog: ModProject[] = await api.modsCatalog(false);
      const files = new Map<string, ModProject>();
      for (const project of catalog) {
        for (const item of project.versions) files.set(item.file.name, project);
      }
      const found: ModUpdate[] = [];
      for (const mod of installed) {
        const project = files.get(mod.file);
        const latest = project?.versions[0];
        if (!project || !latest || !mod.version) continue;
        if (latest.version !== mod.version) {
          found.push({ name: project.name, from: mod.version, to: latest.version });
        }
      }
      updates = found.slice(0, 4);
    } catch {
      updates = [];
    } finally {
      modsLoading = false;
    }
  }

  async function play() {
    if (!profile) return;
    launching = true;
    error = "";
    try {
      if (version && !version.ready) {
        const scan = await api.importScan();
        if (!scan.imported) {
          error = `Нет ресурсов Minecraft. Скопируйте готовую папку 26.3 в ${scan.folder} и нажмите «Играть» ещё раз.`;
          notify(error, "error");
          return;
        }
        notify(
          `Ресурсы Minecraft ${scan.imported.minecraft} подхвачены из папки переноса.`,
          "success",
        );
        await refreshHorizon();
      }
      const result = await api.launchGame(profile.id);
      if (boot) {
        boot.horizon.running = {
          pid: result.pid,
          gameDir: profile.gameDir,
          player: profile.playerName,
          startedAt: Date.now(),
          profileId: profile.id,
        };
      }
      notify("Horizon запускается…", "success");
    } catch (caught) {
      error = errorText(caught);
      notify(error, "error");
    } finally {
      launching = false;
    }
  }

  async function switchProfile(id: string) {
    if (id === profile?.id || switching) return;
    switching = id;
    try {
      await activateProfile(id);
      await loadModInfo();
    } finally {
      switching = "";
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

  async function openWorldFolder() {
    if (!profile) return;
    try {
      await api.openPath(profile.gameDir);
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }
</script>

<svelte:head><title>Главная · GERA Launcher</title></svelte:head>

<div class="home">
  {#if error}
    <div class="banner" role="alert">
      <Icon name="warning" size={18} />
      <span>{error}</span>
    </div>
  {/if}

  <section class="hero" class:live={!!running}>
    <div class="hero-body">
      <p class="eyebrow">{greeting}{#if profile} · {profile.playerName}{/if}</p>
      <h1>{profile ? profile.name : "Первая сборка"}</h1>
      <p class="hero-desc">
        {profile?.description?.trim()
          ? profile.description
          : profile
            ? "Миры, моды и настройки этой сборки хранятся отдельно."
            : "Создайте сборку — GERA заведёт для неё отдельную игровую папку."}
      </p>

      <div class="hero-chips">
        {#if version}
          <span class="chip">Horizon {horizonDate}</span>
          <span class="chip">Minecraft {minecraft}</span>
        {/if}
        <span class="chip">{modsCount === null ? "—" : modsCount} модов</span>
        <span class="chip" class:ok={version?.ready && !running} class:warn={!version || !version.ready}>
          {statusText}
        </span>
      </div>

      <div class="hero-actions">
        {#if profile && version}
          <button
            type="button"
            class="play"
            disabled={!canPlay}
            onclick={play}
            title={running ? "Игра уже запущена" : "Запустить Horizon"}
          >
            <span class="play-icon">
              <Icon name={running ? "check" : "play"} size={26} />
            </span>
            <span>{launching ? "Запускаем…" : running ? "В игре" : "Играть"}</span>
          </button>
        {:else if !profile}
          <button type="button" class="play" onclick={() => go("builds")}>
            <span class="play-icon"><Icon name="add" size={26} /></span>
            <span>Создать сборку</span>
          </button>
        {:else}
          <button type="button" class="play" onclick={() => go("settings")}>
            <span class="play-icon"><Icon name="settings" size={24} /></span>
            <span>Настроить клиент</span>
          </button>
        {/if}

        {#if profile}
          <IconButton
            icon="folder"
            label="Открыть игровую папку"
            variant="tonal"
            size="lg"
            onclick={openWorldFolder}
          />
          <IconButton
            icon="builds"
            label="Выбрать другую сборку"
            variant="standard"
            size="lg"
            onclick={() => go("builds")}
          />
        {/if}
      </div>
    </div>

    <div class="hero-art" aria-hidden="true">
      <div class="tile tile-a"><Icon name="builds" size={88} /></div>
      <div class="tile tile-b"><Icon name="mods" size={42} /></div>
      <div class="tile tile-c"><Icon name="play" size={28} /></div>
    </div>
  </section>

  {#if version && !version.ready && !running}
    <section class="drop-hint">
      <span class="hint-icon"><Icon name="download" size={22} /></span>
      <div class="hint-copy">
        <strong>Нет ресурсов Minecraft</strong>
        <span>
          Перетащите папку <span class="mono">26.3</span> или её zip прямо в окно. Можно положить
          архив в папку переноса и нажать «Играть» — GERA подхватит его сама.
        </span>
        {#if dropFolder}
          <span class="mono hint-path">{dropFolder}</span>
        {/if}
      </div>
      <Button size="sm" variant="tonal" icon="folder" onclick={openDropFolder}>
        Открыть папку
      </Button>
    </section>
  {/if}

  <section class="quick">
    <button type="button" class="quick-tile" onclick={() => go("mods")}>
      <span class="quick-icon primary"><Icon name="mods" size={22} /></span>
      <span class="quick-text">
        <strong>{modsCount === null ? "—" : modsCount}</strong>
        <span>модов установлено</span>
      </span>
      <Icon name="arrow" size={18} />
    </button>

    <div class="quick-tile static">
      <span class="quick-icon tertiary"><Icon name="chip" size={22} /></span>
      <span class="quick-text">
        <strong>{version ? `Horizon ${horizonDate}` : "Клиент не найден"}</strong>
        <span>{version ? `Minecraft ${minecraft}` : "Установите в настройках"}</span>
      </span>
    </div>

    <button type="button" class="quick-tile" onclick={() => go("mods")}>
      <span class="quick-icon secondary"><Icon name="update" size={22} /></span>
      <span class="quick-text">
        <strong>{modsLoading ? "…" : updates.length ? `${updates.length} обновл.` : "Всё свежее"}</strong>
        <span>{updates.length ? "можно обновить моды" : "обновлений модов нет"}</span>
      </span>
      <Icon name="arrow" size={18} />
    </button>
  </section>

  {#if profile}
    <section class="card updates">
      <div class="section-head">
        <h2>Обновления модов</h2>
        <button type="button" class="link-button" onclick={() => go("mods")}>Каталог</button>
      </div>
      {#if modsLoading}
        <div class="update-loading">
          <span class="skeleton line"></span>
          <span class="skeleton line short"></span>
        </div>
      {:else if updates.length}
        <div class="update-list">
          {#each updates as item (item.name)}
            <div class="update-row">
              <span class="update-icon"><Icon name="mods" size={18} /></span>
              <span class="update-name">{item.name}</span>
              <span class="update-versions mono">{item.from} → {item.to}</span>
              <span class="chip primary">обновление</span>
            </div>
          {/each}
        </div>
      {:else}
        <p class="update-empty">
          <Icon name="check" size={18} />
          Установленные моды совпадают с последними версиями каталога.
        </p>
      {/if}
    </section>
  {/if}

  {#if builds.length > 1}
    <section class="builds-strip">
      <div class="section-head">
        <h2>Сборки</h2>
        <button type="button" class="link-button" onclick={() => go("builds")}>Все сборки</button>
      </div>
      <div class="scroll-x">
        {#each builds as item (item.id)}
          <button
            type="button"
            class="build-pill"
            class:active={item.id === profile?.id}
            disabled={!!switching}
            onclick={() => switchProfile(item.id)}
          >
            <span class="pill-avatar">{item.playerName.slice(0, 1).toUpperCase()}</span>
            <span class="pill-text">
              <strong>{item.name}</strong>
              <span>{switching === item.id ? "Переключаем…" : item.playerName}</span>
            </span>
            {#if item.id === profile?.id}
              <Icon name="check" size={16} />
            {/if}
          </button>
        {/each}
        <button type="button" class="build-pill add" onclick={() => go("builds")}>
          <span class="pill-avatar add-icon"><Icon name="add" size={18} /></span>
          <span class="pill-text">
            <strong>Новая сборка</strong>
            <span>Своя папка</span>
          </span>
        </button>
      </div>
    </section>
  {/if}
</div>

<style>
  .home {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .hero {
    position: relative;
    display: flex;
    align-items: center;
    min-height: 330px;
    padding: 38px 40px;
    border-radius: var(--shape-3xl);
    overflow: hidden;
    background: var(--hero-gradient);
    isolation: isolate;
  }
  .hero.live {
    background: var(--hero-live-gradient);
  }
  :global([data-ui="heavy"]) .hero {
    box-shadow: var(--elevation-1);
  }

  .hero-body {
    position: relative;
    z-index: 2;
    width: 58%;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
  }
  h1 {
    margin: 0;
    font-size: var(--type-display);
    line-height: 1.02;
    font-weight: 850;
    letter-spacing: var(--tracking-display);
    max-width: 14ch;
  }
  .hero-desc {
    margin: 2px 0 0;
    max-width: 46ch;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--on-surface-variant);
  }
  .hero-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }
  .hero-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 18px;
    flex-wrap: wrap;
  }

  .play {
    display: inline-flex;
    align-items: center;
    gap: 12px;
    height: 68px;
    padding: 0 34px 0 12px;
    border: none;
    border-radius: var(--shape-full);
    background: var(--primary);
    color: var(--on-primary);
    font: inherit;
    font-size: 17px;
    font-weight: 750;
    letter-spacing: -0.01em;
    cursor: pointer;
    box-shadow: var(--elevation-play);
    transition:
      transform var(--motion) var(--ease-spring),
      box-shadow var(--motion) var(--ease-standard),
      filter var(--motion) var(--ease-standard);
  }
  .play:hover:not(:disabled) {
    filter: brightness(1.07);
    transform: translateY(-2px);
  }
  .play:active:not(:disabled) {
    transform: scale(0.975);
  }
  .play:disabled {
    opacity: 0.55;
    cursor: default;
    box-shadow: none;
  }
  .play-icon {
    width: 46px;
    height: 46px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--on-primary) 14%, transparent);
  }

  .hero-art {
    position: absolute;
    right: 3%;
    top: 50%;
    width: 360px;
    height: 300px;
    transform: translateY(-50%);
    z-index: 1;
    pointer-events: none;
  }
  .tile {
    position: absolute;
    display: grid;
    place-items: center;
    border-radius: var(--shape-2xl);
  }
  .tile-a {
    width: 196px;
    height: 196px;
    right: 42px;
    top: 44px;
    transform: rotate(-7deg);
    background: var(--primary-container);
    color: var(--on-primary-container);
    background-image: var(--hero-pattern);
    background-size: 14px 14px;
  }
  .tile-b {
    width: 104px;
    height: 104px;
    right: 0;
    bottom: 24px;
    transform: rotate(9deg);
    background: var(--tertiary-container);
    color: var(--on-tertiary-container);
  }
  .tile-c {
    width: 66px;
    height: 66px;
    left: 12px;
    bottom: 30px;
    transform: rotate(-12deg);
    border-radius: 50%;
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  :global([data-ui="heavy"]) .tile-a {
    animation: float-a 9s ease-in-out infinite;
  }
  :global([data-ui="heavy"]) .tile-b {
    animation: float-b 7s ease-in-out infinite;
  }
  :global([data-ui="heavy"]) .tile-c {
    animation: float-b 8s ease-in-out infinite reverse;
  }
  @keyframes float-a {
    50% {
      transform: rotate(-7deg) translateY(-10px);
    }
  }
  @keyframes float-b {
    50% {
      transform: rotate(9deg) translateY(-8px);
    }
  }

  .drop-hint {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 16px 18px;
    border-radius: var(--shape-xl);
    background: var(--surface-container);
  }
  .hint-icon {
    width: 44px;
    height: 44px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--tertiary-container);
    color: var(--on-tertiary-container);
  }
  .hint-copy {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }
  .hint-copy strong {
    font-size: 14px;
    font-weight: 700;
  }
  .hint-copy > span {
    font-size: 12.5px;
    color: var(--on-surface-variant);
    line-height: 1.5;
  }
  .hint-path {
    overflow-wrap: anywhere;
    opacity: 0.8;
  }

  .quick {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .quick-tile {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px;
    border: none;
    border-radius: var(--shape-xl);
    background: var(--surface-container-low);
    color: var(--on-surface);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--motion-fast) var(--ease-standard);
  }
  .quick-tile:hover {
    background: var(--surface-container);
  }
  .quick-tile.static {
    cursor: default;
  }
  .quick-tile.static:hover {
    background: var(--surface-container-low);
  }
  .quick-tile > :global(.icon) {
    color: var(--on-surface-variant);
    margin-left: auto;
  }
  .quick-icon {
    width: 46px;
    height: 46px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: var(--shape-md);
  }
  .quick-icon.primary {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .quick-icon.secondary {
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  .quick-icon.tertiary {
    background: var(--tertiary-container);
    color: var(--on-tertiary-container);
  }
  .quick-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .quick-text strong {
    font-size: 15px;
    font-weight: 750;
    letter-spacing: -0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .quick-text span {
    font-size: 11.5px;
    color: var(--on-surface-variant);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .updates {
    padding: 20px 22px;
  }
  .update-loading {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .skeleton.line {
    min-height: 44px;
    border-radius: var(--shape-md);
  }
  .skeleton.line.short {
    min-height: 44px;
    width: 70%;
  }
  .update-list {
    display: flex;
    flex-direction: column;
  }
  .update-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--outline-variant);
  }
  .update-row:last-child {
    border-bottom: none;
  }
  .update-icon {
    width: 36px;
    height: 36px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: var(--shape-sm);
    background: var(--secondary-container);
    color: var(--on-secondary-container);
  }
  .update-name {
    flex: 1;
    min-width: 0;
    font-size: 13.5px;
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .update-versions {
    color: var(--on-surface-variant);
    white-space: nowrap;
  }
  .update-empty {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--on-surface-variant);
    font-size: 13px;
  }
  .update-empty :global(.icon) {
    color: var(--primary);
  }

  .builds-strip {
    display: flex;
    flex-direction: column;
  }
  .build-pill {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 0 0 auto;
    min-width: 210px;
    padding: 10px 14px 10px 10px;
    border: none;
    border-radius: var(--shape-lg);
    background: var(--surface-container-low);
    color: var(--on-surface);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--motion-fast) var(--ease-standard);
  }
  .build-pill:hover:not(:disabled):not(.active) {
    background: var(--surface-container);
  }
  .build-pill.active {
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .build-pill.active:hover:not(:disabled) {
    background: color-mix(in srgb, var(--on-primary-container) 9%, var(--primary-container));
  }
  .pill-avatar {
    width: 40px;
    height: 40px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: var(--shape-md);
    font-size: 16px;
    font-weight: 800;
    background: var(--tertiary-container);
    color: var(--on-tertiary-container);
  }
  .active .pill-avatar {
    background: color-mix(in srgb, var(--on-primary-container) 12%, transparent);
    color: var(--on-primary-container);
  }
  .add-icon {
    background: var(--surface-container-high);
    color: var(--on-surface-variant);
  }
  .pill-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    flex: 1;
  }
  .pill-text strong {
    font-size: 13px;
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pill-text span {
    font-size: 11.5px;
    color: var(--on-surface-variant);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .active .pill-text span {
    color: color-mix(in srgb, var(--on-primary-container) 72%, transparent);
  }

  @media (max-width: 1040px) {
    .hero-body {
      width: 66%;
    }
    .hero-art {
      right: -4%;
      opacity: 0.7;
    }
  }

  @media (max-width: 860px) {
    .quick {
      grid-template-columns: minmax(0, 1fr);
    }
    .hero {
      padding: 30px 26px;
    }
    .hero-body {
      width: 100%;
    }
    .hero-art {
      display: none;
    }
    .hero-desc {
      max-width: 54ch;
    }
  }

  @media (max-width: 620px) {
    .hero {
      min-height: 0;
      padding: 26px 22px;
    }
    h1 {
      font-size: 34px;
    }
    .play {
      height: 60px;
      font-size: 15.5px;
    }
    .play-icon {
      width: 40px;
      height: 40px;
    }
    .update-versions {
      display: none;
    }
  }
</style>
