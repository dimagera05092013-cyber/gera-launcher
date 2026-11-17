<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";
  import { updater, startUpdate, postponeUpdate } from "../update.svelte";

  const info = $derived(updater.info);
  const percent = $derived(
    updater.total > 0 ? Math.min(100, Math.round((updater.downloaded / updater.total) * 100)) : 0,
  );

  function megabytes(bytes: number): string {
    if (bytes <= 0) return "0 МБ";
    return `${(bytes / 1024 / 1024).toFixed(1).replace(".", ",")} МБ`;
  }
</script>

<Dialog
  open={updater.dialog && info !== null}
  title={updater.phase === "error" ? "Не удалось обновиться" : "Доступна новая версия"}
  wide
  onclose={postponeUpdate}
>
  {#if info}
    {#if updater.phase === "installing"}
      <div class="state">
        <span class="mark"><Icon name="update" size={26} /></span>
        <strong>Перезапускаем GERA…</strong>
        <p>Лаунчер заменит свой файл и откроется снова. Это займёт пару секунд.</p>
      </div>
    {:else if updater.phase === "downloading"}
      <div class="state">
        <strong>Скачиваем GERA {info.version}</strong>
        <div
          class="bar"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          aria-label="Загрузка обновления"
        >
          <span style={`width:${percent}%`}></span>
        </div>
        <p><span class="mono">{percent}%</span> · {megabytes(updater.downloaded)} из {megabytes(
          updater.total || info.size,
        )}</p>
      </div>
    {:else if updater.phase === "error"}
      <p class="lead">{updater.error}</p>
      <p class="hint">Файл лаунчера не изменён. Можно попробовать ещё раз или обновиться позже.</p>
    {:else}
      <p class="lead">
        Вышла версия <strong>{info.version}</strong>. Сейчас установлена {info.current}.
      </p>
      {#if info.notes}
        <div class="notes">{info.notes}</div>
      {:else}
        <p class="hint">Описание изменений не указано.</p>
      {/if}
      <p class="hint">
        GERA скачает новый файл ({megabytes(info.size)}), проверит его контрольную сумму и
        перезапустится. Данные, сборки и серверы останутся на месте.
      </p>
    {/if}
  {/if}

  {#snippet actions()}
    {#if updater.phase === "error"}
      <Button variant="text" onclick={postponeUpdate}>Позже</Button>
      <Button icon="refresh" onclick={startUpdate}>Повторить</Button>
    {:else if updater.phase === "idle"}
      <Button variant="text" onclick={postponeUpdate}>Позже</Button>
      <Button icon="download" onclick={startUpdate}>Обновиться</Button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .lead {
    margin: 0 0 12px;
    color: var(--on-surface);
    font-size: 15px;
  }
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 12px 4px 6px;
    text-align: center;
  }
  .state strong {
    font-size: 16px;
    color: var(--on-surface);
  }
  .state p {
    margin: 0;
    font-size: 13px;
    color: var(--on-surface-variant);
  }
  .mark {
    width: 52px;
    height: 52px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .bar {
    width: 100%;
    height: 8px;
    border-radius: 999px;
    background: var(--surface-container-highest);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--primary);
    transition: width 150ms linear;
  }
  .notes {
    max-height: 240px;
    overflow: auto;
    padding: 12px 14px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
    color: var(--on-surface-variant);
    font-size: 13px;
    line-height: 1.55;
    white-space: pre-wrap;
  }
  .hint {
    margin: 10px 0 0;
    font-size: 12.5px;
    color: var(--on-surface-variant);
  }
  .mono {
    font-variant-numeric: tabular-nums;
  }
</style>
