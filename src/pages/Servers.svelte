<script lang="ts">
  import { onMount, tick } from "svelte";
  import { ui, notify, reload } from "../lib/store.svelte";
  import { go } from "../lib/router.svelte";
  import { api, errorText } from "../lib/api";
  import type {
    ImportedServer,
    Profile,
    ServerEntry,
    ServerImages,
    ServerImportScan,
    ServerStatus,
  } from "../lib/types";
  import Button from "../lib/components/Button.svelte";
  import IconButton from "../lib/components/IconButton.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Dialog from "../lib/components/Dialog.svelte";

  type SortMode = "manual" | "online" | "name" | "state";

  interface DraftState {
    id: string | null;
    name: string;
    host: string;
    port: number;
    description: string;
    profileId: string | null;
    fallbackText: string;
    fallbackColor: string;
    /// Newly picked data URL, sent to the backend on save.
    banner: string | null;
    icon: string | null;
    bannerPreview: string | null;
    iconPreview: string | null;
    clearBanner: boolean;
    clearIcon: boolean;
  }

  const servers = $derived(ui.boot?.servers ?? []);
  const profiles = $derived(ui.boot?.profiles ?? []);

  let images = $state<Record<string, ServerImages>>({});
  let statuses = $state<Record<string, ServerStatus | "loading">>({});
  let sort = $state<SortMode>("manual");
  let selectedId = $state<string | null>(null);
  let launchingId = $state<string | null>(null);
  let refreshing = $state(false);

  let form = $state<DraftState | null>(null);
  let formError = $state("");
  let saving = $state(false);
  let faviconBusy = $state(false);

  let importOpen = $state(false);
  let importBusy = $state(false);
  let importScan = $state<ServerImportScan | null>(null);
  let importError = $state("");
  let importSelected = $state<Record<string, boolean>>({});
  let importProfiles = $state<Record<string, string | null>>({});

  let removing = $state<ServerEntry | null>(null);
  let menu = $state<{ server: ServerEntry; x: number; y: number } | null>(null);
  let dragId = $state<string | null>(null);
  let dragOverId = $state<string | null>(null);

  const lastPing = new Map<string, number>();

  // ---------------------------------------------------------------- sorting

  function onlinePlayers(server: ServerEntry): number {
    const status = statusOf(server);
    return status?.online ? (status.onlinePlayers ?? 0) : -1;
  }

  function stateRank(server: ServerEntry): number {
    const value = statuses[server.id];
    if (!value) return 2;
    if (value === "loading") return 1;
    return value.online ? 0 : 3;
  }

  function latency(server: ServerEntry): number {
    return statusOf(server)?.latencyMs ?? 999_999;
  }

  const ordered = $derived.by(() => {
    const pinned = servers.filter((server) => server.pinned);
    const rest = servers.filter((server) => !server.pinned);
    const byName = (a: ServerEntry, b: ServerEntry) => a.name.localeCompare(b.name, "ru");
    let compare: (a: ServerEntry, b: ServerEntry) => number;
    switch (sort) {
      case "name":
        compare = byName;
        break;
      case "online":
        compare = (a, b) =>
          onlinePlayers(b) - onlinePlayers(a) || byName(a, b);
        break;
      case "state":
        compare = (a, b) => stateRank(a) - stateRank(b) || latency(a) - latency(b) || byName(a, b);
        break;
      default:
        compare = (a, b) => a.position - b.position;
    }
    pinned.sort(compare);
    rest.sort(compare);
    return [...pinned, ...rest];
  });

  const selected = $derived(
    ordered.find((server) => server.id === selectedId) ?? ordered[0] ?? null,
  );

  // ---------------------------------------------------------------- helpers

  function statusOf(server: ServerEntry): ServerStatus | null {
    const value = statuses[server.id];
    return value && value !== "loading" ? value : null;
  }

  function profileOf(server: ServerEntry): Profile | null {
    return profiles.find((profile) => profile.id === server.profileId) ?? null;
  }

  function profileLabel(server: ServerEntry): string {
    if (!server.profileId) return "Без сборки";
    return profileOf(server)?.name ?? "Сборка удалена";
  }

  function address(server: ServerEntry): string {
    return server.port === 25565 ? server.host : `${server.host}:${server.port}`;
  }

  function initials(server: ServerEntry): string {
    const text = server.fallbackText?.trim();
    if (text) return text;
    return nameInitials(server.name);
  }

  function nameInitials(name: string): string {
    return Array.from(name.trim()).slice(0, 2).join("").toUpperCase() || "?";
  }

  function stateLabel(server: ServerEntry): string {
    const value = statuses[server.id];
    if (!value) return "не проверен";
    if (value === "loading") return "проверка…";
    return value.online ? "онлайн" : "офлайн";
  }

  function stateTone(server: ServerEntry): "ok" | "warn" | "wait" | "" {
    const value = statuses[server.id];
    if (!value) return "";
    if (value === "loading") return "wait";
    return value.online ? "ok" : "warn";
  }

  function playersText(server: ServerEntry): string {
    const status = statusOf(server);
    if (!status?.online) return "";
    if (status.onlinePlayers === null) return "";
    return `${status.onlinePlayers}${status.maxPlayers !== null ? ` / ${status.maxPlayers}` : ""}`;
  }

  function note(server: ServerEntry): string {
    return server.description?.trim() || statusOf(server)?.motd?.trim() || "Описание не задано";
  }

  function replaceServer(updated: ServerEntry) {
    if (!ui.boot) return;
    const index = ui.boot.servers.findIndex((server) => server.id === updated.id);
    if (index >= 0) ui.boot.servers[index] = updated;
    else ui.boot.servers.push(updated);
  }

  // ---------------------------------------------------------------- images

  async function loadImage(id: string) {
    try {
      images[id] = await api.serverImages(id);
    } catch {
      // A missing picture is not an error: the card falls back to letters.
    }
  }

  // -------------------------------------------------------------- statuses

  async function refreshStatuses(force = false) {
    if (refreshing) return;
    refreshing = true;
    const now = Date.now();
    const targets = servers.filter(
      (server) => force || now - (lastPing.get(server.id) ?? 0) > 30_000,
    );
    targets.forEach((server) => (statuses[server.id] = "loading"));
    const queue = [...targets];
    const worker = async () => {
      for (;;) {
        const server = queue.shift();
        if (!server) return;
        lastPing.set(server.id, Date.now());
        try {
          statuses[server.id] = await api.serverStatus(server.host, server.port);
        } catch (error) {
          statuses[server.id] = {
            online: false,
            onlinePlayers: null,
            maxPlayers: null,
            motd: null,
            version: null,
            latencyMs: null,
            favicon: null,
            error: errorText(error),
          };
        }
      }
    };
    await Promise.all([worker(), worker(), worker()]);
    refreshing = false;
  }

  onMount(() => {
    void Promise.all(servers.map((server) => loadImage(server.id)));
    void refreshStatuses(true);
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refreshStatuses(false);
    }, 45_000);
    return () => window.clearInterval(timer);
  });

  // New records (import, save in another tab) get pictures and a first ping.
  $effect(() => {
    const ids = servers.map((server) => server.id);
    const missingImages = ids.filter((id) => images[id] === undefined);
    if (missingImages.length > 0) {
      void Promise.all(missingImages.map((id) => loadImage(id)));
    }
    if (servers.some((server) => statuses[server.id] === undefined)) {
      void refreshStatuses(false);
    }
  });

  // --------------------------------------------------------------- launch

  async function connect(server: ServerEntry) {
    if (launchingId) return;
    selectedId = server.id;
    const profile = profileOf(server);
    if (!server.profileId || !profile) {
      openEdit(server);
      notify(
        server.profileId
          ? "Привязанная сборка удалена. Выберите другую."
          : "Сначала привяжите к серверу сборку.",
        "error",
      );
      return;
    }
    launchingId = server.id;
    try {
      const result = await api.launchServer(server.id);
      if (ui.boot) {
        ui.boot.horizon.running = {
          pid: result.pid,
          gameDir: profile.gameDir,
          player: profile.playerName,
          startedAt: Date.now(),
          profileId: profile.id,
        };
      }
      notify(`Horizon запускается: ${profile.name} → ${server.name}`, "success");
      await reload();
    } catch (error) {
      notify(errorText(error), "error");
    } finally {
      launchingId = null;
    }
  }

  async function setProfile(server: ServerEntry, profileId: string) {
    try {
      const updated = await api.serverSetProfile(server.id, profileId || null);
      replaceServer(updated);
      selectedId = updated.id;
      notify(
        profileId ? `Сборка изменена: ${profileOf(updated)?.name ?? ""}` : "Сборка отвязана.",
        "success",
      );
    } catch (error) {
      notify(errorText(error), "error");
    }
  }

  async function togglePin(server: ServerEntry) {
    try {
      const updated = await api.serverPin(server.id, !server.pinned);
      replaceServer(updated);
      notify(updated.pinned ? "Сервер закреплён." : "Сервер откреплён.", "success");
    } catch (error) {
      notify(errorText(error), "error");
    }
  }

  // ------------------------------------------------------------ drag order

  function startDrag(event: DragEvent, server: ServerEntry) {
    if (sort !== "manual") {
      event.preventDefault();
      return;
    }
    dragId = server.id;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", server.id);
    }
  }

  function allowDrop(event: DragEvent, server: ServerEntry) {
    const source = dragId ? servers.find((entry) => entry.id === dragId) : null;
    if (!source || source.pinned !== server.pinned) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    dragOverId = server.id;
  }

  async function dropOn(event: DragEvent, target: ServerEntry) {
    event.preventDefault();
    const sourceId = dragId;
    dragId = null;
    dragOverId = null;
    if (!sourceId || sourceId === target.id) return;
    const source = servers.find((server) => server.id === sourceId);
    if (!source || source.pinned !== target.pinned) return;
    const group = ordered
      .filter((server) => server.pinned === source.pinned)
      .map((server) => server.id);
    const fromIndex = group.indexOf(sourceId);
    if (fromIndex < 0) return;
    group.splice(fromIndex, 1);
    const toIndex = group.indexOf(target.id);
    group.splice(toIndex < 0 ? group.length : toIndex, 0, sourceId);
    const pinnedIds = source.pinned
      ? group
      : ordered.filter((server) => server.pinned).map((server) => server.id);
    const restIds = source.pinned
      ? ordered.filter((server) => !server.pinned).map((server) => server.id)
      : group;
    try {
      const list = await api.serverReorder([...pinnedIds, ...restIds]);
      if (ui.boot) ui.boot.servers = list;
      sort = "manual";
    } catch (error) {
      notify(errorText(error), "error");
    }
  }

  // ----------------------------------------------------------------- menu

  let menuEl = $state<HTMLDivElement>();

  function openMenu(event: MouseEvent, server: ServerEntry) {
    event.preventDefault();
    event.stopPropagation();
    selectedId = server.id;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const width = 220;
    const height = 250;
    menu = {
      server,
      x: Math.max(8, Math.min(rect.right - width, window.innerWidth - width - 8)),
      y: Math.max(8, Math.min(rect.bottom + 6, window.innerHeight - height - 8)),
    };
    // The actions button is a separate control, so the menu has to take focus
    // by itself; otherwise Tab keeps walking through the cards behind it.
    void tick().then(() => menuEl?.querySelector<HTMLButtonElement>("button")?.focus());
  }

  function closeMenu() {
    menu = null;
  }

  // ----------------------------------------------------------------- form

  function defaultProfileId(): string | null {
    return ui.boot?.activeProfile?.id ?? profiles[0]?.id ?? null;
  }

  function openAdd() {
    formError = "";
    form = {
      id: null,
      name: "",
      host: "",
      port: 25565,
      description: "",
      profileId: defaultProfileId(),
      fallbackText: "",
      fallbackColor: "#6750A4",
      banner: null,
      icon: null,
      bannerPreview: null,
      iconPreview: null,
      clearBanner: false,
      clearIcon: false,
    };
  }

  function openEdit(server: ServerEntry) {
    const image = images[server.id];
    const profileId =
      server.profileId && profiles.some((profile) => profile.id === server.profileId)
        ? server.profileId
        : null;
    formError =
      server.profileId && !profileId ? "Привязанная сборка удалена. Выберите другую." : "";
    form = {
      id: server.id,
      name: server.name,
      host: server.host,
      port: server.port,
      description: server.description,
      profileId,
      fallbackText: server.fallbackText,
      fallbackColor: server.fallbackColor,
      banner: null,
      icon: null,
      bannerPreview: image?.banner ?? null,
      iconPreview: image?.icon ?? null,
      clearBanner: false,
      clearIcon: false,
    };
  }

  function closeForm() {
    if (saving) return;
    form = null;
  }

  function readFile(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }

  async function pickImage(event: Event, kind: "banner" | "icon") {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file || !form) return;
    if (file.size > 4 * 1024 * 1024) {
      formError = "Изображение больше 4 МБ.";
      return;
    }
    try {
      const dataUrl = await readFile(file);
      if (kind === "banner") {
        form.banner = dataUrl;
        form.bannerPreview = dataUrl;
        form.clearBanner = false;
      } else {
        form.icon = dataUrl;
        form.iconPreview = dataUrl;
        form.clearIcon = false;
      }
      formError = "";
    } catch {
      formError = "Не удалось прочитать изображение.";
    }
  }

  function removeImage(kind: "banner" | "icon") {
    if (!form) return;
    if (kind === "banner") {
      form.banner = null;
      form.bannerPreview = null;
      form.clearBanner = form.id !== null;
    } else {
      form.icon = null;
      form.iconPreview = null;
      form.clearIcon = form.id !== null;
    }
  }

  async function fetchFavicon() {
    if (!form || faviconBusy) return;
    const host = form.host.trim();
    if (!host) {
      formError = "Сначала укажите адрес сервера.";
      return;
    }
    faviconBusy = true;
    try {
      const status = await api.serverStatus(host, Number(form.port) || 25565);
      if (status.favicon) {
        form.icon = status.favicon;
        form.iconPreview = status.favicon;
        form.clearIcon = false;
        notify("Иконка получена через Server List Ping.", "success");
      } else {
        notify(
          status.online ? "Сервер не отдаёт иконку — используйте заглушку." : (status.error ?? "Сервер не отвечает."),
          "info",
        );
      }
    } catch (error) {
      notify(errorText(error), "error");
    } finally {
      faviconBusy = false;
    }
  }

  async function saveForm() {
    if (!form || saving) return;
    saving = true;
    formError = "";
    try {
      const saved = await api.serverSave({
        id: form.id,
        name: form.name,
        host: form.host,
        port: Number(form.port) || 25565,
        description: form.description,
        profileId: form.profileId,
        fallbackText: form.fallbackText,
        fallbackColor: form.fallbackColor,
        banner: form.banner,
        icon: form.icon,
        clearBanner: form.clearBanner,
        clearIcon: form.clearIcon,
      });
      await reload();
      await loadImage(saved.id);
      selectedId = saved.id;
      form = null;
      notify("Сервер сохранён.", "success");
      void refreshStatuses(false);
    } catch (error) {
      formError = errorText(error);
    } finally {
      saving = false;
    }
  }

  // --------------------------------------------------------------- delete

  async function confirmRemove() {
    if (!removing) return;
    saving = true;
    try {
      await api.serverRemove(removing.id);
      delete images[removing.id];
      delete statuses[removing.id];
      removing = null;
      await reload();
      notify("Сервер убран из GERA. Сборки, миры и моды не тронуты.", "info");
    } catch (error) {
      notify(errorText(error), "error");
    } finally {
      saving = false;
    }
  }

  // ---------------------------------------------------------------- import

  function importKey(item: ImportedServer): string {
    return `${item.host}:${item.port}`;
  }

  const importCount = $derived(
    importScan
      ? importScan.servers.filter(
          (item) => !item.alreadyAdded && importSelected[importKey(item)],
        ).length
      : 0,
  );

  async function openImport() {
    form = null;
    importOpen = true;
    await scanImport();
  }

  function closeImport() {
    if (importBusy) return;
    importOpen = false;
    importScan = null;
    importError = "";
  }

  async function scanImport() {
    importBusy = true;
    importError = "";
    try {
      const scan = await api.serverImportScan();
      importScan = scan;
      importError = scan.error ?? "";
      const selected: Record<string, boolean> = {};
      const profileMap: Record<string, string | null> = {};
      for (const item of scan.servers) {
        const key = importKey(item);
        selected[key] = !item.alreadyAdded;
        profileMap[key] = defaultProfileId();
      }
      importSelected = selected;
      importProfiles = profileMap;
    } catch (error) {
      importScan = null;
      importError = errorText(error);
    } finally {
      importBusy = false;
    }
  }

  function selectAll() {
    if (!importScan) return;
    const next: Record<string, boolean> = { ...importSelected };
    for (const item of importScan.servers) {
      if (!item.alreadyAdded) next[importKey(item)] = true;
    }
    importSelected = next;
  }

  function selectNone() {
    if (!importScan) return;
    const next: Record<string, boolean> = { ...importSelected };
    for (const item of importScan.servers) next[importKey(item)] = false;
    importSelected = next;
  }

  async function confirmImport() {
    if (!importScan || importBusy) return;
    const items = importScan.servers
      .filter((item) => !item.alreadyAdded && importSelected[importKey(item)])
      .map((item) => ({
        name: item.name,
        host: item.host,
        port: item.port,
        icon: item.icon,
        profileId: importProfiles[importKey(item)] || null,
      }));
    if (items.length === 0) return;
    importBusy = true;
    importError = "";
    try {
      const added = await api.serverImportAdd(items);
      await reload();
      await Promise.all(added.map((server) => loadImage(server.id)));
      importOpen = false;
      importScan = null;
      notify(
        added.length > 0
          ? `Импортировано серверов: ${added.length}.`
          : "Новых серверов нет: возможно, они уже добавлены.",
        added.length > 0 ? "success" : "info",
      );
      void refreshStatuses(false);
    } catch (error) {
      importError = errorText(error);
    } finally {
      importBusy = false;
    }
  }
