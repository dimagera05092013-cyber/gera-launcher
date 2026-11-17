import type { Bootstrap, Profile, Settings } from "./types";
import { api, errorText } from "./api";
import { applyTheme } from "./theme";

export type ToastKind = "info" | "success" | "error";

export interface Toast {
  id: number;
  text: string;
  kind: ToastKind;
}

export type PageId = "home" | "builds" | "servers" | "mods" | "settings";

export const ui = $state({
  ready: false,
  loading: true,
  error: "",
  boot: null as Bootstrap | null,
  page: "home" as PageId,
  pageComponent: null as unknown,
  pageLoading: false,
  busy: false,
  toast: null as Toast | null,
});

let toastId = 0;

export function notify(text: string, kind: ToastKind = "info") {
  ui.toast = { id: ++toastId, text, kind };
}

export function activeProfile(): Profile | null {
  return ui.boot?.activeProfile ?? null;
}

export function activeSettings(): Settings | null {
  return ui.boot?.settings ?? null;
}

export async function init() {
  ui.loading = true;
  ui.error = "";
  try {
    const boot = await api.bootstrap();
    ui.boot = boot;
    applyTheme(boot.settings.accent, boot.settings.theme, boot.settings.uiMode, boot.settings.gradients);
    ui.ready = true;
  } catch (error) {
    ui.error = errorText(error);
  } finally {
    ui.loading = false;
  }
}

export async function reload() {
  if (!ui.boot) return init();
  try {
    const boot = await api.bootstrap();
    ui.boot = boot;
    applyTheme(boot.settings.accent, boot.settings.theme, boot.settings.uiMode, boot.settings.gradients);
  } catch (error) {
    notify(errorText(error), "error");
  }
}

export async function refreshHorizon() {
  if (!ui.boot) return;
  ui.busy = true;
  try {
    const horizon = await api.refreshHorizon();
    ui.boot.horizon = horizon;
    const status = await api.gameStatus();
    if (ui.boot.activeProfile) {
      ui.boot.activeProfile.lastExitCode = status.lastExitCode;
      ui.boot.activeProfile.lastPlayed = status.lastPlayed;
    }
  } catch (error) {
    notify(errorText(error), "error");
  } finally {
    ui.busy = false;
  }
}

export async function saveSettings(patch: Partial<Settings>) {
  if (!ui.boot) return;
  const next: Settings = { ...ui.boot.settings, ...patch };
  ui.boot.settings = next;
  applyTheme(next.accent, next.theme, next.uiMode, next.gradients);
  try {
    const saved = await api.saveSettings(next);
    ui.boot.settings = saved;
  } catch (error) {
    notify(errorText(error), "error");
    await reload();
  }
}

export async function activateProfile(id: string) {
  if (!ui.boot) return;
  if (ui.boot.activeProfile?.id === id) return;
  try {
    await api.activateProfile(id);
    await reload();
  } catch (error) {
    notify(errorText(error), "error");
  }
}
