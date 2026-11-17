use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::horizon::{self, HorizonVersion, RunningGame};
use crate::paths::Paths;
use crate::servers::{self, ServerImages, ServerImportScan, ServerStatus};
use crate::store::{self, now_ms, Profile, ServerEntry, Settings, Store};

#[derive(Debug, Clone)]
pub struct TrackedGame {
    pub pid: u32,
    pub profile_id: String,
    pub started_at: u64,
    pub log_path: String,
}

pub struct AppState {
    pub paths: Paths,
    pub store: Mutex<Store>,
    pub running: Mutex<Option<TrackedGame>>,
}

impl AppState {
    pub fn new(paths: Paths, store: Store) -> Self {
        Self {
            paths,
            store: Mutex::new(store),
            running: Mutex::new(None),
        }
    }
}

pub fn store_lock(state: &AppState) -> MutexGuard<'_, Store> {
    state.store.lock().unwrap_or_else(|err| err.into_inner())
}

fn running_lock(state: &AppState) -> MutexGuard<'_, Option<TrackedGame>> {
    state.running.lock().unwrap_or_else(|err| err.into_inner())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningView {
    pub pid: u32,
    pub game_dir: String,
    pub player: Option<String>,
    pub started_at: Option<u64>,
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HorizonReport {
    pub versions: Vec<HorizonVersion>,
    pub default_game_dir: String,
    pub running: Option<RunningView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub target: String,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub version: String,
    pub data_dir: String,
    pub settings: Settings,
    pub profiles: Vec<Profile>,
    pub servers: Vec<ServerEntry>,
    pub active_profile: Option<Profile>,
    pub horizon: HorizonReport,
    pub app_info: AppInfo,
    pub recovered_from: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub pid: u32,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportScan {
    pub folder: String,
    pub imported: Option<crate::clients::ImportedResources>,
}

/// Where a player drops a prepared resource folder brought from another PC.
#[tauri::command]
pub fn import_folder() -> String {
    crate::clients::drop_dir().to_string_lossy().into_owned()
}

/// Import the drop folder into the active profile's client. Nothing to import
/// is not an error: the player simply has resources already.
#[tauri::command]
pub fn import_scan(
    state: State<'_, AppState>,
    target: Option<String>,
    source: Option<String>,
) -> Result<ImportScan, String> {
    let folder = crate::clients::drop_dir().to_string_lossy().into_owned();
    let explicit = target.filter(|value| !value.trim().is_empty());
    let source = source.filter(|value| !value.trim().is_empty());
    let target = match explicit {
        Some(dir) => dir,
        None => {
            let (profile, settings) = {
                let store = store_lock(&state);
                let profile = store
                    .active_profile
                    .as_ref()
                    .and_then(|id| store.profile(id))
                    .cloned();
                (profile, store.settings.clone())
            };
            let discovery = horizon::discover(settings.custom_horizon_dir.as_deref());
            let chosen = match profile {
                Some(profile) => horizon::resolve(&profile, &discovery).cloned(),
                None => discovery
                    .versions
                    .iter()
                    .find(|version| version.source == "gera" && !version.ready)
                    .cloned(),
            };
            match chosen {
                Some(version) if !version.ready => version.dir,
                Some(version) if source.is_some() => version.dir,
                Some(_) => return Ok(ImportScan { folder, imported: None }),
                None => {
                    return Err(
                        "Нет сборки GERA. Сначала установите клиент Horizon.".into(),
                    )
                }
            }
        }
    };
    let imported = match source {
        Some(source) => Some(crate::clients::import_resources(&target, &source)?),
        None => crate::clients::import_from_drop(&target)?,
    };
    Ok(ImportScan { folder, imported })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub running: Option<RunningView>,
    pub last_exit_code: Option<i32>,
    pub last_played: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GameExited {
    profile_id: String,
    code: Option<i32>,
    log_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagItem {
    pub label: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub ok: bool,
    pub items: Vec<DiagItem>,
    pub launch_api: Option<horizon::LaunchApiInfo>,
}

fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        target: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        profile: if cfg!(debug_assertions) { "debug" } else { "release" }.into(),
    }
}

fn report(custom_dir: Option<&str>) -> HorizonReport {
    let discovery = horizon::discover(custom_dir);
    HorizonReport {
        versions: discovery.versions,
        default_game_dir: crate::paths::horizon_default_game_dir()
            .to_string_lossy()
            .into_owned(),
        running: None,
    }
}

fn running_view(state: &AppState) -> Option<RunningView> {
    let tracked = running_lock(state).clone();
    if let Some(tracked) = tracked {
        if horizon::process_alive(tracked.pid) {
            let game = horizon::running_games()
                .into_iter()
                .find(|g| g.pid == tracked.pid);
            return Some(RunningView {
                pid: tracked.pid,
                game_dir: game
                    .as_ref()
                    .map(|g| g.game_dir.clone())
                    .unwrap_or_default(),
                player: game.and_then(|g| g.player),
                started_at: Some(tracked.started_at),
                profile_id: Some(tracked.profile_id),
            });
        }
    }

    let profile = store_lock(state).active().cloned()?;
    let wanted = horizon::effective_game_dir(&state.paths, &profile);
    let wanted = fs::canonicalize(&wanted).unwrap_or(wanted);
    horizon::running_games()
        .into_iter()
        .find(|game: &RunningGame| {
            let dir = PathBuf::from(&game.game_dir);
            fs::canonicalize(&dir).unwrap_or(dir) == wanted
        })
        .map(|game| RunningView {
            pid: game.pid,
            game_dir: game.game_dir,
            player: game.player,
            started_at: None,
            profile_id: Some(profile.id),
        })
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap, String> {
    let (settings, profiles, servers, active_profile, recovered_from) = {
        let store = store_lock(&state);
        (
            store.settings.clone(),
            store.profiles.clone(),
            store.servers.clone(),
            store.active().cloned(),
            store.recovered_from.clone(),
        )
    };
    let mut horizon = report(settings.custom_horizon_dir.as_deref());
    horizon.running = running_view(&state);
    Ok(Bootstrap {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: state.paths.data.to_string_lossy().into_owned(),
        settings,
        profiles,
        servers,
        active_profile,
        horizon,
        app_info: app_info(),
        recovered_from,
    })
}

#[tauri::command]
pub fn refresh_horizon(state: State<'_, AppState>) -> Result<HorizonReport, String> {
    let custom = store_lock(&state).settings.custom_horizon_dir.clone();
    let mut horizon = report(custom.as_deref());
    horizon.running = running_view(&state);
    Ok(horizon)
}

#[tauri::command]
pub fn game_status(state: State<'_, AppState>) -> Result<GameStatus, String> {
    let (last_exit_code, last_played) = {
        let store = store_lock(&state);
        match store.active() {
            Some(profile) => (profile.last_exit_code, profile.last_played),
            None => (None, None),
        }
    };
    Ok(GameStatus {
        running: running_view(&state),
        last_exit_code,
        last_played,
    })
}

#[tauri::command]
pub fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    let mut next = settings;
    next.ui_mode = next.ui_mode.to_ascii_lowercase();
    if next.ui_mode != "light" && next.ui_mode != "heavy" {
        return Err("Неизвестный режим интерфейса.".into());
    }
    next.theme = next.theme.to_ascii_lowercase();
    if !matches!(next.theme.as_str(), "system" | "light" | "dark") {
        return Err("Неизвестная тема.".into());
    }
    next.accent = next.accent.trim().to_string();
    if !store::is_hex_color(&next.accent) {
        return Err("Акцентный цвет должен быть в формате #rrggbb.".into());
    }
    next.custom_horizon_dir = next
        .custom_horizon_dir
        .map(|dir| dir.trim().to_string())
        .filter(|dir| !dir.is_empty());
    if let Some(dir) = &next.custom_horizon_dir {
        let path = PathBuf::from(dir);
        if !path.join(crate::sysutil::exe("horizon-client")).is_file() {
            return Err(format!(
                "В папке нет horizon-client: {}. Укажите папку версии Horizon.",
                path.display()
            ));
        }
    }
    next.update_manifest = next.update_manifest.trim().to_string();
    if !next.update_manifest.is_empty() && !next.update_manifest.starts_with("https://") {
        return Err("Адрес канала обновлений должен начинаться с https://".into());
    }

    let mut store = store_lock(&state);
    store.settings = next.clone();
    store::save(&state.paths, &store).map_err(|e| format!("Не удалось сохранить настройки: {e}"))?;
    drop(store);
    crate::apply_window_theme(&app, &next.theme);
    Ok(next)
}

fn unique_profile_id(store: &Store, name: &str) -> String {
    let mut base: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    base = base.trim_matches('-').to_string();
    if base.is_empty() {
        base = "profile".into();
    }
    base.truncate(24);
    let mut candidate = format!("{base}-{}", now_ms() % 100_000);
    while store.profiles.iter().any(|p| p.id == candidate) {
        candidate.push('x');
    }
    candidate
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let ca = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let cb = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}

#[tauri::command]
pub fn save_profile(state: State<'_, AppState>, mut profile: Profile) -> Result<Profile, String> {
    let mut store = store_lock(&state);
    let creating = profile.id.trim().is_empty();
    let existing = if creating {
        None
    } else {
        store.profile(&profile.id).cloned()
    };
    if !creating && existing.is_none() {
        return Err("Сборка не найдена.".into());
    }

    if creating {
        profile.id = unique_profile_id(&store, &profile.name);
        profile.detected = false;
        profile.created_at = now_ms();
        profile.last_played = None;
        profile.last_exit_code = None;
        if profile.game_dir.trim().is_empty() {
            profile.game_dir = state
                .paths
                .profiles
                .join(&profile.id)
                .join("game")
                .to_string_lossy()
                .into_owned();
        }
    } else if let Some(old) = &existing {
        profile.detected = old.detected;
        profile.created_at = old.created_at;
        profile.last_played = old.last_played;
        profile.last_exit_code = old.last_exit_code;
    }

    store::validate_profile(&profile, existing.as_ref())?;

    let game_dir = PathBuf::from(&profile.game_dir);
    if creating {
        let inside_app = game_dir.starts_with(&state.paths.profiles);
        if !inside_app && !game_dir.is_dir() {
            return Err(format!(
                "Папка не существует: {}. Выберите существующую игровую папку.",
                game_dir.display()
            ));
        }
        if inside_app {
            fs::create_dir_all(&game_dir)
                .map_err(|e| format!("Не удалось создать игровую папку: {e}"))?;
        }
    } else if !game_dir.is_dir() {
        return Err(format!("Папка не существует: {}", game_dir.display()));
    }

    for other in &store.profiles {
        if other.id != profile.id && same_dir(Path::new(&other.game_dir), &game_dir) {
            return Err(format!(
                "Эта папка уже используется сборкой «{}». Сборки должны иметь разные папки.",
                other.name
            ));
        }
    }

    if creating {
        store.profiles.push(profile.clone());
    } else if let Some(slot) = store.profile_mut(&profile.id) {
        *slot = profile.clone();
    }
    if store.active_profile.is_none() {
        store.active_profile = Some(profile.id.clone());
    }
    store::save(&state.paths, &store).map_err(|e| format!("Не удалось сохранить сборку: {e}"))?;
    Ok(profile)
}

/// Copies a build's game folder. Caches, logs and screenshots are skipped:
/// worlds, mods, configs and options are what a build is made of.
fn copy_game_dir(source: &Path, target: &Path) -> Result<u64, String> {
    const SKIP: &[&str] = &[
        "logs",
        "screenshots",
        "downloads",
        "backups",
        ".horizon-cache",
        ".mixin.out",
    ];
    fn walk(from: &Path, to: &Path, skip: &[&str], bytes: &mut u64) -> Result<(), String> {
        fs::create_dir_all(to).map_err(|e| format!("Не удалось создать {}: {e}", to.display()))?;
        let entries = fs::read_dir(from).map_err(|e| format!("Не удалось прочитать {}: {e}", from.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("Не удалось прочитать {}: {e}", from.display()))?;
            let name = entry.file_name();
            let name_text = name.to_string_lossy();
            if skip.iter().any(|s| *s == name_text) {
                continue;
            }
            let src = entry.path();
            let dst = to.join(&name);
            let meta = fs::symlink_metadata(&src)
                .map_err(|e| format!("Не удалось прочитать {}: {e}", src.display()))?;
            if meta.file_type().is_symlink() {
                crate::sysutil::copy_link(&src, &dst)?;
            } else if meta.is_dir() {
                walk(&src, &dst, skip, bytes)?;
            } else {
                fs::copy(&src, &dst)
                    .map_err(|e| format!("Не удалось скопировать {}: {e}", src.display()))?;
                *bytes += meta.len();
            }
        }
        Ok(())
    }
    let mut bytes = 0u64;
    walk(source, target, SKIP, &mut bytes)?;
    Ok(bytes)
}

#[tauri::command]
pub fn duplicate_profile(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<Profile, String> {
    let mut store = store_lock(&state);
    let source = store
        .profile(&id)
        .cloned()
        .ok_or_else(|| "Сборка не найдена.".to_string())?;
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Название сборки: от 1 до 40 символов.".into());
    }

    let new_id = unique_profile_id(&store, &name);
    let game_dir = state.paths.profiles.join(&new_id).join("game");
    let source_dir = PathBuf::from(&source.game_dir);
    if !source_dir.is_dir() {
        return Err(format!(
            "Игровая папка сборки не найдена: {}",
            source_dir.display()
        ));
    }
    let bytes = copy_game_dir(&source_dir, &game_dir)?;

    let profile = Profile {
        id: new_id,
        name,
        description: source.description.clone(),
        player_name: source.player_name.clone(),
        game_dir: game_dir.to_string_lossy().into_owned(),
        horizon_version: source.horizon_version.clone(),
        skin: source.skin.clone(),
        detected: false,
        created_at: now_ms(),
        last_played: None,
        last_exit_code: None,
    };
    store.profiles.push(profile.clone());
    store::save(&state.paths, &store).map_err(|e| format!("Не удалось сохранить: {e}"))?;
    println!(
        "GERA: сборка «{}» скопирована ({} МБ)",
        profile.name,
        bytes / 1024 / 1024
    );
    Ok(profile)
}

#[tauri::command]
pub fn remove_profile(state: State<'_, AppState>, id: String) -> Result<Vec<Profile>, String> {
    let mut store = store_lock(&state);
    let before = store.profiles.len();
    store.profiles.retain(|p| p.id != id);
    if store.profiles.len() == before {
        return Err("Сборка не найдена.".into());
    }
    if store.active_profile.as_deref() == Some(id.as_str()) {
        store.active_profile = store.profiles.first().map(|p| p.id.clone());
    }
    store::save(&state.paths, &store).map_err(|e| format!("Не удалось сохранить: {e}"))?;
    Ok(store.profiles.clone())
}

#[tauri::command]
pub fn activate_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut store = store_lock(&state);
    if store.profile(&id).is_none() {
        return Err("Сборка не найдена.".into());
    }
    store.active_profile = Some(id);
    store::save(&state.paths, &store).map_err(|e| format!("Не удалось сохранить: {e}"))
}

fn validate_launch(profile: &Profile, version: &HorizonVersion) -> Result<(), String> {
    if !version.ready {
        return Err(format!(
            "Ресурсы Horizon не подготовлены. Скопируйте готовую папку ресурсов в {} и нажмите «Играть», либо нажмите «Подготовить» у этой версии в настройках.",
            crate::clients::drop_dir().display()
        ));
    }
    let client = Path::new(&version.client);
    if !client.is_file() {
        return Err(format!("Не найден horizon-client: {}", version.client));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(client)
            .map_err(|e| format!("Не удалось прочитать horizon-client: {e}"))?
            .permissions()
            .mode();
        if mode & 0o111 == 0 {
            return Err(format!("horizon-client не исполняемый: {}", version.client));
        }
    }
    if !store::is_player_name(&profile.player_name) {
        return Err("Исправьте ник сборки перед запуском.".into());
    }
    Ok(())
}

fn start_game(
    app: Option<&AppHandle>,
    state: &AppState,
    profile: &Profile,
    join: Option<String>,
) -> Result<LaunchResult, String> {
    {
        let running = running_lock(state);
        if let Some(tracked) = running.as_ref() {
            if horizon::process_alive(tracked.pid) {
                return Err("Игра уже запущена.".into());
            }
        }
    }

    let settings = store_lock(state).settings.clone();

    let discovery = horizon::discover(settings.custom_horizon_dir.as_deref());
    let mut version = horizon::resolve(&profile, &discovery)
        .cloned()
        .ok_or_else(|| {
            "Клиент Horizon не найден. Установите официальную сборку в настройках GERA."
                .to_string()
        })?;
    // A resource folder dropped into GERA's import folder is picked up here, so
    // on a new PC the player only copies the folder and presses Play.
    if !version.ready {
        if let Ok(Some(imported)) = crate::clients::import_from_drop(&version.dir) {
            version.ready = true;
            version.resources = imported.path;
            version.minecraft = imported.minecraft;
        }
    }
    validate_launch(&profile, &version)?;

    let game_dir = horizon::effective_game_dir(&state.paths, &profile);
    if !game_dir.is_dir() {
        if profile.detected {
            return Err(format!("Игровая папка не найдена: {}", game_dir.display()));
        }
        if game_dir.starts_with(&state.paths.profiles) {
            fs::create_dir_all(&game_dir)
                .map_err(|e| format!("Не удалось создать игровую папку: {e}"))?;
        } else {
            return Err(format!("Игровая папка не найдена: {}", game_dir.display()));
        }
    }
    if !game_dir.is_dir() {
        return Err(format!("Игровая папка не найдена: {}", game_dir.display()));
    }

    state.paths.ensure().map_err(|e| format!("Не удалось создать папку данных: {e}"))?;
    let stamp = now_ms();
    let log_path = state
        .paths
        .logs
        .join(format!("launch-{stamp}-{}.log", profile.id));
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("Не удалось открыть лог запуска: {e}"))?;
    let stderr = log
        .try_clone()
        .map_err(|e| format!("Не удалось подготовить лог запуска: {e}"))?;

    let mut command = Command::new(&version.client);
    command
        .current_dir(&game_dir)
        .arg("--resources")
        .arg(&version.resources)
        .arg("--game-dir")
        .arg(&game_dir)
        .arg("--home")
        .arg(&game_dir)
        .arg("--name")
        .arg(&profile.player_name)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));

    match profile.skin.as_ref().map(PathBuf::from) {
        Some(skin) if skin.is_file() => {
            command.env("HORIZON_OFFLINE_SKIN", &skin);
        }
        _ => {
            command.env_remove("HORIZON_OFFLINE_SKIN");
        }
    }

    // Horizon's documented direct connect: `--server HOST[:PORT]`.
    if let Some(address) = join.as_deref() {
        command.arg("--server").arg(address);
    }

    let child = crate::sysutil::quiet(&mut command)
        .spawn()
        .map_err(|e| format!("Не удалось запустить Horizon: {e}"))?;
    let pid = child.id();
    let started_at = now_ms();

    {
        let mut store = store_lock(&state);
        if let Some(slot) = store.profile_mut(&profile.id) {
            slot.last_played = Some(started_at);
            slot.last_exit_code = None;
        }
        store.active_profile = Some(profile.id.clone());
        let _ = store::save(&state.paths, &store);
    }
    *running_lock(&state) = Some(TrackedGame {
        pid,
        profile_id: profile.id.clone(),
        started_at,
        log_path: log_path.to_string_lossy().into_owned(),
    });

    // Free the launcher's CPU and memory for the game. On a two-core laptop the
    // WebView competes with Horizon's resource loader, so the window goes away
    // as soon as the game process is alive. If the game died at startup, the
    // window comes back so the player can read the error.
    if settings.close_on_launch {
        if let Some(app) = app {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    if horizon::process_alive(pid) {
                        let _ = window.close();
                    } else {
                        let _ = window.show();
                    }
                });
            }
        }
    }

    let Some(app) = app else {
        // Headless callers (tests) do not need the exit notification.
        drop(child);
        return Ok(LaunchResult {
            pid,
            log_path: log_path.to_string_lossy().into_owned(),
        });
    };

    let handle = AppHandle::clone(app);
    let profile_id = profile.id.clone();
    let log_path_string = log_path.to_string_lossy().into_owned();
    std::thread::spawn(move || {
        let mut child = child;
        let status = child.wait();
        let code = status.ok().and_then(|s| s.code());
        {
            let state = handle.state::<AppState>();
            {
                let mut store = store_lock(&state);
                if let Some(slot) = store.profile_mut(&profile_id) {
                    slot.last_exit_code = code;
                }
                let _ = store::save(&state.paths, &store);
            }
            *running_lock(&state) = None;
        }
        let _ = handle.emit(
            "game-exited",
            GameExited {
                profile_id,
                code,
                log_path: log_path_string,
            },
        );
    });

    Ok(LaunchResult {
        pid,
        log_path: log_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn launch_game(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<LaunchResult, String> {
    let profile = {
        let store = store_lock(&state);
        store
            .profile(&profile_id)
            .cloned()
            .ok_or_else(|| "Сборка не найдена.".to_string())?
    };
    start_game(Some(&app), &state, &profile, None)
}

/// Launch the server's bound profile and join it right away. The address is
/// passed through Horizon's own `--server HOST[:PORT]` flag.
#[tauri::command]
pub fn launch_server(
    app: AppHandle,
    state: State<'_, AppState>,
    server_id: String,
) -> Result<LaunchResult, String> {
    launch_server_core(Some(&app), &state, &server_id)
}

fn launch_server_core(
    app: Option<&AppHandle>,
    state: &AppState,
    server_id: &str,
) -> Result<LaunchResult, String> {
    let (server, profile) = {
        let store = store_lock(state);
        let server = store
            .server(server_id)
            .cloned()
            .ok_or_else(|| "Сервер не найден.".to_string())?;
        let profile_id = server
            .profile_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                "К серверу не привязана сборка. Выберите её в настройках сервера.".to_string()
            })?;
        let profile = store.profile(&profile_id).cloned().ok_or_else(|| {
            "Привязанная сборка не найдена. Выберите другую сборку для этого сервера.".to_string()
        })?;
        (server, profile)
    };

    servers::validate(&server.name, &server.host, server.port)?;
    let address = servers::join_address(&server.host, server.port);
    let result = start_game(app, state, &profile, Some(address))?;
    {
        let mut store = store_lock(state);
        if let Some(slot) = store.server_mut(&server.id) {
            slot.last_played = Some(now_ms());
        }
        store.active_profile = Some(profile.id.clone());
        let _ = store::save(&state.paths, &store);
    }
    Ok(result)
}

// ------------------------------------------------------------------ servers

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDraft {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    #[serde(default = "default_server_port")]
    pub port: u16,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub fallback_text: String,
    #[serde(default)]
    pub fallback_color: String,
    /// New picture as a data URL; `None` keeps the current one.
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub clear_banner: bool,
    #[serde(default)]
    pub clear_icon: bool,
}

fn default_server_port() -> u16 {
    servers::DEFAULT_PORT
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerImportItem {
    pub name: String,
    pub host: String,
    #[serde(default = "default_server_port")]
    pub port: u16,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

fn save_servers(state: &AppState, store: &Store) -> Result<(), String> {
    store::save(&state.paths, store).map_err(|e| format!("Не удалось сохранить серверы: {e}"))
}

#[tauri::command]
pub fn server_save(
    state: State<'_, AppState>,
    draft: ServerDraft,
) -> Result<ServerEntry, String> {
    save_server(&state, draft)
}

fn save_server(state: &AppState, draft: ServerDraft) -> Result<ServerEntry, String> {
    let mut store = store_lock(state);

    let name = draft.name.trim().to_string();
    let (raw_host, embedded_port) = servers::split_address(&draft.host);
    let host = servers::normalise_host(&raw_host);
    let port = if embedded_port != servers::DEFAULT_PORT {
        embedded_port
    } else {
        draft.port
    };
    servers::validate(&name, &host, port)?;

    if let Some(profile_id) = draft.profile_id.as_deref() {
        if !profile_id.trim().is_empty() && store.profile(profile_id).is_none() {
            return Err("Выбранная сборка не найдена. Обновите список сборок.".into());
        }
    }

    let id = match draft.id.clone() {
        Some(id) => {
            if store.server(&id).is_none() {
                return Err("Сервер не найден.".into());
            }
            id
        }
        None => servers::next_id(&store),
    };

    if store.servers.iter().any(|s| {
        s.id != id && servers::normalise_host(&s.host) == host && s.port == port
    }) {
        return Err(format!(
            "Сервер {} уже добавлен.",
            servers::display_address(&host, port)
        ));
    }

    let existing = store.server(&id).cloned();
    let mut banner = existing.as_ref().and_then(|s| s.banner.clone());
    let mut icon = existing.as_ref().and_then(|s| s.icon.clone());

    if draft.clear_banner {
        servers::clear_image(&state.paths, &id, "banner");
        banner = None;
    }
    if let Some(data) = draft.banner.as_deref().filter(|value| !value.trim().is_empty()) {
        banner = Some(servers::save_image(&state.paths, &id, "banner", data)?);
    }
    if draft.clear_icon {
        servers::clear_image(&state.paths, &id, "icon");
        icon = None;
    }
    if let Some(data) = draft.icon.as_deref().filter(|value| !value.trim().is_empty()) {
        icon = Some(servers::save_image(&state.paths, &id, "icon", data)?);
    }

    let fallback_text: String = {
        let text: String = draft.fallback_text.trim().chars().take(3).collect();
        if text.is_empty() {
            servers::fallback_letters(&name)
        } else {
            text
        }
    };
    let entry = ServerEntry {
        id: id.clone(),
        name,
        host,
        port,
        description: draft.description.trim().chars().take(500).collect(),
        banner,
        icon,
        fallback_text,
        fallback_color: servers::clean_color(&draft.fallback_color),
        profile_id: draft.profile_id.clone().filter(|value| !value.trim().is_empty()),
        pinned: existing.as_ref().map(|s| s.pinned).unwrap_or(false),
        position: existing
            .as_ref()
            .map(|s| s.position)
            .unwrap_or_else(|| servers::next_position(&store)),
        created_at: existing.as_ref().map(|s| s.created_at).unwrap_or_else(now_ms),
        last_played: existing.as_ref().and_then(|s| s.last_played),
    };

    if let Some(slot) = store.server_mut(&id) {
        *slot = entry.clone();
    } else {
        store.servers.push(entry.clone());
    }
    save_servers(&state, &store)?;
    Ok(entry)
}

#[tauri::command]
pub fn server_remove(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut store = store_lock(&state);
    if store.server(&id).is_none() {
        return Err("Сервер не найден.".into());
    }
    servers::clear_image(&state.paths, &id, "banner");
    servers::clear_image(&state.paths, &id, "icon");
    store.servers.retain(|s| s.id != id);
    save_servers(&state, &store)
}

#[tauri::command]
pub fn server_set_image(
    state: State<'_, AppState>,
    id: String,
    kind: String,
    data_url: String,
) -> Result<(), String> {
    let mut store = store_lock(&state);
    if store.server(&id).is_none() {
        return Err("Сервер не найден.".into());
    }
    let file = servers::save_image(&state.paths, &id, &kind, &data_url)?;
    if let Some(slot) = store.server_mut(&id) {
        match kind.as_str() {
            "banner" => slot.banner = Some(file),
            "icon" => slot.icon = Some(file),
            _ => return Err("Неизвестный тип изображения.".into()),
        }
    }
    save_servers(&state, &store)
}

#[tauri::command]
pub fn server_clear_image(
    state: State<'_, AppState>,
    id: String,
    kind: String,
) -> Result<(), String> {
    let mut store = store_lock(&state);
    if store.server(&id).is_none() {
        return Err("Сервер не найден.".into());
    }
    servers::clear_image(&state.paths, &id, &kind);
    if let Some(slot) = store.server_mut(&id) {
        match kind.as_str() {
            "banner" => slot.banner = None,
            "icon" => slot.icon = None,
            _ => return Err("Неизвестный тип изображения.".into()),
        }
    }
    save_servers(&state, &store)
}

#[tauri::command]
pub fn server_images(
    state: State<'_, AppState>,
    id: String,
) -> Result<ServerImages, String> {
    let store = store_lock(&state);
    let server = store
        .server(&id)
        .ok_or_else(|| "Сервер не найден.".to_string())?;
    Ok(servers::images_for(&state.paths, server))
}

#[tauri::command]
pub fn server_import_scan(state: State<'_, AppState>) -> Result<ServerImportScan, String> {
    let store = store_lock(&state);
    Ok(servers::scan_import(&store))
}

#[tauri::command]
pub fn server_import_add(
    state: State<'_, AppState>,
    items: Vec<ServerImportItem>,
) -> Result<Vec<ServerEntry>, String> {
    import_servers(&state, items)
}

fn import_servers(
    state: &AppState,
    items: Vec<ServerImportItem>,
) -> Result<Vec<ServerEntry>, String> {
    let mut store = store_lock(state);
    let mut added = Vec::new();
    for item in items {
        let (raw_host, embedded_port) = servers::split_address(&item.host);
        let host = servers::normalise_host(&raw_host);
        let port = if embedded_port != servers::DEFAULT_PORT {
            embedded_port
        } else {
            item.port
        };
        if servers::validate(&item.name, &host, port).is_err() {
            continue;
        }
        if store.servers.iter().any(|s| {
            servers::normalise_host(&s.host) == host && s.port == port
        }) {
            continue;
        }
        if let Some(profile_id) = item.profile_id.as_deref() {
            if !profile_id.trim().is_empty() && store.profile(profile_id).is_none() {
                continue;
            }
        }
        let id = servers::next_id(&store);
        let name: String = item.name.trim().chars().take(60).collect();
        let mut entry = ServerEntry {
            id: id.clone(),
            name: name.clone(),
            host,
            port,
            description: String::new(),
            banner: None,
            icon: None,
            fallback_text: servers::fallback_letters(&name),
            fallback_color: "#6750a4".into(),
            profile_id: item.profile_id.clone().filter(|value| !value.trim().is_empty()),
            pinned: false,
            position: servers::next_position(&store),
            created_at: now_ms(),
            last_played: None,
        };
        if let Some(data) = item.icon.as_deref().filter(|value| value.starts_with("data:")) {
            if let Ok(file) = servers::save_image(&state.paths, &id, "icon", data) {
                entry.icon = Some(file);
            }
        }
        store.servers.push(entry.clone());
        added.push(entry);
    }
    save_servers(&state, &store)?;
    Ok(added)
}

#[tauri::command]
pub fn server_reorder(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<ServerEntry>, String> {
    reorder_servers(&state, ids)
}

fn reorder_servers(state: &AppState, ids: Vec<String>) -> Result<Vec<ServerEntry>, String> {
    let mut store = store_lock(state);
    for (index, id) in ids.iter().enumerate() {
        if let Some(slot) = store.server_mut(id) {
            slot.position = index as u32;
        }
    }
    let mut tail = ids.len() as u32;
    for slot in store.servers.iter_mut() {
        if !ids.iter().any(|id| id == &slot.id) {
            slot.position = tail;
            tail += 1;
        }
    }
    save_servers(&state, &store)?;
    Ok(store.servers.clone())
}

#[tauri::command]
pub fn server_pin(
    state: State<'_, AppState>,
    id: String,
    pinned: bool,
) -> Result<ServerEntry, String> {
    pin_server(&state, &id, pinned)
}

fn pin_server(state: &AppState, id: &str, pinned: bool) -> Result<ServerEntry, String> {
    let mut store = store_lock(state);
    let slot = store
        .server_mut(id)
        .ok_or_else(|| "Сервер не найден.".to_string())?;
    slot.pinned = pinned;
    let entry = slot.clone();
    save_servers(state, &store)?;
    Ok(entry)
}

#[tauri::command]
pub fn server_set_profile(
    state: State<'_, AppState>,
    id: String,
    profile_id: Option<String>,
) -> Result<ServerEntry, String> {
    let mut store = store_lock(&state);
    if let Some(profile_id) = profile_id.as_deref() {
        if !profile_id.trim().is_empty() && store.profile(profile_id).is_none() {
            return Err("Сборка не найдена.".into());
        }
    }
    let slot = store
        .server_mut(&id)
        .ok_or_else(|| "Сервер не найден.".to_string())?;
    slot.profile_id = profile_id.filter(|value| !value.trim().is_empty());
    let entry = slot.clone();
    save_servers(&state, &store)?;
    Ok(entry)
}

/// Server List Ping in a blocking thread; the UI never waits on the socket.
#[tauri::command]
pub async fn server_status(host: String, port: u16) -> Result<ServerStatus, String> {
    tauri::async_runtime::spawn_blocking(move || servers::ping(&host, port))
        .await
        .map_err(|e| format!("Проверка сервера не выполнена: {e}"))
}

#[tauri::command]
pub fn read_game_log(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    lines: Option<usize>,
) -> Result<Option<String>, String> {
    let store = store_lock(&state);
    let profile = match profile_id {
        Some(id) => store.profile(&id).cloned(),
        None => store.active().cloned(),
    }
    .ok_or_else(|| "Сборка не найдена.".to_string())?;
    let game_dir = horizon::effective_game_dir(&state.paths, &profile);
    let count = lines.unwrap_or(80).clamp(1, 400);
    Ok(horizon::tail_log(&game_dir, count))
}

#[tauri::command]
pub fn diagnostics(state: State<'_, AppState>) -> Result<Diagnostics, String> {
    let store = store_lock(&state);
    let discovery = horizon::discover(store.settings.custom_horizon_dir.as_deref());
    let mut items = Vec::new();

    let ready = discovery.versions.iter().filter(|v| v.ready).count();
    items.push(DiagItem {
        label: "Horizon".into(),
        status: if discovery.versions.is_empty() { "error" } else { "ok" }.into(),
        detail: if discovery.versions.is_empty() {
            "Установленные версии не найдены.".into()
        } else {
            format!(
                "Найдено версий: {}, с готовыми ресурсами: {}.",
                discovery.versions.len(),
                ready
            )
        },
    });

    items.push(DiagItem {
        label: "Ресурсы игры".into(),
        status: if ready > 0 { "ok" } else { "error" }.into(),
        detail: if ready > 0 {
            "Ресурсы подготовлены: игру можно запускать.".into()
        } else {
            "Нет версии с готовыми ресурсами. Подготовьте их в настройках GERA."
                .into()
        },
    });

    if let Some(profile) = store.active() {
        let game_dir = horizon::effective_game_dir(&state.paths, profile);
        let exists = game_dir.is_dir();
        items.push(DiagItem {
            label: "Игровая папка".into(),
            status: if exists { "ok" } else { "error" }.into(),
            detail: if exists {
                game_dir.to_string_lossy().into_owned()
            } else {
                format!("Не найдена: {}", game_dir.display())
            },
        });
        items.push(DiagItem {
            label: "Ник сборки".into(),
            status: if store::is_player_name(&profile.player_name) { "ok" } else { "error" }.into(),
            detail: profile.player_name.clone(),
        });
    } else {
        items.push(DiagItem {
            label: "Сборка".into(),
            status: "error".into(),
            detail: "Активная сборка не выбрана.".into(),
        });
    }

    let probe = state.paths.data.join(".write-probe");
    let writable = fs::write(&probe, b"ok").is_ok();
    if writable {
        let _ = fs::remove_file(&probe);
    }
    items.push(DiagItem {
        label: "Данные GERA".into(),
        status: if writable { "ok" } else { "error" }.into(),
        detail: format!(
            "{} — {}",
            state.paths.data.display(),
            if writable { "запись доступна" } else { "нет записи" }
        ),
    });

    let gera_clients = discovery
        .versions
        .iter()
        .filter(|v| v.source == "gera")
        .count();
    let legacy_clients = discovery
        .versions
        .iter()
        .filter(|v| v.source == "legacy")
        .count();
    items.push(DiagItem {
        label: "Клиенты GERA".into(),
        status: if gera_clients > 0 { "ok" } else { "warn" }.into(),
        detail: if gera_clients > 0 {
            format!(
                "{gera_clients} установлено в GERA{}",
                if legacy_clients > 0 {
                    format!(", {legacy_clients} найдено на диске")
                } else {
                    String::new()
                }
            )
        } else {
            "Своих клиентов пока нет. Можно установить официальную сборку Horizon."
                .into()
        },
    });

    if let Some(backup) = &store.recovered_from {
        items.push(DiagItem {
            label: "Файл настроек".into(),
            status: "warn".into(),
            detail: format!("Повреждённый state.json сохранён: {backup}"),
        });
    }

    let launch_api = discovery
        .versions
        .iter()
        .find(|v| v.ready)
        .and_then(|v| horizon::launch_api(Path::new(&v.client)));

    let ok = items.iter().all(|item| item.status != "error");
    Ok(Diagnostics {
        ok,
        items,
        launch_api,
    })
}

fn spawn_open(target: &str) -> Result<(), String> {
    crate::sysutil::open_target(target)
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return Err("Путь должен быть абсолютным.".into());
    }
    if !path.exists() {
        return Err(format!("Путь не найден: {}", path.display()));
    }
    spawn_open(&path.to_string_lossy())
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    let url = url.trim();
    if !url.starts_with("https://") {
        return Err("Разрешены только https-ссылки.".into());
    }
    spawn_open(url)
}

#[tauri::command]
pub async fn horizon_releases() -> Result<Vec<crate::clients::ClientRelease>, String> {
    tauri::async_runtime::spawn_blocking(crate::clients::list_releases)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn horizon_install(tag: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || crate::clients::install(&tag))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn horizon_import(
    folder: String,
    source: String,
) -> Result<crate::clients::ImportedResources, String> {
    tauri::async_runtime::spawn_blocking(move || crate::clients::import_resources(&folder, &source))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn horizon_prepare(
    folder: String,
    minecraft_dir: String,
    server_jar: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::clients::prepare(&folder, &minecraft_dir, server_jar.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mods_catalog(force: bool) -> Result<Vec<crate::mods::ModProject>, String> {
    tauri::async_runtime::spawn_blocking(move || crate::mods::catalog(force))
        .await
        .map_err(|e| e.to_string())?
}

fn game_dir_for(state: &AppState, profile_id: Option<&str>) -> Result<(String, PathBuf), String> {
    let store = store_lock(state);
    let profile = match profile_id {
        Some(id) => store.profile(id),
        None => store.active(),
    }
    .ok_or_else(|| "Сборка не найдена.".to_string())?;
    let path = horizon::effective_game_dir(&state.paths, profile);
    Ok((profile.id.clone(), path))
}

#[tauri::command]
pub fn mods_installed(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<(String, Vec<crate::mods::InstalledMod>), String> {
    let (id, game_dir) = game_dir_for(&state, profile_id.as_deref())?;
    Ok((id.clone(), crate::mods::installed(&id, &game_dir)))
}

#[tauri::command]
pub fn mods_builtin(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<Vec<crate::mods::BuiltinMod>, String> {
    let (_, game_dir) = game_dir_for(&state, profile_id.as_deref())?;
    Ok(crate::mods::builtin(&game_dir))
}

#[tauri::command]
pub async fn mods_install(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    project_id: String,
    version: Option<String>,
) -> Result<crate::mods::InstalledMod, String> {
    let (id, game_dir) = game_dir_for(&state, profile_id.as_deref())?;
    drop(state);
    tauri::async_runtime::spawn_blocking(move || {
        crate::mods::install(&id, &game_dir, &project_id, version.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn mods_remove(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    file: String,
) -> Result<Vec<crate::mods::InstalledMod>, String> {
    let (id, game_dir) = game_dir_for(&state, profile_id.as_deref())?;
    crate::mods::remove(&id, &game_dir, &file)?;
    Ok(crate::mods::installed(&id, &game_dir))
}



/// Point a profile at a native build GERA compiled.
#[tauri::command]
pub fn native_use(
    state: State<'_, AppState>,
    profile_id: String,
    folder: String,
) -> Result<Profile, String> {
    if !folder.starts_with("native-") || folder.contains('/') || folder.contains("..") {
        return Err("Это не папка нативной сборки GERA.".into());
    }
    let dir = crate::paths::clients_dir().join(&folder);
    if !dir.join(crate::sysutil::exe("horizon-client")).is_file()
        || !dir.join("native-build.json").is_file()
    {
        return Err("Сборка не найдена.".into());
    }
    let mut store = store_lock(&state);
    let index = store
        .profiles
        .iter()
        .position(|profile| profile.id == profile_id)
        .ok_or_else(|| "Сборка не найдена.".to_string())?;
    store.profiles[index].horizon_version = Some(folder);
    let profile = store.profiles[index].clone();
    store::save(&state.paths, &store).map_err(|e| format!("Не удалось сохранить сборку: {e}"))?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> (PathBuf, Paths) {
        let root = std::env::temp_dir().join(format!("gera-commands-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = Paths {
            data: root.join("data"),
            logs: root.join("data/logs"),
            profiles: root.join("data/profiles"),
            cache: root.join("data/cache"),
        };
        paths.ensure().unwrap();
        (root, paths)
    }

    fn draft(name: &str, host: &str, port: u16) -> ServerDraft {
        ServerDraft {
            id: None,
            name: name.into(),
            host: host.into(),
            port,
            description: String::new(),
            profile_id: None,
            fallback_text: String::new(),
            fallback_color: "#6750A4".into(),
            banner: None,
            icon: None,
            clear_banner: false,
            clear_icon: false,
        }
    }

    #[test]
    fn saves_imports_deduplicates_and_reorders_servers() {
        let (root, paths) = temp_paths("records");
        let state = AppState::new(paths, Store::default());

        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        let mut first = draft("Anarchy", "Play.Example.com", 25565);
        first.banner = Some(format!("data:image/png;base64,{}", servers::b64_encode(&png)));
        let saved = save_server(&state, first.clone()).unwrap();
        assert_eq!(saved.host, "play.example.com");
        assert_eq!(saved.fallback_text, "AN");
        let banner = saved.banner.clone().expect("banner saved");
        assert!(state.paths.data.join("servers").join(banner).is_file());

        // The same address must not be added twice.
        let error = save_server(&state, first.clone()).unwrap_err();
        assert!(error.contains("уже добавлен"), "{error}");

        // Editing keeps the picture and lets the name change.
        let mut edit = first.clone();
        edit.id = Some(saved.id.clone());
        edit.name = "Anarchy 2".into();
        let edited = save_server(&state, edit).unwrap();
        assert_eq!(edited.name, "Anarchy 2");
        assert_eq!(edited.banner, saved.banner);

        // Import skips the known address and adds the new one.
        let added = import_servers(
            &state,
            vec![
                ServerImportItem {
                    name: "Anarchy".into(),
                    host: "play.example.com".into(),
                    port: 25565,
                    icon: None,
                    profile_id: None,
                },
                ServerImportItem {
                    name: "New".into(),
                    host: "new.example.com".into(),
                    port: 25566,
                    icon: Some(format!("data:image/png;base64,{}", servers::b64_encode(&png))),
                    profile_id: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].host, "new.example.com");
        assert!(added[0].icon.is_some());

        let list = reorder_servers(&state, vec![added[0].id.clone(), saved.id.clone()]).unwrap();
        let by_id = |id: &str| list.iter().find(|s| s.id == id).unwrap().position;
        assert_eq!(by_id(&added[0].id), 0);
        assert_eq!(by_id(&saved.id), 1);

        let pinned = pin_server(&state, &saved.id, true).unwrap();
        assert!(pinned.pinned);

        let loaded = store::load_or_init(&state.paths).unwrap();
        assert_eq!(loaded.servers.len(), 2);
        assert!(loaded.servers.iter().any(|s| s.id == saved.id && s.pinned));

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn launches_the_bound_profile_with_the_server_flag() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        let (root, paths) = temp_paths("launch");
        let version = root.join("fake-horizon");
        let resources = version.join(".local/horizon-client/26.3");
        fs::create_dir_all(&resources).unwrap();
        fs::write(resources.join("asset-index.json"), "{}").unwrap();
        let args_file = root.join("args.txt");
        let client = version.join("horizon-client");
        fs::write(
            &client,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n", args_file.display()),
        )
        .unwrap();
        let mut mode = fs::metadata(&client).unwrap().permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&client, mode).unwrap();

        let game_dir = root.join("game");
        fs::create_dir_all(&game_dir).unwrap();

        let mut store = Store::default();
        store.settings.custom_horizon_dir = Some(version.to_string_lossy().into_owned());
        store.profiles.push(Profile {
            id: "p1".into(),
            name: "Test".into(),
            description: String::new(),
            player_name: "Player".into(),
            game_dir: game_dir.to_string_lossy().into_owned(),
            horizon_version: Some("fake-horizon (своя)".into()),
            skin: None,
            detected: true,
            created_at: 1,
            last_played: None,
            last_exit_code: None,
        });
        store.servers.push(ServerEntry {
            id: "server-1".into(),
            name: "Anarchy".into(),
            host: "play.example.com".into(),
            port: 25566,
            description: String::new(),
            banner: None,
            icon: None,
            fallback_text: "AN".into(),
            fallback_color: "#6750a4".into(),
            profile_id: Some("p1".into()),
            pinned: false,
            position: 1,
            created_at: 1,
            last_played: None,
        });
        store::save(&paths, &store).unwrap();
        let state = AppState::new(paths, store);

        let result = launch_server_core(None, &state, "server-1").unwrap();
        assert!(result.pid > 0);

        let deadline = Instant::now() + Duration::from_secs(5);
        while !args_file.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let args = fs::read_to_string(&args_file).expect("fake client did not run");
        let lines: Vec<&str> = args.lines().collect();
        let index = lines
            .iter()
            .position(|line| *line == "--server")
            .expect("--server was not passed");
        assert_eq!(lines[index + 1], "play.example.com:25566");
        assert!(lines.contains(&"--resources"));

        let saved = store::load_or_init(&state.paths).unwrap();
        assert!(saved.servers[0].last_played.is_some());
        assert_eq!(saved.active_profile.as_deref(), Some("p1"));

        // A deleted build is reported instead of launching something random.
        let mut broken = saved;
        broken.profiles.clear();
        let state = AppState::new(state.paths, broken);
        let error = launch_server_core(None, &state, "server-1").unwrap_err();
        assert!(error.contains("Привязанная сборка не найдена"), "{error}");

        let _ = fs::remove_dir_all(&root);
    }
}