</script>

<svelte:head><title>Серверы · GERA Launcher</title></svelte:head>

<div class="page-stack">
  <div class="page-head">
    <div>
      <h2>Ваши серверы</h2>
      <p>
        Каждый сервер запускает свою сборку и подключается к адресу через штатный
        <span class="mono">--server</span> Horizon. Список мультиплеера Horizon при импорте только
        читается.
      </p>
    </div>
    <div class="row head-actions">
      <Button variant="tonal" icon="download" onclick={openImport}>
        Импортировать из Minecraft
      </Button>
      <Button icon="add" onclick={openAdd}>Добавить сервер</Button>
    </div>
  </div>

  {#if ordered.length === 0}
    <div class="empty servers-empty">
      <Icon name="servers" size={32} />
      <strong>Пока нет серверов</strong>
      <span>
        Добавьте сервер вручную или перенесите готовый список из мультиплеера Horizon. Импорт не
        меняет файлы Horizon.
      </span>
      <div class="row">
        <Button variant="tonal" icon="download" onclick={openImport}>
          Импортировать из Minecraft
        </Button>
        <Button icon="add" onclick={openAdd}>Добавить сервер</Button>
      </div>
    </div>
  {:else}
    <div class="sortbar">
      <span class="eyebrow">Сортировка</span>
      <div class="segmented">
        <button type="button" class:active={sort === "manual"} onclick={() => (sort = "manual")}>
          <Icon name="drag" size={15} /> Вручную
        </button>
        <button type="button" class:active={sort === "online"} onclick={() => (sort = "online")}>
          <Icon name="signal" size={15} /> По онлайну
        </button>
        <button type="button" class:active={sort === "name"} onclick={() => (sort = "name")}>
          По названию
        </button>
        <button type="button" class:active={sort === "state"} onclick={() => (sort = "state")}>
          По состоянию
        </button>
      </div>
      <span class="spacer"></span>
      <Button
        size="sm"
        variant="text"
        icon="refresh"
        disabled={refreshing || servers.length === 0}
        onclick={() => refreshStatuses(true)}
      >
        {refreshing ? "Проверяем…" : "Проверить"}
      </Button>
    </div>

    <div class="servers">
      {#each ordered as server (server.id)}
        <div
          class="server"
          class:pinned={server.pinned}
          class:launching={launchingId === server.id}
          class:drag-over={dragOverId === server.id}
          tabindex="0"
          role="button"
          aria-label={`Подключиться к ${server.name}`}
          onpointerenter={() => (selectedId = server.id)}
          onfocus={() => (selectedId = server.id)}
          onclick={() => connect(server)}
          onkeydown={(event) => {
            // Only the card itself launches. Enter or Space pressed on the
            // inner buttons (drag handle, actions menu) must not bubble up
            // into a connection.
            if (event.target !== event.currentTarget) return;
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              connect(server);
            }
          }}
          oncontextmenu={(event) => openMenu(event, server)}
          ondragover={(event) => allowDrop(event, server)}
          ondrop={(event) => dropOn(event, server)}
          ondragend={() => {
            dragId = null;
            dragOverId = null;
          }}
        >
          <div class="banner-area" style={`--tint: ${server.fallbackColor}`}>
            {#if images[server.id]?.banner}
              <img src={images[server.id]?.banner ?? ""} alt="" draggable="false" />
            {:else}
              <div class="banner-fallback">
                {#if images[server.id]?.icon}
                  <img
                    class="banner-icon"
                    src={images[server.id]?.icon ?? ""}
                    alt=""
                    draggable="false"
                  />
                {:else}
                  <span class="banner-letters">{initials(server)}</span>
                {/if}
              </div>
            {/if}
            {#if server.pinned}
              <span class="pin-badge"><Icon name="pin" size={12} /> закреплён</span>
            {/if}
          </div>

          <div class="server-body">
            {#if sort === "manual"}
              <button
                type="button"
                class="drag-handle"
                draggable="true"
                title="Перетащите, чтобы изменить порядок"
                aria-label="Перетащить сервер"
                onclick={(event) => event.stopPropagation()}
                ondragstart={(event) => startDrag(event, server)}
              >
                <Icon name="drag" size={16} />
              </button>
            {/if}

            <div class="server-icon" style={`--tint: ${server.fallbackColor}`}>
              {#if images[server.id]?.icon}
                <img src={images[server.id]?.icon ?? ""} alt="" draggable="false" />
              {:else}
                <span>{initials(server)}</span>
              {/if}
            </div>

            <div class="server-main">
              <h3 title={server.name}>{server.name}</h3>
              <span class="server-profile">
                <Icon name="builds" size={13} />
                {profileLabel(server)}
              </span>
              <span class="server-note" title={note(server)}>{note(server)}</span>
            </div>

            <div class="server-endpoint">
              <span class="address mono" title={address(server)}>{address(server)}</span>
              <span class="state {stateTone(server)}">
                <span class="dot"></span>
                {launchingId === server.id ? "запуск…" : stateLabel(server)}
              </span>
              {#if playersText(server)}
                <span class="players">
                  {playersText(server)} игроков
                  {#if statusOf(server)?.latencyMs != null}
                    · {statusOf(server)?.latencyMs} мс
                  {/if}
                </span>
              {/if}
            </div>

            <div class="server-actions">
              <IconButton
                icon="more"
                label="Действия с сервером"
                variant="text"
                size="sm"
                onclick={(event) => openMenu(event, server)}
              />
            </div>
          </div>
        </div>
      {/each}
    </div>

    {#if selected}
      <div class="launch-info">
        <Icon name="builds" size={18} />
        <div class="grow">
          <strong>{selected.name}</strong>
          <span>запускается через сборку, можно сменить перед запуском</span>
        </div>
        <select
          class="select slim"
          aria-label="Сборка для выбранного сервера"
          value={selected.profileId ?? ""}
          onchange={(event) => setProfile(selected, (event.currentTarget as HTMLSelectElement).value)}
        >
          <option value="">Без сборки</option>
          {#if selected.profileId && !profileOf(selected)}
            <option value={selected.profileId}>Сборка удалена</option>
          {/if}
          {#each profiles as profile (profile.id)}
            <option value={profile.id}>{profile.name}</option>
          {/each}
        </select>
        {#if profiles.length === 0}
          <Button size="sm" variant="text" icon="add" onclick={() => go("builds")}>
            Создать сборку
          </Button>
        {/if}
      </div>
    {/if}

  {/if}
</div>

<!-- Context menu -->
{#if menu}
  <div
    class="menu-scrim"
    role="presentation"
    onclick={closeMenu}
    oncontextmenu={(event) => {
      event.preventDefault();
      closeMenu();
    }}
  ></div>
  <div class="menu" style={`left: ${menu.x}px; top: ${menu.y}px`} role="menu" tabindex="-1" bind:this={menuEl}
    onkeydown={(event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeMenu();
      }
    }}
  >
    <button
      type="button"
      role="menuitem"
      onclick={() => {
        const server = menu?.server;
        closeMenu();
        if (server) void connect(server);
      }}
    >
      <Icon name="play" size={18} /> Подключиться
    </button>
    <button
      type="button"
      role="menuitem"
      onclick={() => {
        const server = menu?.server;
        closeMenu();
        if (server) openEdit(server);
      }}
    >
      <Icon name="edit" size={18} /> Изменить
    </button>
    <button
      type="button"
      role="menuitem"
      onclick={() => {
        const server = menu?.server;
        closeMenu();
        if (server) void togglePin(server);
      }}
    >
      <Icon name="pin" size={18} /> {menu?.server.pinned ? "Открепить" : "Закрепить"}
    </button>
    <button
      type="button"
      role="menuitem"
      onclick={() => {
        const server = menu?.server;
        closeMenu();
        if (server) {
          lastPing.delete(server.id);
          void refreshStatuses(false);
        }
      }}
    >
      <Icon name="refresh" size={18} /> Обновить статус
    </button>
    <hr />
    <button
      type="button"
      role="menuitem"
      class="danger"
      onclick={() => {
        const server = menu?.server;
        closeMenu();
        if (server) removing = server;
      }}
    >
      <Icon name="delete" size={18} /> Удалить
    </button>
  </div>
{/if}

<!-- Add / edit -->
<Dialog open={form !== null} title={form?.id ? "Изменить сервер" : "Новый сервер"} wide onclose={closeForm}>
  {#if form}
    <div class="form">
      <div class="field">
        <label for="s-name">Название</label>
        <input id="s-name" class="input" bind:value={form.name} maxlength="60" placeholder="Например: Anarchy" />
      </div>

      <div class="two-fields">
        <div class="field grow">
          <label for="s-host">IP-адрес или домен</label>
          <input
            id="s-host"
            class="input mono"
            bind:value={form.host}
            maxlength="253"
            spellcheck="false"
            autocapitalize="off"
            placeholder="play.example.com"
          />
        </div>
        <div class="field port-field">
          <label for="s-port">Порт</label>
          <input id="s-port" class="input" type="number" min="1" max="65535" bind:value={form.port} />
        </div>
      </div>

      <div class="field">
        <label for="s-desc">Описание</label>
        <textarea
          id="s-desc"
          class="textarea"
          bind:value={form.description}
          maxlength="500"
          rows="2"
          placeholder="Короткая заметка на карточке. Необязательно."
        ></textarea>
      </div>

      <div class="field">
        <label for="s-profile">Сборка</label>
        <select
          id="s-profile"
          class="select"
          value={form.profileId ?? ""}
          onchange={(event) =>
            (form!.profileId = (event.currentTarget as HTMLSelectElement).value || null)}
        >
          <option value="">Без сборки</option>
          {#each profiles as profile (profile.id)}
            <option value={profile.id}>{profile.name}</option>
          {/each}
        </select>
        {#if profiles.length === 0}
          <span class="hint">Сборок пока нет. Сервер можно сохранить и привязать позже.</span>
        {:else}
          <span class="hint">При нажатии на карточку запустится именно эта сборка.</span>
        {/if}
      </div>

      <div class="art-row">
        <div class="art-field">
          <span class="field-label">Баннер</span>
          <div class="art-preview banner-preview" style={`--tint: ${form.fallbackColor}`}>
            {#if form.bannerPreview}
              <img src={form.bannerPreview} alt="" />
            {:else if form.iconPreview}
              <img class="preview-icon" src={form.iconPreview} alt="" />
            {:else}
              <span class="placeholder">{form.fallbackText || nameInitials(form.name)}</span>
            {/if}
          </div>
          <div class="row">
            <label class="file-btn">
              <Icon name="image" size={16} /> Загрузить
              <input
                type="file"
                accept="image/png,image/jpeg,image/webp,image/gif"
                onchange={(event) => pickImage(event, "banner")}
              />
            </label>
            {#if form.bannerPreview}
              <Button size="sm" variant="text" icon="delete" onclick={() => removeImage("banner")}>
                Убрать
              </Button>
            {/if}
          </div>
        </div>

        <div class="art-field">
          <span class="field-label">Иконка</span>
          <div class="art-preview icon-preview" style={`--tint: ${form.fallbackColor}`}>
            {#if form.iconPreview}
              <img src={form.iconPreview} alt="" />
            {:else}
              <span class="placeholder">{form.fallbackText || "?"}</span>
            {/if}
          </div>
          <div class="row">
            <label class="file-btn">
              <Icon name="image" size={16} /> Загрузить
              <input
                type="file"
                accept="image/png,image/jpeg,image/webp,image/gif"
                onchange={(event) => pickImage(event, "icon")}
              />
            </label>
            <Button
              size="sm"
              variant="text"
              icon="download"
              disabled={faviconBusy}
              onclick={fetchFavicon}
            >
              {faviconBusy ? "Запрос…" : "С сервера"}
            </Button>
            {#if form.iconPreview}
              <Button size="sm" variant="text" icon="delete" onclick={() => removeImage("icon")}>
                Убрать
              </Button>
            {/if}
          </div>
        </div>
      </div>

      <div class="two-fields">
        <div class="field grow">
          <label for="s-fallback">Текст заглушки</label>
          <input
            id="s-fallback"
            class="input"
            bind:value={form.fallbackText}
            maxlength="3"
            placeholder="AN"
          />
          <span class="hint">1–3 символа, если нет иконки.</span>
        </div>
        <div class="field color-field">
          <label for="s-color">Цвет заглушки</label>
          <input id="s-color" class="color-input" type="color" bind:value={form.fallbackColor} />
        </div>
      </div>

      <div class="row">
        <Button variant="outlined" icon="download" onclick={openImport}>
          Импортировать из Minecraft
        </Button>
        <span class="hint">Откроется список серверов Horizon.</span>
      </div>

      {#if formError}
        <div class="banner">{formError}</div>
      {/if}
    </div>
  {/if}
  {#snippet actions()}
    <Button variant="text" onclick={closeForm} disabled={saving}>Отмена</Button>
    <Button icon="check" onclick={saveForm} disabled={saving}>
      {saving ? "Сохраняем…" : "Сохранить"}
    </Button>
  {/snippet}
</Dialog>

<!-- Import -->
<Dialog open={importOpen} title="Импорт из Minecraft" wide onclose={closeImport}>
  {#if importBusy && !importScan}
    <p>Ищу список серверов Horizon…</p>
  {:else if importScan}
    {#if importScan.servers.length === 0}
      <div class="empty">
        <Icon name="servers" size={28} />
        <strong>Серверы не найдены</strong>
        <span>
          {importScan.error ??
            "Добавьте сервер в меню мультиплеера Horizon и повторите поиск. Файл читается без запуска игры."}
        </span>
        <Button variant="tonal" icon="refresh" onclick={scanImport} disabled={importBusy}>
          Искать снова
        </Button>
      </div>
    {:else}
      <p class="hint">
        Найдено серверов: {importScan.servers.length}. Источник:
        <span class="mono">{importScan.source}</span>. Файл Horizon только читается.
      </p>
      {#if importScan.extraSources.length > 0}
        <p class="hint">
          Дополнительно прочитано:
          <span class="mono">{importScan.extraSources.join(", ")}</span>
        </p>
      {/if}
      {#if importError}
        <div class="banner">{importError}</div>
      {/if}

      <div class="import-list">
        {#each importScan.servers as item (importKey(item))}
          <div class="import-row" class:done={item.alreadyAdded}>
            <label class="check">
              <input
                type="checkbox"
                disabled={item.alreadyAdded}
                checked={!item.alreadyAdded && Boolean(importSelected[importKey(item)])}
                onchange={(event) => {
                  importSelected[importKey(item)] = (event.currentTarget as HTMLInputElement).checked;
                }}
              />
            </label>
            <div class="import-icon">
              {#if item.icon}
                <img src={item.icon} alt="" />
              {:else}
                <span>{Array.from(item.name).slice(0, 2).join("").toUpperCase()}</span>
              {/if}
            </div>
            <div class="grow">
              <strong title={item.name}>{item.name}</strong>
              <span class="mono">
                {item.port === 25565 ? item.host : `${item.host}:${item.port}`}
              </span>
            </div>
            {#if item.alreadyAdded}
              <span class="badge">уже добавлен</span>
            {:else}
              <select
                class="select slim"
                aria-label={`Сборка для ${item.name}`}
                value={importProfiles[importKey(item)] ?? ""}
                onchange={(event) =>
                  (importProfiles[importKey(item)] =
                    (event.currentTarget as HTMLSelectElement).value || null)}
              >
                <option value="">Без сборки</option>
                {#each profiles as profile (profile.id)}
                  <option value={profile.id}>{profile.name}</option>
                {/each}
              </select>
            {/if}
          </div>
        {/each}
      </div>

      <div class="row import-tools">
        <Button size="sm" variant="text" onclick={selectAll}>Выбрать все</Button>
        <Button size="sm" variant="text" onclick={selectNone}>Снять выделение</Button>
        <span class="spacer"></span>
        <span class="muted small">Выбрано: {importCount}</span>
        <Button size="sm" variant="text" icon="refresh" onclick={scanImport} disabled={importBusy}>
          Повторить поиск
        </Button>
      </div>
    {/if}
  {:else}
    <div class="banner">{importError || "Не удалось прочитать список серверов Horizon."}</div>
    <div class="row" style="margin-top:14px">
      <Button variant="tonal" icon="refresh" onclick={scanImport}>Искать снова</Button>
    </div>
  {/if}
  {#snippet actions()}
    <Button variant="text" onclick={closeImport} disabled={importBusy}>Закрыть</Button>
    <Button icon="download" onclick={confirmImport} disabled={importBusy || importCount === 0}>
      {importBusy ? "Импортируем…" : `Импортировать выбранные${importCount ? ` (${importCount})` : ""}`}
    </Button>
  {/snippet}
</Dialog>

<!-- Delete -->
<Dialog open={removing !== null} title="Удалить сервер?" onclose={() => (saving ? null : (removing = null))}>
  <p>
    Сервер <strong>{removing?.name}</strong> исчезнет из GERA вместе со своим баннером и иконкой.
    Сборка, её миры и моды останутся на диске. Список серверов Horizon не изменится.
  </p>
  {#snippet actions()}
    <Button variant="text" onclick={() => (removing = null)} disabled={saving}>Отмена</Button>
    <Button variant="danger" icon="delete" onclick={confirmRemove} disabled={saving}>
      {saving ? "Удаляем…" : "Удалить сервер"}
    </Button>
  {/snippet}
</Dialog>

<style>
  .head-actions {
    justify-content: flex-end;
  }
  .servers-empty {
    padding: 56px 24px;
    gap: 10px;
  }

  .sortbar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .servers {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .server {
    position: relative;
    display: flex;
    flex-direction: column;
    border-radius: var(--shape-lg);
    background: var(--surface-container-low);
    overflow: hidden;
    cursor: pointer;
    outline: none;
    transition:
      background-color var(--motion) var(--ease-standard),
      box-shadow var(--motion) var(--ease-standard),
      transform var(--motion) var(--ease-standard);
  }
  .server:hover {
    background: var(--surface-container);
  }
  :global([data-ui="heavy"]) .server:hover {
    transform: translateY(-1px);
    box-shadow: var(--elevation-1);
  }
  .server:focus-visible {
    box-shadow: 0 0 0 2px var(--primary);
  }
  .server.drag-over {
    box-shadow: inset 0 0 0 2px var(--primary);
  }
  .server.launching {
    opacity: 0.72;
  }

  .banner-area {
    position: relative;
    height: 130px;
    overflow: hidden;
    background: color-mix(in srgb, var(--tint) 22%, var(--surface-container-high));
  }
  :global([data-gradient="on"]) .banner-area {
    background: linear-gradient(
      112deg,
      color-mix(in srgb, var(--tint) 32%, var(--surface-container-high)) 0%,
      var(--surface-container-high) 64%
    );
  }
  .banner-area > img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .banner-fallback {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  .banner-letters {
    font-size: 56px;
    font-weight: 800;
    letter-spacing: -0.05em;
    color: color-mix(in srgb, var(--tint) 70%, var(--on-surface));
    opacity: 0.32;
    user-select: none;
  }
  .banner-icon {
    width: 68px !important;
    height: 68px !important;
    border-radius: 15px;
    object-fit: cover;
    box-shadow: var(--elevation-1);
  }
  .pin-badge {
    position: absolute;
    top: 10px;
    left: 12px;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px;
    border-radius: var(--shape-full);
    background: color-mix(in srgb, var(--surface) 84%, transparent);
    color: var(--on-surface);
    font-size: 11px;
    font-weight: 700;
  }

  .server-body {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 14px 18px 16px;
  }
  .drag-handle {
    display: grid;
    place-items: center;
    width: 20px;
    height: 34px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--on-surface-variant);
    opacity: 0.55;
    cursor: grab;
    flex: 0 0 auto;
  }
  .drag-handle:active {
    cursor: grabbing;
  }
  .server-icon {
    width: 54px;
    height: 54px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    overflow: hidden;
    border-radius: 14px;
    background: color-mix(in srgb, var(--tint) 26%, var(--surface-container-high));
    color: color-mix(in srgb, var(--tint) 72%, var(--on-surface));
    font-size: 18px;
    font-weight: 800;
    box-shadow: inset 0 0 0 1px var(--outline-variant);
  }
  .server-icon img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .server-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .server-main h3 {
    margin: 0;
    font-size: 15.5px;
    font-weight: 700;
    letter-spacing: -0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .server-profile {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 650;
    color: var(--on-surface-variant);
  }
  .server-note {
    font-size: 12px;
    color: var(--on-surface-variant);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    opacity: 0.9;
  }

  .server-endpoint {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
    text-align: right;
    max-width: 38%;
    flex: 0 0 auto;
  }
  .address {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 650;
    color: var(--on-surface-variant);
  }
  .state .dot {
    width: 7px;
    height: 7px;
  }
  .state.ok {
    color: var(--primary);
  }
  .state.warn {
    color: var(--error);
  }
  .players {
    font-size: 11.5px;
    color: var(--on-surface-variant);
  }

  .server-actions {
    flex: 0 0 auto;
  }

  .launch-info {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-3);
    padding: 12px 16px;
    border-radius: var(--shape-lg);
    background: var(--surface-container);
    color: var(--on-surface-variant);
  }
  .launch-info .grow {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1 1 220px;
  }
  .launch-info strong {
    font-size: 13.5px;
    color: var(--on-surface);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .launch-info span {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .launch-info .select.slim {
    flex: 0 0 auto;
    margin-left: auto;
  }

  .select.slim {
    height: 38px;
    padding: 0 40px 0 14px;
    background-position:
      calc(100% - 19px) 16px,
      calc(100% - 14px) 16px;
  }

  .menu-scrim {
    position: fixed;
    inset: 0;
    z-index: 29;
  }
  .menu {
    position: fixed;
    z-index: 30;
    width: 220px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    border-radius: var(--shape-md);
    background: var(--surface-container-high);
    box-shadow: var(--elevation-2);
    animation: menu-pop var(--motion-fast) var(--ease-standard);
  }
  .menu button {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    border: none;
    border-radius: var(--shape-sm);
    background: transparent;
    color: var(--on-surface);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover {
    background: color-mix(in srgb, var(--on-surface) 8%, transparent);
  }
  .menu button.danger {
    color: var(--error);
  }
  .menu hr {
    height: 1px;
    border: none;
    margin: 5px 4px;
    background: var(--outline-variant);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  /* Dialogs use surface-container-high, which is exactly the shared control
     colour. Lift the fields one step so they stay visible inside the form. */
  .form .input,
  .form .select,
  .form .textarea,
  .form .file-btn {
    background-color: var(--surface-container-highest);
  }
  .two-fields {
    display: flex;
    align-items: flex-end;
    gap: 12px;
  }
  .port-field {
    width: 112px;
    flex: 0 0 auto;
  }
  .color-field {
    width: 96px;
    flex: 0 0 auto;
  }

  .art-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .art-field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .art-preview {
    position: relative;
    height: 92px;
    display: grid;
    place-items: center;
    overflow: hidden;
    border-radius: var(--shape-md);
    background:
      linear-gradient(
        112deg,
        color-mix(in srgb, var(--tint) 32%, var(--surface-container-high)) 0%,
        var(--surface-container-high) 70%
      );
    box-shadow: inset 0 0 0 1px var(--outline-variant);
  }
  .art-preview > img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .art-preview .preview-icon {
    width: 54px;
    height: 54px;
    border-radius: 12px;
    box-shadow: var(--elevation-1);
  }
  .icon-preview {
    width: 92px;
  }
  .placeholder {
    font-size: 26px;
    font-weight: 800;
    color: color-mix(in srgb, var(--tint) 72%, var(--on-surface));
    opacity: 0.4;
    user-select: none;
  }

  .file-btn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 36px;
    padding: 0 14px;
    border-radius: var(--shape-full);
    background: var(--surface-container-high);
    color: var(--on-surface);
    font-size: 12.5px;
    font-weight: 650;
    cursor: pointer;
    transition: background-color var(--motion-fast) var(--ease-standard);
  }
  .file-btn:hover {
    background: var(--surface-container-highest);
  }
  .file-btn input {
    display: none;
  }

  .import-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 340px;
    overflow: auto;
    margin: 12px 0;
    padding-right: 2px;
  }
  .import-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
  }
  .import-row.done {
    opacity: 0.62;
  }
  .check input {
    width: 18px;
    height: 18px;
    accent-color: var(--primary);
    cursor: pointer;
  }
  .import-icon {
    width: 36px;
    height: 36px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    overflow: hidden;
    border-radius: 10px;
    background: var(--surface-container-highest);
    font-size: 12px;
    font-weight: 750;
  }
  .import-icon img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .import-row .grow {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .import-row strong {
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .import-row .mono {
    font-size: 11.5px;
    color: var(--on-surface-variant);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .import-row .select {
    width: 170px;
    flex: 0 0 auto;
  }
  .import-tools {
    margin-top: 4px;
  }

  @media (max-width: 760px) {
    .server-body {
      flex-wrap: wrap;
      gap: 10px 12px;
    }
    .drag-handle {
      display: none;
    }
    .server-endpoint {
      order: 10;
      flex-basis: 100%;
      max-width: none;
      align-items: flex-start;
      text-align: left;
      padding-left: 66px;
    }
    .server-note {
      display: none;
    }
    .launch-info {
      flex-wrap: wrap;
    }
    .launch-info .select {
      width: 100%;
      max-width: none;
    }
    .art-row {
      grid-template-columns: 1fr;
    }
    .import-row {
      flex-wrap: wrap;
    }
    .import-row .select {
      width: 100%;
    }
    .head-actions {
      width: 100%;
    }
  }

  @media (max-width: 520px) {
    .banner-area {
      height: 104px;
    }
    .two-fields {
      flex-wrap: wrap;
    }
    .port-field,
    .color-field {
      width: 100%;
    }
  }

  @keyframes menu-pop {
    from {
      opacity: 0;
      transform: translateY(-4px) scale(0.98);
    }
  }
</style>
