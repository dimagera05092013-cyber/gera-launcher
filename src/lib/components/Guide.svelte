<script lang="ts">
  import { ui, dismissGuide } from "../store.svelte";
  import { go } from "../router.svelte";
  import Dialog from "./Dialog.svelte";
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "../icons";

  interface Step {
    icon: IconName;
    title: string;
    text: string;
    hint: string;
    page?: "home" | "builds" | "settings";
    action?: string;
  }

  const steps: Step[] = [
    {
      icon: "java",
      title: "Java",
      text: "Ставить Java вручную не нужно. Если подходящей версии нет, GERA сама скачает Temurin с официального сервера Adoptium.",
      hint: "Уже установленная Java тоже подойдёт.",
    },
    {
      icon: "chip",
      title: "Клиент Horizon",
      text: "В настройках нажмите «Установить клиент» — GERA скачает официальный релиз с GitHub и проверит файл по SHA-256.",
      hint: "Если клиент уже есть, GERA найдёт его сама.",
      page: "settings",
      action: "Открыть настройки",
    },
    {
      icon: "builds",
      title: "Сборка",
      text: "На странице «Сборки» создайте сборку: название, ник и папка с мирами. Клиент и Java подставятся автоматически.",
      hint: "Сборок может быть сколько угодно — у каждой своя папка.",
      page: "builds",
      action: "Открыть сборки",
    },
    {
      icon: "play",
      title: "Запуск",
      text: "На главной выберите сборку и нажмите «Играть». Первый запуск дольше обычного: скачиваются библиотеки и ресурсы игры.",
      hint: "Если игра закроется сразу, GERA покажет причину и путь к логу.",
      page: "home",
      action: "На главную",
    },
  ];

  let step = $state(0);
  const current = $derived(steps[step]);
  const last = $derived(step === steps.length - 1);

  // Every open starts from the first step, even when the guide is reopened
  // from settings later.
  $effect(() => {
    if (ui.guideOpen) step = 0;
  });

  function openPage() {
    const page = current.page;
    dismissGuide();
    if (page) void go(page);
  }
</script>

<Dialog
  open={ui.guideOpen}
  title="Как подготовить Minecraft"
  onclose={dismissGuide}
>
  <div class="guide">
    <div class="head">
      <span class="mark"><Icon name={current.icon} size={24} /></span>
      <span class="count">Шаг {step + 1} из {steps.length}</span>
    </div>
    <h3>{current.title}</h3>
    <p>{current.text}</p>
    <p class="hint">{current.hint}</p>
  </div>

  <div class="dots" aria-hidden="true">
    {#each steps as _, index (index)}
      <span class:active={index === step}></span>
    {/each}
  </div>

  {#snippet actions()}
    <Button variant="text" onclick={dismissGuide}>Пропустить</Button>
    {#if step > 0}
      <Button variant="text" icon="back" onclick={() => (step -= 1)}>Назад</Button>
    {/if}
    {#if current.action}
      <Button variant="tonal" icon="open" onclick={openPage}>{current.action}</Button>
    {/if}
    {#if last}
      <Button variant="filled" icon="check" onclick={dismissGuide}>Начать</Button>
    {:else}
      <Button variant="filled" onclick={() => (step += 1)}>Далее</Button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }
  .mark {
    width: 46px;
    height: 46px;
    display: grid;
    place-items: center;
    border-radius: var(--shape-lg);
    background: var(--primary-container);
    color: var(--on-primary-container);
  }
  .count {
    font-size: 12px;
    font-weight: 650;
    color: var(--on-surface-variant);
  }
  h3 {
    margin: 0 0 6px;
    font-size: 17px;
    font-weight: 700;
    color: var(--on-surface);
  }
  .hint {
    padding: 8px 12px;
    border-radius: var(--shape-md);
    background: var(--surface-container);
    font-size: 12.5px;
  }
  .dots {
    display: flex;
    gap: 6px;
    margin-top: 16px;
  }
  .dots span {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--outline-variant);
    transition: background-color var(--motion-fast) var(--ease-standard);
  }
  .dots span.active {
    background: var(--primary);
  }
</style>
