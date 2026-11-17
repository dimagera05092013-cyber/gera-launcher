export interface Settings {
  uiMode: "light" | "heavy";
  theme: "system" | "light" | "dark";
  accent: string;
  gradients: boolean;
  customHorizonDir: string | null;
  minecraftDir: string | null;
  serverJar: string | null;
  updateManifest: string;
  checkModUpdates: boolean;
  closeOnLaunch: boolean;
}

export interface Profile {
  id: string;
  name: string;
  description: string;
  playerName: string;
  gameDir: string;
  horizonVersion: string | null;
  skin: string | null;
  detected: boolean;
  createdAt: number;
  lastPlayed: number | null;
  lastExitCode: number | null;
}

export interface HorizonVersion {
  tag: string;
  dir: string;
  client: string;
  resources: string;
  ready: boolean;
  source: "gera" | "legacy" | "custom";
  revision: string | null;
  minecraft: string;
}

export interface ClientRelease {
  tag: string;
  folder: string;
  name: string;
  published: string;
  nightly: boolean;
  size: number;
  installed: boolean;
  asset: string;
}

export interface ImportedResources {
  folder: string;
  minecraft: string;
  path: string;
  bytes: number;
}

export interface ImportScan {
  folder: string;
  imported: ImportedResources | null;
}

export interface ModFileInfo {
  name: string;
  url: string;
  size: number;
  sha256: string;
}

export interface ModVersion {
  version: string;
  published: string;
  changelog: string;
  api: string;
  game: string;
  file: ModFileInfo;
}

export interface ModProject {
  id: string;
  kind: string;
  name: string;
  summary: string;
  description: string;
  authors: string[];
  icon: string;
  screenshots: string[];
  categories: string[];
  downloads: number;
  summaryRu: string;
  descriptionRu: string;
  versions: ModVersion[];
}

export interface InstalledMod {
  file: string;
  projectId: string | null;
  name: string | null;
  version: string | null;
  sha256: string | null;
  installedAt: number;
}

export interface BuiltinMod {
  id: string;
  title: string;
  enabled: boolean;
}

export interface RunningView {
  pid: number;
  gameDir: string;
  player: string | null;
  startedAt: number | null;
  profileId: string | null;
}

export interface HorizonReport {
  versions: HorizonVersion[];
  defaultGameDir: string;
  running: RunningView | null;
}

export interface AppInfo {
  version: string;
  target: string;
  profile: string;
}

export interface Bootstrap {
  version: string;
  dataDir: string;
  settings: Settings;
  profiles: Profile[];
  servers: ServerEntry[];
  activeProfile: Profile | null;
  horizon: HorizonReport;
  appInfo: AppInfo;
  recoveredFrom: string | null;
}

export interface LaunchResult {
  pid: number;
  logPath: string;
}

export interface GameStatus {
  running: RunningView | null;
  lastExitCode: number | null;
  lastPlayed: number | null;
}

export interface DiagItem {
  label: string;
  status: "ok" | "warn" | "error" | "info";
  detail: string;
}

export interface LaunchApiInfo {
  launchApi: string;
  horizon: string;
  minecraft: string;
  options: string[];
}

export interface Diagnostics {
  ok: boolean;
  items: DiagItem[];
  launchApi: LaunchApiInfo | null;
}

export interface GameExited {
  profileId: string;
  code: number | null;
  logPath: string;
}

export interface NativeJob {
  running: boolean;
  step: number;
  total: number;
  stage: string;
  detail: string;
  log: string[];
  error: string | null;
  folder: string | null;
  project: string | null;
  compileDone: number | null;
  compileTotal: number | null;
  startedAt: number;
  finishedAt: number;
}

export interface NativeBuild {
  folder: string;
  projectId: string;
  name: string;
  version: string;
  horizon: string;
  mixins: string;
  base: string;
  game: string;
  builtAt: number;
}

export interface NativeStatus {
  rustc: string | null;
  cargo: string | null;
  unzip: boolean;
  diskFreeGb: number;
  workDir: string;
  job: NativeJob;
  builds: NativeBuild[];
}

export interface NativeCheck {
  baseTag: string;
  baseRevision: string | null;
  horizon: string;
  mixins: string;
  matchCommit: boolean;
  game: string;
  built: string | null;
  traitApi: boolean;
}

export interface ServerEntry {
  id: string;
  name: string;
  host: string;
  port: number;
  description: string;
  banner: string | null;
  icon: string | null;
  fallbackText: string;
  fallbackColor: string;
  profileId: string | null;
  pinned: boolean;
  position: number;
  createdAt: number;
  lastPlayed: number | null;
}

export interface ServerImages {
  banner: string | null;
  icon: string | null;
}

export interface ServerStatus {
  online: boolean;
  onlinePlayers: number | null;
  maxPlayers: number | null;
  motd: string | null;
  version: string | null;
  latencyMs: number | null;
  favicon: string | null;
  error: string | null;
}

export interface ImportedServer {
  name: string;
  host: string;
  port: number;
  icon: string | null;
  alreadyAdded: boolean;
}

export interface ServerImportScan {
  source: string | null;
  servers: ImportedServer[];
  extraSources: string[];
  error: string | null;
}

export interface ServerDraft {
  id: string | null;
  name: string;
  host: string;
  port: number;
  description: string;
  profileId: string | null;
  fallbackText: string;
  fallbackColor: string;
  banner: string | null;
  icon: string | null;
  clearBanner: boolean;
  clearIcon: boolean;
}

export interface ServerImportItem {
  name: string;
  host: string;
  port: number;
  icon: string | null;
  profileId: string | null;
}

export interface UpdateInfo {
  version: string;
  current: string;
  notes: string;
  url: string;
  asset: string;
  size: number;
}

export interface UpdateProgress {
  downloaded: number;
  total: number;
}
