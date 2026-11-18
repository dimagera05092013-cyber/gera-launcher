import { invoke } from "@tauri-apps/api/core";
import type {
  Bootstrap,
  BuiltinMod,
  ClientRelease,
  Diagnostics,
  GameStatus,
  HorizonReport,
  ImportScan,
  ImportedResources,
  InstalledMod,
  LaunchResult,
  ModProject,
  NativeBuild,
  NativeCheck,
  NativeJob,
  NativeStatus,
  Profile,
  ServerDraft,
  ServerEntry,
  ServerImages,
  ServerImportItem,
  ServerImportScan,
  ServerStatus,
  Settings,
  UpdateInfo,
  JavaVersionInfo,
  JavaLaunch,
  JavaCheck,
  LogsInfo,
  ResourceCheck,
} from "./types";

export const api = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  refreshHorizon: () => invoke<HorizonReport>("refresh_horizon"),
  gameStatus: () => invoke<GameStatus>("game_status"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  saveProfile: (profile: Profile) => invoke<Profile>("save_profile", { profile }),
  duplicateProfile: (id: string, name: string) =>
    invoke<Profile>("duplicate_profile", { id, name }),
  removeProfile: (id: string) => invoke<Profile[]>("remove_profile", { id }),
  activateProfile: (id: string) => invoke<void>("activate_profile", { id }),
  launchGame: (profileId: string) => invoke<LaunchResult>("launch_game", { profileId }),
  launchServer: (serverId: string) => invoke<LaunchResult>("launch_server", { serverId }),
  serverSave: (draft: ServerDraft) => invoke<ServerEntry>("server_save", { draft }),
  serverRemove: (id: string) => invoke<void>("server_remove", { id }),
  serverSetImage: (id: string, kind: "banner" | "icon", dataUrl: string) =>
    invoke<void>("server_set_image", { id, kind, dataUrl }),
  serverClearImage: (id: string, kind: "banner" | "icon") =>
    invoke<void>("server_clear_image", { id, kind }),
  serverImages: (id: string) => invoke<ServerImages>("server_images", { id }),
  serverImportScan: () => invoke<ServerImportScan>("server_import_scan"),
  serverImportAdd: (items: ServerImportItem[]) =>
    invoke<ServerEntry[]>("server_import_add", { items }),
  serverReorder: (ids: string[]) => invoke<ServerEntry[]>("server_reorder", { ids }),
  serverPin: (id: string, pinned: boolean) =>
    invoke<ServerEntry>("server_pin", { id, pinned }),
  serverSetProfile: (id: string, profileId: string | null) =>
    invoke<ServerEntry>("server_set_profile", { id, profileId }),
  serverStatus: (host: string, port: number) =>
    invoke<ServerStatus>("server_status", { host, port }),
  readGameLog: (profileId: string | null, lines = 80) =>
    invoke<string | null>("read_game_log", { profileId, lines }),
  diagnostics: () => invoke<Diagnostics>("diagnostics"),
  horizonReleases: () => invoke<ClientRelease[]>("horizon_releases"),
  horizonInstall: (tag: string) => invoke<string>("horizon_install", { tag }),
  horizonImport: (folder: string, source: string) =>
    invoke<ImportedResources>("horizon_import", { folder, source }),
  importFolder: () => invoke<string>("import_folder"),
  importScan: (target: string | null = null, source: string | null = null) =>
    invoke<ImportScan>("import_scan", { target, source }),
  horizonPrepare: (folder: string, minecraftDir: string, serverJar: string | null) =>
    invoke<string>("horizon_prepare", { folder, minecraftDir, serverJar }),
  modsCatalog: (force = false) => invoke<ModProject[]>("mods_catalog", { force }),
  modsInstalled: (profileId: string | null) =>
    invoke<[string, InstalledMod[]]>("mods_installed", { profileId }),
  modsBuiltin: (profileId: string | null) =>
    invoke<BuiltinMod[]>("mods_builtin", { profileId }),
  modsInstall: (profileId: string | null, projectId: string, version: string | null = null) =>
    invoke<InstalledMod>("mods_install", { profileId, projectId, version }),
  modsRemove: (profileId: string | null, file: string) =>
    invoke<InstalledMod[]>("mods_remove", { profileId, file }),
  nativeStatus: () => invoke<NativeStatus>("native_status"),
  nativeCheck: (profileId: string, projectId: string, version: string | null = null) =>
    invoke<NativeCheck>("native_check", { profileId, projectId, version }),
  nativeBuild: (
    profileId: string,
    projectId: string,
    version: string | null = null,
    allowMismatch = false,
  ) => invoke<void>("native_build", { profileId, projectId, version, allowMismatch }),
  nativeCancel: () => invoke<NativeJob>("native_cancel"),
  nativeLog: () => invoke<NativeJob>("native_log"),
  nativeBuilds: () => invoke<NativeBuild[]>("native_builds"),
  nativeRemove: (folder: string) => invoke<NativeBuild[]>("native_remove", { folder }),
  nativeUse: (profileId: string, folder: string) =>
    invoke<Profile>("native_use", { profileId, folder }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  updateCheck: () => invoke<UpdateInfo | null>("update_check"),
  updateDownload: (version: string) => invoke<void>("update_download", { version }),
  updateInstall: () => invoke<void>("update_install"),
  javaVersions: () => invoke<JavaVersionInfo[]>("java_versions"),
  javaInstall: (id: string) => invoke<void>("java_install", { id }),
  javaLaunch: (id: string, player: string) => invoke<JavaLaunch>("java_launch", { id, player }),
  javaFolder: () => invoke<string>("java_folder"),
  javaCheck: (id: string) => invoke<JavaCheck>("java_check", { id }),
  javaCancel: () => invoke<void>("java_cancel"),
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  openLastLog: () => invoke<void>("open_last_log"),
  logsInfo: () => invoke<LogsInfo>("logs_info"),
  horizonCheck: (folder: string) => invoke<ResourceCheck>("horizon_check", { folder }),
};

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
