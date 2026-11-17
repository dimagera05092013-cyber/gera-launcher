import { listen } from "@tauri-apps/api/event";
import { api, errorText } from "./api";
import { notify } from "./store.svelte";
import type { UpdateInfo, UpdateProgress } from "./types";

export type UpdatePhase = "idle" | "downloading" | "installing" | "error";

export const updater = $state({
  dialog: false,
  info: null as UpdateInfo | null,
  checking: false,
  phase: "idle" as UpdatePhase,
  downloaded: 0,
  total: 0,
  error: "",
});

let listening = false;

async function listenProgress() {
  if (listening) return;
  listening = true;
  await listen<UpdateProgress>("update-progress", (event) => {
    updater.downloaded = event.payload.downloaded;
    updater.total = event.payload.total || updater.total;
  });
}

export async function checkUpdates(manual = false) {
  if (updater.checking || updater.phase === "downloading" || updater.phase === "installing") {
    return;
  }
  updater.checking = true;
  try {
    const info = await api.updateCheck();
    if (info) {
      updater.info = info;
      updater.phase = "idle";
      updater.error = "";
      updater.downloaded = 0;
      updater.total = info.size;
      updater.dialog = true;
    } else if (manual) {
      notify("Установлена последняя версия GERA.", "success");
    }
  } catch (caught) {
    if (manual) notify(errorText(caught), "error");
  } finally {
    updater.checking = false;
  }
}

export async function startUpdate() {
  if (!updater.info) return;
  if (updater.phase === "downloading" || updater.phase === "installing") return;
  updater.phase = "downloading";
  updater.error = "";
  updater.downloaded = 0;
  try {
    await listenProgress();
    await api.updateDownload(updater.info.version);
    updater.phase = "installing";
    // The backend replaces the exe and restarts GERA, so this call may never
    // return its result.
    await api.updateInstall();
  } catch (caught) {
    updater.phase = "error";
    updater.error = errorText(caught);
  }
}

export function postponeUpdate() {
  if (updater.phase === "downloading" || updater.phase === "installing") return;
  updater.dialog = false;
}
