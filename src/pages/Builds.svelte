<script lang="ts">
  import { onMount } from "svelte";
  import { ui, notify, reload, activateProfile } from "../lib/store.svelte";
  import { go } from "../lib/router.svelte";
  import { api, errorText } from "../lib/api";
  import type { Profile } from "../lib/types";
  import Button from "../lib/components/Button.svelte";
  import IconButton from "../lib/components/IconButton.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Dialog from "../lib/components/Dialog.svelte";

  const boot = $derived(ui.boot);
  const builds = $derived(boot?.profiles ?? []);
  const versions = $derived(boot?.horizon.versions ?? []);
  const activeId = $derived(boot?.activeProfile?.id ?? null);

  let form = $state<Profile | null>(null);
  let formError = $state("");
  let saving = $state(false);
  let removing = $state<Profile | null>(null);
  let launchingId = $state<string | null>(null);
  let copying = $state<Profile | null>(null);
  let copyName = $state("");
  let copyBusy = $state(false);
  let counts = $state<Record<string, number | null>>({});

  onMount(() => {
    void loadCounts();
  });

  async function loadCounts() {
    const list = builds;
    const next: Record<string, number | null> = {};
    await Promise.all(
      list.map(async (item) => {
        try {
          const [, mods] = await api.modsInstalled(item.id);
          next[item.id] = mods.length;
        } catch {
          next[item.id] = null;
        }
      }),
    );
    counts = next;
  }

  function blank(): Profile {
    return {
      id: "",
      name: "Новая сборка",
      description: "",
      playerName: "Player",
      gameDir: "",
      horizonVersion: null,
      skin: null,
      detected: false,
      createdAt: 0,
      lastPlayed: null,
      lastExitCode: null,
    };
  }

  function create() {
    formError = "";
    form = blank();
  }

  function edit(profile: Profile) {
    formError = "";
    form = { ...profile };
  }

  function close() {
    if (saving) return;
    form = null;
  }

  async function save() {
    if (!form) return;
    saving = true;
    formError = "";
    try {
      const skin = form.skin?.trim() ? form.skin.trim() : null;
      await api.saveProfile({
        ...form,
        name: form.name.trim(),
        description: form.description?.trim() ?? "",
        skin,
      });
      await reload();
      await loadCounts();
      form = null;
      notify("Сборка сохранена.", "success");
    } catch (caught) {
      formError = errorText(caught);
    } finally {
      saving = false;
    }
  }

  function openCopy(profile: Profile) {
    copying = profile;
    copyName = `${profile.name} — копия`;
  }

  async function confirmCopy() {
    if (!copying || copyBusy) return;
    copyBusy = true;
    try {
      await api.duplicateProfile(copying.id, copyName);
      copying = null;
      await reload();
      await loadCounts();
      notify("Сборка скопирована: миры, моды и настройки на месте.", "success");
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      copyBusy = false;
    }
  }

  async function confirmRemove() {
    if (!removing) return;
    saving = true;
    try {
      await api.removeProfile(removing.id);
      removing = null;
      await reload();
      await loadCounts();
      notify("Сборка убрана из списка. Папка с мирами не удалена.", "info");
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      saving = false;
    }
  }

  async function openPath(path: string) {
    try {
      await api.openPath(path);
    } catch (caught) {
      notify(errorText(caught), "error");
    }
  }

  async function makeActive(profile: Profile) {
    await activateProfile(profile.id);
    notify(`Активная сборка: ${profile.name}`, "success");
  }

  async function play(profile: Profile) {
    launchingId = profile.id;
    try {
      await api.activateProfile(profile.id);
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
      await reload();
      notify(`Horizon запускается: ${profile.playerName}`, "success");
    } catch (caught) {
      notify(errorText(caught), "error");
    } finally {
      launchingId = null;
    }
  }

  function lastPlayed(profile: Profile): string {
    if (!profile.lastPlayed) return "ещё не запускалась";
    return new Date(profile.lastPlayed).toLocaleString("ru-RU", {
      day: "numeric",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function versionLabel(profile: Profile): string {
    if (!profile.horizonVersion) return "авто";
    const match = profile.horizonVersion.match(/(\d{4}-\d{2}-\d{2})/);
    return match?.[1] ?? profile.horizonVersion;
  }

  function tone(index: number): string {
    return `tone-${(index % 3) + 1}`;
  }
</script>

<svelte:head><title>Сборки · GERA Launcher</title></svelte:head>

<div class="page-stack">
  <div class="page-head">
    <div>
      <h2>Ваши сборки</h2>
      <p>
        У каждой сборки своя папка: миры, моды и настройки не пересекаются. GERA не перемещает и не
        удаляет существующие папки.
      </p>
    </div>
    <Button icon="add" onclick={create}>Новая сборка</Button>
  </div>

  {#if builds.length === 0}
    <div class="empty">
      <Icon name="builds" size={30} />
      <strong>Сборок пока нет</strong>
      <span>Создайте первую — GERA заведёт отдельную папку в своих данных или возьмёт существующую.</span>
      <Button variant="tonal" icon="add" onclick={create}>Создать сборку</Button>
    </div>
  {:else}
    <div class="grid builds">
      {#each builds as profile, index (profile.id)}
        <article class="build {tone(index)}" class:active={profile.id === activeId}>
          <div class="build-art" aria-hidden="true">
            <span class="art-letter">{profile.playerName.slice(0, 1).toUpperCase()}</span>
            <span class="art-icon"><Icon name="builds" size={20} /></span>
          </div>

          <div class="build-body">
            <header>
              <div class="names">
                <h3>{profile.name}</h3>
                <span>{profile.playerName}</span>
              </div>
              {#if profile.id === activeId}
                <span class="badge ok">активная</span>
              {:else if profile.detected}
                <span class="badge">обнаружена</span>
              {/if}
            </header>

            <p class="description">
              {profile.description?.trim() || "Без описания. Добавьте его в настройках сборки."}
            </p>

            <div class="chips">
              <span class="chip">Horizon {versionLabel(profile)}</span>
              <span class="chip">
                {counts[profile.id] === undefined
                  ? "…"
                  : counts[profile.id] === null
                    ? "—"
                    : counts[profile.id]}
                модов
              </span>
              <span class="chip outlined">{lastPlayed(profile)}</span>
            </div>

            <div class="path" title={profile.gameDir}>
              <Icon name="folder" size={14} />
              <span class="mono">{profile.gameDir}</span>
            </div>

            <footer>
              <Button
                size="sm"
                icon="play"
                disabled={launchingId !== null}
                onclick={() => play(profile)}
              >
                {launchingId === profile.id ? "Запуск…" : "Играть"}
              </Button>
              {#if profile.id !== activeId}
                <Button size="sm" variant="tonal" onclick={() => makeActive(profile)}>Выбрать</Button>
              {/if}
              <span class="spacer"></span>
              <IconButton
                icon="folder"
                label="Открыть игровую папку"
                variant="text"
                size="sm"
                onclick={() => openPath(profile.gameDir)}
              />
              <IconButton
                icon="edit"
                label="Изменить сборку"
                variant="text"
                size="sm"
                onclick={() => edit(profile)}
              />
              <IconButton
                icon="copy"
                label="Дублировать сборку"
                variant="text"
                size="sm"
                onclick={() => openCopy(profile)}
              />
              <IconButton
                icon="delete"
                label="Убрать из списка"
                variant="text"
                size="sm"
                onclick={() => (removing = profile)}
              />
            </footer>
          </div>
        </article>
      {/each}
    </div>
  {/if}

  <div class="banner info">
    <Icon name="info" size={20} />
    <div>
      <strong>Данные в безопасности.</strong>
      Удаление сборки убирает только запись из GERA. Миры, настройки и моды остаются в папке на
      диске. Дублирование копирует миры и моды, но пропускает кэш, логи и скриншоты.
    </div>
  </div>

  <div class="row">
    <Button variant="text" icon="arrow" onclick={() => go("home")}>На главную</Button>
  </div>
</div>

<Dialog open={form !== null} title={form?.id ? "Изменить сборку" : "Новая сборка"} onclose={close}>
  {#if form}
    <div class="form">
      <div class="field">
        <label for="b-name">Название</label>
        <input id="b-name" class="input" bind:value={form.name} maxlength="40" />
      </div>
      <div class="field">
        <label for="b-desc">Описание</label>
        <textarea
          id="b-desc"
          class="textarea"
          bind:value={form.description}
          maxlength="180"
          rows="2"
          placeholder="Например: ванильное выживание с Veinminer"
        ></textarea>
        <span class="hint">Короткая заметка на карточке сборки. Необязательно.</span>
      </div>
      <div class="field">
        <label for="b-nick">Ник в игре</label>
        <input
          id="b-nick"
          class="input"
          bind:value={form.playerName}
          maxlength="16"
          spellcheck="false"
          autocapitalize="off"
        />
        <span class="hint">3–16 символов: латиница, цифры и «_». Офлайн-ник Horizon.</span>
      </div>
      <div class="field">
        <label for="b-version">Версия Horizon</label>
        <select
          id="b-version"
          class="select"
          value={form.horizonVersion ?? ""}
          onchange={(event) =>
            (form!.horizonVersion = (event.currentTarget as HTMLSelectElement).value || null)}
        >
          <option value="">Авто — новейшая с готовыми ресурсами</option>
          {#each versions as version (version.tag)}
            <option value={version.tag}>
              {version.tag}{version.ready ? "" : " — ресурсы не готовы"}
            </option>
          {/each}
        </select>
      </div>
      {#if form.id && form.detected}
        <div class="field">
          <label for="b-dir">Игровая папка</label>
          <input id="b-dir" class="input mono" value={form.gameDir} disabled />
          <span class="hint">Обнаруженная папка не перемещается лаунчером.</span>
        </div>
      {:else if form.id}
        <div class="field">
          <label for="b-dir">Игровая папка</label>
          <input id="b-dir" class="input mono" bind:value={form.gameDir} spellcheck="false" />
          <span class="hint">Должна существовать. Миры и настройки остаются на месте.</span>
        </div>
      {:else}
        <div class="field">
          <span class="field-label">Игровая папка</span>
          <div class="auto-dir">
            <Icon name="folder" size={16} />
            <span>
              Будет создана в данных GERA:
              <span class="mono">profiles/{form.name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}/game</span>
            </span>
          </div>
        </div>
      {/if}
      <div class="field">
        <label for="b-skin">Скин (необязательно)</label>
        <input
          id="b-skin"
          class="input mono"
          value={form.skin ?? ""}
          placeholder="/home/…/skin.png"
          spellcheck="false"
          oninput={(event) =>
            (form!.skin = (event.currentTarget as HTMLInputElement).value || null)}
        />
        <span class="hint">PNG-файл. Horizon принимает его через HORIZON_OFFLINE_SKIN.</span>
      </div>
      {#if formError}
        <div class="banner" role="alert">{formError}</div>
      {/if}
    </div>
  {/if}
  {#snippet actions()}
    <Button variant="text" onclick={close} disabled={saving}>Отмена</Button>
    <Button variant="filled" onclick={save} disabled={saving}>
      {saving ? "Сохраняем…" : "Сохранить"}
    </Button>
  {/snippet}
</Dialog>

<Dialog
  open={copying !== null}
  title="Дублировать сборку?"
  onclose={() => (copyBusy ? null : (copying = null))}
>
  <p>
    Копия сборки <strong>{copying?.name}</strong> получит свою папку: миры, моды, конфиги и
    настройки. Кэш, логи, скриншоты и резервные копии не копируются.
  </p>
  <div class="field" style="margin-top:14px">
    <label for="copy-name">Название копии</label>
    <input id="copy-name" class="input" bind:value={copyName} maxlength="40" />
  </div>
  {#snippet actions()}
    <Button variant="text" onclick={() => (copying = null)} disabled={copyBusy}>Отмена</Button>
    <Button variant="filled" icon="copy" onclick={confirmCopy} disabled={copyBusy || !copyName.trim()}>
      {copyBusy ? "Копируем…" : "Дублировать"}
    </Button>
  {/snippet}
</Dialog>

<Dialog open={removing !== null} title="Убрать сборку?" onclose={() => (removing = null)}>
  <p>
    Сборка <strong>{removing?.name}</strong> исчезнет из GERA. Папка
    <span class="mono">{removing?.gameDir}</span> и всё её содержимое останутся на диске.
  </p>
  {#snippet actions()}
    <Button variant="text" onclick={() => (removing = null)} disabled={saving}>Отмена</Button>
    <Button variant="danger" onclick={confirmRemove} disabled={saving}>
      {saving ? "Убираем…" : "Убрать из списка"}
    </Button>
  {/snippet}
</Dialog>

<style>
  .builds {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  @media (max-width: 860px) {
    .builds {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .build {
    display: flex;
    flex-direction: column;
    border-radius: var(--shape-2xl);
    background: var(--surface-container-low);
    overflow: hidden;
    transition:
      background-color var(--motion) var(--ease-standard),
      transform var(--motion) var(--ease-standard);
  }
  :global([data-ui="heavy"]) .build:hover {
    transform: translateY(-2px);
    box-shadow: var(--elevation-1);
  }
  .build.active {
    background: var(--surface-container);
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--primary) 55%, transparent);
  }

  .build-art {
    position: relative;
    height: 92px;
    display: flex;
    align-items: center;
    padding: 0 22px;
    overflow: hidden;
  }
  .tone-1 .build-art {
    background: var(--tone-1);
  }
  .tone-2 .build-art {
    background: var(--tone-2);
  }
  .tone-3 .build-art {
    background: var(--tone-3);
  }
  .art-letter {
    font-size: 46px;
    font-weight: 900;
    line-height: 1;
    letter-spacing: -0.05em;
    color: var(--on-primary-container);
    opacity: 0.9;
  }
  .tone-2 .art-letter {
    color: var(--on-secondary-container);
  }
  .tone-3 .art-letter {
    color: var(--on-tertiary-container);
  }
  .art-icon {
    position: absolute;
    right: 20px;
    bottom: -18px;
    width: 74px;
    height: 74px;
    display: grid;
    place-items: center;
    border-radius: 24px;
    transform: rotate(-10deg);
    background: color-mix(in srgb, var(--surface) 22%, transparent);
    color: var(--on-primary-container);
  }

  .build-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 18px 20px 16px;
  }
  header {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .names {
    min-width: 0;
    flex: 1;
  }
  .names h3 {
    margin: 0;
    font-size: 18px;
    font-weight: 750;
    letter-spacing: -0.02em;
    line-height: 1.2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .names span {
    font-size: 12.5px;
    color: var(--on-surface-variant);
  }
  .description {
    margin: 0;
    min-height: 40px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--on-surface-variant);
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .path {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 8px 10px;
    border-radius: var(--shape-sm);
    background: var(--surface-container);
    color: var(--on-surface-variant);
    overflow: hidden;
    white-space: nowrap;
  }
  .path span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
    margin-top: 2px;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .auto-dir {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
    color: var(--on-surface-variant);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
</style>
