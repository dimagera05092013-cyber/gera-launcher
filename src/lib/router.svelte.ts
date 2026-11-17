import type { PageId } from "./store.svelte";
import { ui } from "./store.svelte";

export type { PageId } from "./store.svelte";

type Loader = () => Promise<{ default: unknown }>;

const registry: Record<PageId, Loader> = {
  home: () => import("../pages/Home.svelte"),
  mods: () => import("../pages/Mods.svelte"),
  builds: () => import("../pages/Builds.svelte"),
  servers: () => import("../pages/Servers.svelte"),
  settings: () => import("../pages/Settings.svelte"),
};

export const pageMeta: Record<PageId, { title: string; subtitle: string }> = {
  home: { title: "Главная", subtitle: "Ваша сборка и запуск игры" },
  builds: { title: "Сборки", subtitle: "Миры, моды и настройки — всё раздельно" },
  servers: { title: "Серверы", subtitle: "Свои серверы, импорт из Horizon и вход в один клик" },
  mods: { title: "Моды", subtitle: "Каталог Horizon Mods Registry" },
  settings: { title: "Настройки", subtitle: "Внешний вид, клиент и данные" },
};

export async function go(page: PageId) {
  if (ui.page === page && ui.pageComponent) return;
  ui.pageLoading = true;
  try {
    const module = await registry[page]();
    ui.pageComponent = module.default;
    ui.page = page;
    document.querySelector(".content")?.scrollTo({ top: 0 });
  } catch (error) {
    console.error(error);
  } finally {
    ui.pageLoading = false;
  }
}

export async function start(page: PageId) {
  ui.pageComponent = null;
  ui.pageLoading = true;
  // Preload every page once: switching sections then happens without a
  // network/disk pause, so the header and content never disagree.
  await Promise.all(Object.values(registry).map((load) => load().catch(() => null)));
  return go(page);
}
