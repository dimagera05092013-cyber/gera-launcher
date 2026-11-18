//! Vanilla Minecraft: Java Edition.
//!
//! GERA downloads the official Mojang metadata and client files, then runs the
//! unmodified game. No third-party launcher or renamed jar is involved.
//!
//! Only offline (single-player / offline-mode servers) sign-in is implemented:
//! it needs no Microsoft account and no client id. The player name is enough.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use md5::{Digest, Md5};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::journal::{Kind, Log};
use crate::net;
use crate::paths;
use crate::sysutil;

/// Set by `java_cancel`; every download loop checks it between files.
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn cancel_install() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn check_cancel() -> Result<(), String> {
    if CANCEL.load(Ordering::SeqCst) {
        Err("Загрузка отменена.".into())
    } else {
        Ok(())
    }
}

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const ADOPTIUM: &str = "https://api.adoptium.net/v3/binary/latest";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersionInfo {
    pub id: String,
    pub released: String,
    pub installed: bool,
    pub size: u64,
    pub java_major: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaProgress {
    pub stage: String,
    pub label: String,
    pub done: u64,
    pub total: u64,
    pub files_done: u64,
    pub files_total: u64,
    /// Average speed of the current stage, bytes per second.
    pub speed: u64,
}

/// Result of checking an installed version against Mojang's own metadata.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaCheck {
    pub id: String,
    pub ok: bool,
    pub checked: u64,
    pub missing: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaLaunch {
    pub pid: u32,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaExited {
    pub id: String,
    pub code: Option<i32>,
    pub log_path: String,
    /// True when the game died within 20 seconds — almost always a start-up
    /// failure, and the reason is in `tail`.
    pub early: bool,
    pub tail: String,
}

fn root() -> PathBuf {
    paths::data_home().join("gera-launcher/vanilla")
}

fn versions_dir() -> PathBuf {
    root().join("versions")
}

fn version_dir(id: &str) -> PathBuf {
    versions_dir().join(id)
}

fn libraries_dir() -> PathBuf {
    root().join("libraries")
}

fn assets_dir() -> PathBuf {
    root().join("assets")
}

fn natives_dir(id: &str) -> PathBuf {
    version_dir(id).join("natives")
}

pub fn game_dir() -> PathBuf {
    root().join("game")
}

fn java_store() -> PathBuf {
    root().join("java")
}

#[allow(clippy::too_many_arguments)]
fn emit(
    app: Option<&AppHandle>,
    stage: &str,
    label: &str,
    done: u64,
    total: u64,
    files_done: u64,
    files_total: u64,
    started: Instant,
) {
    let seconds = started.elapsed().as_secs_f64().max(0.001);
    let Some(app) = app else {
        return;
    };
    let _ = app.emit(
        "java-progress",
        JavaProgress {
            stage: stage.to_string(),
            label: label.to_string(),
            done,
            total,
            files_done,
            files_total,
            speed: (done as f64 / seconds) as u64,
        },
    );
}

fn manifest() -> Result<Value, String> {
    let text = net::fetch_text(MANIFEST_URL, 30)?;
    serde_json::from_str(&text).map_err(|e| format!("Не удалось разобрать список версий Minecraft: {e}"))
}

fn version_entry(manifest: &Value, id: &str) -> Option<Value> {
    manifest["versions"]
        .as_array()?
        .iter()
        .find(|item| item["id"] == id)
        .cloned()
}

fn version_json(id: &str) -> Result<Value, String> {
    let path = version_dir(id).join(format!("{id}.json"));
    let text = fs::read_to_string(&path)
        .map_err(|_| format!("Версия {id} не установлена."))?;
    serde_json::from_str(&text).map_err(|e| format!("Повреждён файл версии {id}: {e}"))
}

/// horizon-prepare verifies Mojang's own SHA-1 for every version JSON in the
/// game folder. Launchers that rewrite the file (including older GERA builds,
/// which pretty-printed it) make the preparer refuse to run, which then shows
/// up as a broken launch. Rewrite such files with Mojang's exact bytes.
pub fn repair_official_jsons(dir: &Path) -> Vec<String> {
    let mut repaired = Vec::new();
    let Ok(manifest) = manifest() else {
        return repaired;
    };
    let Ok(entries) = fs::read_dir(dir.join("versions")) else {
        return repaired;
    };
    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path().join(format!("{id}.json"));
        if !path.is_file() {
            continue;
        }
        let Some(item) = version_entry(&manifest, &id) else {
            continue;
        };
        let (Some(expected), Some(url)) = (item["sha1"].as_str(), item["url"].as_str()) else {
            continue;
        };
        let actual = net::sha1_file(&path).unwrap_or_default();
        if actual.eq_ignore_ascii_case(expected) {
            continue;
        }
        if let Ok(text) = net::fetch_text(url, 60) {
            if net::sha1_text(&text).eq_ignore_ascii_case(expected) && fs::write(&path, &text).is_ok()
            {
                repaired.push(id);
            }
        }
    }
    repaired
}

/// Latest stable versions, newest first.
pub fn list_versions() -> Result<Vec<JavaVersionInfo>, String> {
    let manifest = manifest()?;
    let mut result = Vec::new();
    for item in manifest["versions"].as_array().into_iter().flatten() {
        if item["type"] != "release" {
            continue;
        }
        let Some(id) = item["id"].as_str() else {
            continue;
        };
        let installed = version_dir(id).join(".complete").is_file();
        result.push(JavaVersionInfo {
            id: id.to_string(),
            released: item["releaseTime"].as_str().unwrap_or("").to_string(),
            installed,
            size: 0,
            java_major: 21,
        });
        if result.len() >= 14 {
            break;
        }
    }
    Ok(result)
}

fn fetch_file(
    log: &mut Log,
    url: &str,
    destination: &Path,
    expected: u64,
    sha1: Option<&str>,
) -> Result<(), String> {
    log.info(
        "download",
        &format!("{url} → {} ({expected} байт)", destination.display()),
    );
    match net::download_checked(url, destination, expected, sha1) {
        Ok(bytes) => {
            log.info("download", &format!("готово: {bytes} байт"));
            Ok(())
        }
        Err(error) => {
            log.error("download", &format!("{url}: {error}"));
            Err(error)
        }
    }
}

fn allowed(rules: Option<&Value>) -> bool {
    let Some(rules) = rules.and_then(Value::as_array) else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        let mut matches = true;
        if let Some(os) = rule.get("os") {
            if let Some(name) = os["name"].as_str() {
                if name != os_name() {
                    matches = false;
                }
            }
            if let Some(arch) = os["arch"].as_str() {
                if arch != os_arch() {
                    matches = false;
                }
            }
        }
        if let Some(features) = rule.get("features").and_then(Value::as_object) {
            // GERA never enables demo or quick-play features.
            if features.values().any(|value| value == true) {
                matches = false;
            }
        }
        if matches {
            allowed = rule["action"] == "allow";
        }
    }
    allowed
}

fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn os_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else {
        "x64"
    }
}

struct Download {
    path: String,
    url: String,
    size: u64,
    sha1: String,
}

fn library_plan(client: &Value) -> (Vec<Download>, Vec<Download>) {
    let mut jars = Vec::new();
    let mut natives = Vec::new();
    let Some(libraries) = client["libraries"].as_array() else {
        return (jars, natives);
    };

    for library in libraries {
        if !allowed(library.get("rules")) {
            continue;
        }
        let Some(downloads) = library.get("downloads") else {
            continue;
        };

        if let Some(artifact) = downloads.get("artifact") {
            let path = artifact["path"].as_str().unwrap_or("");
            let url = artifact["url"].as_str().unwrap_or("");
            if path.is_empty() || url.is_empty() {
                continue;
            }
            let item = Download {
                path: path.to_string(),
                url: url.to_string(),
                size: artifact["size"].as_u64().unwrap_or(0),
                sha1: artifact["sha1"].as_str().unwrap_or("").to_string(),
            };
            let looks_native = path.contains("-natives-") || path.contains("/natives-");
            if looks_native {
                natives.push(item);
            } else {
                jars.push(item);
            }
        }

        // Older format: a classifier table keyed by OS name.
        if let Some(classifiers) = downloads.get("classifiers").and_then(Value::as_object) {
            let key = library
                .get("natives")
                .and_then(|natives_table| natives_table.get(os_name()))
                .and_then(Value::as_str)
                .map(|name| name.replace("${arch}", if cfg!(target_pointer_width = "64") { "64" } else { "32" }));
            if let Some(key) = key {
                if let Some(artifact) = classifiers.get(&key) {
                    let path = artifact["path"].as_str().unwrap_or("");
                    let url = artifact["url"].as_str().unwrap_or("");
                    if !path.is_empty() && !url.is_empty() {
                        natives.push(Download {
                            path: path.to_string(),
                            url: url.to_string(),
                            size: artifact["size"].as_u64().unwrap_or(0),
                            sha1: artifact["sha1"].as_str().unwrap_or("").to_string(),
                        });
                    }
                }
            }
        }
    }
    (jars, natives)
}

fn download_many(
    app: Option<&AppHandle>,
    log: &mut Log,
    stage: &str,
    base: &Path,
    items: Vec<Download>,
) -> Result<(), String> {
    check_cancel()?;
    let total: u64 = items.iter().map(|item| item.size).sum();
    let next = AtomicUsize::new(0);
    let done = AtomicU64::new(0);
    let files = AtomicU64::new(0);
    let failure: Mutex<Option<(String, String)>> = Mutex::new(None);
    let count = items.len() as u64;
    let started = Instant::now();
    log.info(stage, &format!("файлов: {count}, {total} байт"));

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                if CANCEL.load(Ordering::SeqCst) {
                    break;
                }
                if failure.lock().map(|value| value.is_some()).unwrap_or(true) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= items.len() {
                    break;
                }
                let item = &items[index];
                let destination = base.join(&item.path);
                let problem = match net::download_checked(
                    &item.url,
                    &destination,
                    item.size,
                    Some(&item.sha1),
                ) {
                    Ok(_) => None,
                    Err(error) => Some(error),
                };
                if let Some(error) = problem {
                    if let Ok(mut slot) = failure.lock() {
                        *slot = Some((item.path.clone(), error));
                    }
                    break;
                }
                let total_done = done.fetch_add(item.size, Ordering::Relaxed) + item.size;
                let ready = files.fetch_add(1, Ordering::Relaxed) + 1;
                emit(
                    app,
                    stage,
                    &format!("Файлов: {ready} из {count}"),
                    total_done.min(total),
                    total,
                    ready,
                    count,
                    started,
                );
            });
        }
    });

    if CANCEL.load(Ordering::SeqCst) {
        log.warn(stage, "загрузка отменена пользователем");
        return Err("Загрузка отменена.".into());
    }
    if let Some((path, error)) = failure.into_inner().map_err(|e| e.to_string())? {
        let message = format!("Не удалось скачать {path}: {error}");
        log.error(stage, &message);
        return Err(message);
    }
    log.info(stage, &format!("готово: {count} файлов"));
    Ok(())
}

fn install_version(app: Option<&AppHandle>, id: &str) -> Result<Value, String> {
    CANCEL.store(false, Ordering::SeqCst);
    let mut log = Log::open(Kind::Assets);
    log.info(
        "start",
        &format!("установка Minecraft {id}, данные: {}", root().display()),
    );

    let started = Instant::now();
    emit(app, "meta", "Получаем список версий Mojang", 0, 1, 0, 1, started);
    let manifest = manifest().inspect_err(|error| log.error("meta", error))?;
    let entry = version_entry(&manifest, id)
        .ok_or_else(|| format!("Версия {id} не найдена у Mojang."))?;
    let url = entry["url"]
        .as_str()
        .ok_or("Нет ссылки на описание версии.")?;
    log.info("meta", &format!("описание версии: {url}"));
    emit(app, "meta", "Читаем описание версии", 0, 1, 0, 1, started);
    let text = net::fetch_text(url, 60).inspect_err(|error| log.error("meta", error))?;
    let client: Value = serde_json::from_str(&text)
        .map_err(|e| format!("Некорректное описание версии: {e}"))?;
    let major = client["javaVersion"]["majorVersion"].as_u64().unwrap_or(21);
    log.info("meta", &format!("Minecraft {id}, нужна Java {major}"));

    let dir = version_dir(id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Keep Mojang's bytes untouched: horizon-prepare checks the official SHA-1
    // of this file and refuses to run when a launcher has reformatted it.
    fs::write(dir.join(format!("{id}.json")), &text).map_err(|e| e.to_string())?;

    check_cancel()?;
    let client_download = &client["downloads"]["client"];
    let jar_url = client_download["url"]
        .as_str()
        .ok_or("Нет ссылки на клиент Minecraft.")?;
    let jar_size = client_download["size"].as_u64().unwrap_or(0);
    let jar_sha1 = client_download["sha1"].as_str();
    let jar_path = dir.join(format!("{id}.jar"));
    let jar_ready = fs::metadata(&jar_path).map(|m| m.len() == jar_size && jar_size > 0).unwrap_or(false);
    emit(
        app,
        "client",
        if jar_ready { "Клиент уже скачан" } else { "Скачиваем клиент Minecraft" },
        if jar_ready { jar_size } else { 0 },
        jar_size,
        if jar_ready { 1 } else { 0 },
        1,
        started,
    );
    if !jar_ready {
        fetch_file(&mut log, jar_url, &jar_path, jar_size, jar_sha1)?;
    }
    emit(app, "client", "Клиент готов", jar_size, jar_size, 1, 1, started);

    check_cancel()?;
    let (jars, natives) = library_plan(&client);
    emit(app, "libraries", "Скачиваем библиотеки", 0, 1, 0, jars.len() as u64, started);
    download_many(app, &mut log, "libraries", &libraries_dir(), jars)?;

    check_cancel()?;
    let native_dir = natives_dir(id);
    emit(app, "natives", "Готовим нативные библиотеки", 0, 1, 0, natives.len() as u64, started);
    if !natives.is_empty() {
        // Extraction replaces the previous contents; stale natives crash LWJGL.
        let _ = fs::remove_dir_all(&native_dir);
        fs::create_dir_all(&native_dir).map_err(|e| e.to_string())?;
    }
    let native_count = natives.len() as u64;
    for (done, item) in natives.into_iter().enumerate() {
        check_cancel()?;
        let archive = libraries_dir().join(&item.path);
        fetch_file(&mut log, &item.url, &archive, item.size, Some(&item.sha1))?;
        sysutil::extract_zip(&archive, &native_dir)?;
        let meta = native_dir.join("META-INF");
        if meta.is_dir() {
            let _ = fs::remove_dir_all(meta);
        }
        emit(
            app,
            "natives",
            &format!("Нативные файлы: {} из {native_count}", done + 1),
            done as u64 + 1,
            native_count,
            done as u64 + 1,
            native_count,
            started,
        );
    }
    log.info("natives", &format!("распаковано нативных библиотек: {native_count}"));

    check_cancel()?;
    install_assets(app, &mut log, &client)?;

    check_cancel()?;
    emit(app, "java", "Подбираем Java", 0, 1, 0, 1, started);
    ensure_java(app, &mut log, major)?;

    fs::write(dir.join(".complete"), id).map_err(|e| e.to_string())?;
    log.info("done", &format!("Minecraft {id} установлен"));
    emit(app, "done", "Готово", 1, 1, 1, 1, started);
    Ok(client)
}

/// Which asset objects still have to be downloaded. An existing file counts as
/// present only when its size matches the index; anything else is fetched again.
fn plan_asset_downloads(index: &Value, objects_dir: &Path) -> Result<(Vec<(String, u64)>, u64), String> {
    let objects = index["objects"]
        .as_object()
        .ok_or("В Asset Index нет списка объектов — файл повреждён.")?;
    let mut tasks: Vec<(String, u64)> = Vec::new();
    let mut ready = 0u64;
    for object in objects.values() {
        let Some(hash) = object["hash"].as_str() else {
            continue;
        };
        if hash.len() < 3 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let size = object["size"].as_u64().unwrap_or(0);
        let target = objects_dir.join(&hash[0..2]).join(hash);
        if target.is_file() && fs::metadata(&target).map(|m| m.len() == size).unwrap_or(false) {
            ready += 1;
            continue;
        }
        tasks.push((hash.to_string(), size));
    }
    Ok((tasks, ready))
}

fn install_assets(app: Option<&AppHandle>, log: &mut Log, client: &Value) -> Result<(), String> {
    let index = &client["assetIndex"];
    let index_id = index["id"].as_str().ok_or("Нет идентификатора ресурсов.")?;
    let index_url = index["url"]
        .as_str()
        .ok_or("Нет ссылки на список ресурсов.")?;
    let index_sha1 = index["sha1"].as_str();
    let index_size = index["size"].as_u64().unwrap_or(0);
    let index_path = assets_dir()
        .join("indexes")
        .join(format!("{index_id}.json"));
    let started = Instant::now();
    emit(app, "index", "Получаем Asset Index", 0, 1, 0, 1, started);
    let index_ready = fs::metadata(&index_path)
        .map(|m| m.len() == index_size && index_size > 0)
        .unwrap_or(false);
    if !index_ready {
        fetch_file(log, index_url, &index_path, index_size, index_sha1)?;
    }
    let text = fs::read_to_string(&index_path).map_err(|e| {
        let message = format!(
            "Asset Index скачан, но не читается ({}): {e}. Нажмите «Повторить загрузку».",
            index_path.display()
        );
        log.error("index", &message);
        message
    })?;
    let index_json: Value = serde_json::from_str(&text).map_err(|e| {
        let message = format!("Asset Index повреждён: {e}. Нажмите «Повторить загрузку».");
        log.error("index", &message);
        message
    })?;
    let objects_dir = assets_dir().join("objects");
    let (tasks, ready) = plan_asset_downloads(&index_json, &objects_dir)?;
    let objects_total = index_json["objects"]
        .as_object()
        .map(|objects| objects.len())
        .unwrap_or(0);
    log.info(
        "index",
        &format!("Asset Index {index_id}: {objects_total} объектов"),
    );

    let total: u64 = tasks.iter().map(|(_, size)| *size).sum();
    let count = tasks.len();
    log.info(
        "assets",
        &format!(
            "объектов в индексе: {objects_total}, уже на диске: {ready}, скачать: {count} ({total} байт)"
        ),
    );
    if count == 0 {
        emit(app, "verify", "Ресурсы уже на диске", 1, 1, ready, ready, started);
    }

    let next = AtomicUsize::new(0);
    let done = AtomicU64::new(0);
    let files: u64 = ready;
    let failure: Mutex<Option<(String, String)>> = Mutex::new(None);

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                if CANCEL.load(Ordering::SeqCst) {
                    break;
                }
                if failure.lock().map(|value| value.is_some()).unwrap_or(true) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= tasks.len() {
                    break;
                }
                let (hash, size) = &tasks[index];
                let target = assets_dir().join("objects").join(&hash[0..2]).join(hash);
                let url = format!(
                    "https://resources.download.minecraft.net/{}/{}",
                    &hash[0..2],
                    hash
                );
                let problem = match net::download_checked(&url, &target, *size, Some(hash)) {
                    Ok(_) => None,
                    Err(error) => Some(error),
                };
                if let Some(error) = problem {
                    if let Ok(mut slot) = failure.lock() {
                        *slot = Some((hash.clone(), error));
                    }
                    break;
                }
                let total_done = done.fetch_add(*size, Ordering::Relaxed) + size;
                let ready_now = files + index as u64 + 1;
                if index % 8 == 0 || index + 1 == tasks.len() {
                    emit(
                        app,
                        "assets",
                        &format!("Ресурсов: {ready_now} из {}", files + count as u64),
                        total_done.min(total),
                        total,
                        ready_now,
                        files + count as u64,
                        started,
                    );
                }
            });
        }
    });

    if CANCEL.load(Ordering::SeqCst) {
        log.warn("assets", "загрузка ресурсов отменена");
        return Err("Загрузка отменена.".into());
    }
    if let Some((hash, error)) = failure.into_inner().map_err(|e| e.to_string())? {
        let message = format!(
            "Не удалось скачать ресурс {hash}: {error}\nПроверьте интернет и нажмите «Повторить загрузку» — уже скачанное не загрузится заново."
        );
        log.error("assets", &message);
        return Err(message);
    }

    install_virtual(app, log, &index_json, started)?;
    emit(
        app,
        "verify",
        "Проверка целостности",
        total,
        total,
        files + count as u64,
        files + count as u64,
        started,
    );
    log.info("assets", "все объекты ресурсов на диске");
    Ok(())
}

/// Old versions (before 1.7) need a flat `assets/virtual/<index>` folder.
fn install_virtual(
    app: Option<&AppHandle>,
    log: &mut Log,
    index_json: &Value,
    started: Instant,
) -> Result<(), String> {
    if index_json["virtual"].as_bool() != Some(true) {
        return Ok(());
    }
    let index_id = index_json["id"]
        .as_str()
        .or_else(|| index_json["map_to_resources"].as_str())
        .unwrap_or("legacy");
    let target_root = assets_dir().join("virtual").join(index_id);
    let Some(objects) = index_json["objects"].as_object() else {
        return Ok(());
    };
    let count = objects.len() as u64;
    log.info("virtual", &format!("собираем {count} файлов в {}", target_root.display()));
    for (done, (name, object)) in objects.iter().enumerate() {
        check_cancel()?;
        let Some(hash) = object["hash"].as_str() else {
            continue;
        };
        let source = assets_dir().join("objects").join(&hash[0..2]).join(hash);
        let target = target_root.join(name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if target.is_file() {
            continue;
        }
        if fs::hard_link(&source, &target).is_err() {
            fs::copy(&source, &target)
                .map_err(|e| format!("Не удалось скопировать {name}: {e}"))?;
        }
        if done % 64 == 0 {
            emit(
                app,
                "virtual",
                &format!("Файлов: {} из {count}", done + 1),
                done as u64,
                count,
                done as u64,
                count,
                started,
            );
        }
    }
    Ok(())
}

/// Verify an installed version against Mojang's metadata: every library and
/// asset must exist with the expected SHA-1. Slow by nature (hundreds of MB),
/// so it only runs when the player asks for it.
pub fn check_blocking(id: String) -> Result<JavaCheck, String> {
    let mut log = Log::open(Kind::Assets);
    log.info("check", &format!("проверка Minecraft {id}"));
    let client = version_json(&id)?;
    let mut checked = 0u64;
    let mut missing: Vec<String> = Vec::new();

    let jar = version_dir(&id).join(format!("{id}.jar"));
    let jar_sha1 = client["downloads"]["client"]["sha1"].as_str().unwrap_or("");
    checked += 1;
    if !jar.is_file() {
        missing.push(format!("{id}.jar"));
    } else if !jar_sha1.is_empty() && net::sha1_file(&jar)? != jar_sha1 {
        missing.push(format!("{id}.jar (повреждён)"));
    }

    let (jars, natives) = library_plan(&client);
    for item in jars.into_iter().chain(natives) {
        checked += 1;
        check_cancel()?;
        let path = libraries_dir().join(&item.path);
        if !path.is_file() {
            missing.push(item.path.clone());
            continue;
        }
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size != item.size || (!item.sha1.is_empty() && net::sha1_file(&path)? != item.sha1) {
            missing.push(format!("{} (повреждён)", item.path));
        }
    }

    let index = &client["assetIndex"];
    let index_id = index["id"].as_str().unwrap_or("");
    let index_path = assets_dir().join("indexes").join(format!("{index_id}.json"));
    checked += 1;
    if !index_path.is_file() {
        missing.push(format!("assets/indexes/{index_id}.json"));
    } else {
        let text = fs::read_to_string(&index_path).map_err(|e| e.to_string())?;
        let index_json: Value = serde_json::from_str(&text)
            .map_err(|e| format!("Asset Index повреждён: {e}"))?;
        let Some(objects) = index_json["objects"].as_object() else {
            return Err("Asset Index без списка объектов — нажмите «Повторить загрузку».".into());
        };
        for object in objects.values() {
            checked += 1;
            if checked % 64 == 0 {
                check_cancel()?;
            }
            let Some(hash) = object["hash"].as_str() else {
                continue;
            };
            if hash.len() < 3 {
                continue;
            }
            let size = object["size"].as_u64().unwrap_or(0);
            let path = assets_dir().join("objects").join(&hash[0..2]).join(hash);
            let actual = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let damaged = if !path.is_file() {
                true
            } else if size > 0 && actual != size {
                true
            } else {
                net::sha1_file(&path)? != hash
            };
            if damaged {
                missing.push(format!("assets/objects/{}/{}", &hash[0..2], hash));
                if missing.len() >= 200 {
                    break;
                }
            }
        }
    }

    let ok = missing.is_empty();
    let detail = if ok {
        format!("Проверено файлов: {checked}. Всё на месте.")
    } else {
        format!(
            "Проверено файлов: {checked}. Не хватает или повреждено: {}.",
            missing.len()
        )
    };
    if ok {
        log.info("check", &detail);
    } else {
        log.error("check", &format!("{detail} {}", missing.join(", ")));
    }
    Ok(JavaCheck {
        id,
        ok,
        checked,
        missing,
        detail,
    })
}

/// Java installed on the machine, with its major version.
fn system_java() -> Option<(PathBuf, u64)> {
    let output = crate::sysutil::quiet(Command::new("java").arg("-version"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let start = text.find("version \"")? + "version \"".len();
    let rest = &text[start..];
    let version = rest.split('"').next()?;
    let major = if let Some(rest) = version.strip_prefix("1.") {
        rest.split(['.', '_', '-']).next()?.parse().ok()?
    } else {
        version.split(['.', '-', '+']).next()?.parse().ok()?
    };
    Some((PathBuf::from("java"), major))
}

/// Prefer the Java already installed on the PC; fall back to the copy GERA
/// downloaded earlier. Nothing is downloaded when the system one fits.
fn resolve_java(major: u64) -> Option<PathBuf> {
    if let Some((path, found)) = system_java() {
        if found >= major {
            return Some(path);
        }
    }
    find_java(&java_store().join(format!("java{major}")))
}

fn find_java(home: &Path) -> Option<PathBuf> {
    let direct = home.join("bin").join(sysutil::exe("java"));
    if direct.is_file() {
        return Some(direct);
    }
    let entries = fs::read_dir(home).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join("bin").join(sysutil::exe("java"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn ensure_java(app: Option<&AppHandle>, log: &mut Log, major: u64) -> Result<PathBuf, String> {
    let started = Instant::now();
    if let Some(java) = resolve_java(major) {
        log.info("java", &format!("используем Java с компьютера: {}", java.display()));
        emit(app, "java", "Java найдена на компьютере", 1, 1, 1, 1, started);
        return Ok(java);
    }
    let home = java_store().join(format!("java{major}"));
    if let Some(java) = find_java(&home) {
        log.info("java", &format!("используем скачанную Java: {}", java.display()));
        emit(app, "java", "Java уже скачана", 1, 1, 1, 1, started);
        return Ok(java);
    }
    let (os, archive_name) = if cfg!(windows) {
        ("windows", "jre.zip")
    } else if cfg!(target_os = "macos") {
        ("mac", "jre.tar.gz")
    } else {
        ("linux", "jre.tar.gz")
    };
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x64" };
    let url = format!("{ADOPTIUM}/{major}/ga/{os}/{arch}/jre/hotspot/normal/eclipse");
    let archive = java_store().join(archive_name);
    fs::create_dir_all(java_store()).map_err(|e| e.to_string())?;
    log.info("java", &format!("скачиваем Java {major}: {url}"));
    emit(app, "java", &format!("Скачиваем Java {major}"), 0, 1, 0, 1, started);
    net::download(&url, &archive).inspect_err(|error| log.error("java", error))?;
    emit(app, "java", "Распаковываем Java", 1, 1, 1, 1, started);
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let unpacked = if archive_name.ends_with(".zip") {
        sysutil::extract_zip(&archive, &home)
    } else {
        sysutil::extract_tar_gz(&archive, &home)
    };
    if let Err(error) = unpacked {
        log.error("java", &format!("распаковка Java: {error}"));
        return Err(format!("Не удалось распаковать Java: {error}"));
    }
    let _ = fs::remove_file(&archive);
    let java = find_java(&home).ok_or_else(|| {
        "В скачанной Java не найден исполняемый файл java.".to_string()
    })?;
    log.info("java", &format!("Java {major} готова: {}", java.display()));
    Ok(java)
}

fn classpath(client: &Value, id: &str) -> Vec<String> {
    let (jars, natives) = library_plan(client);
    // Modern Minecraft lets LWJGL load natives straight from these jars
    // (`NativeLibrariesBootstrap`), so they belong on the classpath too.
    let mut entries: Vec<String> = jars
        .into_iter()
        .chain(natives)
        .map(|item| libraries_dir().join(item.path).to_string_lossy().into_owned())
        .collect();
    entries.push(
        version_dir(id)
            .join(format!("{id}.jar"))
            .to_string_lossy()
            .into_owned(),
    );
    entries
}

fn arg_list(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items
            .iter()
            .flat_map(|item| match item {
                Value::String(text) => vec![text.clone()],
                other => {
                    if allowed(other.get("rules")) {
                        arg_list(&other["value"])
                    } else {
                        Vec::new()
                    }
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn offline_uuid(name: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{name}").as_bytes());
    let mut bytes = hasher.finalize();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn substitute(value: &str, map: &[(&str, &str)]) -> String {
    let mut text = value.to_string();
    for (key, replacement) in map {
        text = text.replace(key, replacement);
    }
    text
}

#[cfg(windows)]
fn classpath_separator() -> &'static str {
    ";"
}

#[cfg(not(windows))]
fn classpath_separator() -> &'static str {
    ":"
}

/// Install one official version (client, libraries, assets and a matching JRE).
pub fn install_blocking(app: AppHandle, id: String) -> Result<(), String> {
    install_version(Some(&app), &id)?;
    Ok(())
}

/// Headless install used by `--java-install`.
pub fn install_cli(id: String) -> Result<(), String> {
    install_version(None, &id).map(|_| ())
}

/// Launch an installed version in offline mode.
pub fn launch_blocking(app: Option<AppHandle>, id: String, player: String) -> Result<JavaLaunch, String> {
    let mut journal = Log::open(Kind::Launch);
    let name = player.trim();
    let valid = (3..=16).contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err("Ник: 3–16 символов, только латинские буквы, цифры и _.".into());
    }

    let client = version_json(&id)?;
    let jar = version_dir(&id).join(format!("{id}.jar"));
    if !jar.is_file() {
        let message = format!("Клиент Minecraft {id} не найден: {}. Установите версию заново.", jar.display());
        journal.error("check", &message);
        return Err(message);
    }
    let asset_index = client["assetIndex"]["id"].as_str().unwrap_or("").to_string();
    let index_file = assets_dir().join("indexes").join(format!("{asset_index}.json"));
    if !index_file.is_file() {
        let message = format!(
            "Ресурсы Minecraft {id} не установлены: нет {}. Откройте раздел Java и нажмите «Установить».",
            index_file.display()
        );
        journal.error("check", &message);
        return Err(message);
    }
    let major = client["javaVersion"]["majorVersion"].as_u64().unwrap_or(21);
    let java = resolve_java(major)
        .ok_or_else(|| format!("Нет Java {major}. Установите версию заново."))?;

    let game = game_dir();
    fs::create_dir_all(&game).map_err(|e| e.to_string())?;
    let natives = natives_dir(&id);
    fs::create_dir_all(&natives).map_err(|e| e.to_string())?;

    let cp = classpath(&client, &id).join(classpath_separator());
    let uuid = offline_uuid(name);
    let natives_text = natives.to_string_lossy().into_owned();
    let assets_text = assets_dir().to_string_lossy().into_owned();
    let game_text = game.to_string_lossy().into_owned();
    let version_type = client["type"].as_str().unwrap_or("release").to_string();
    let launcher_version = env!("CARGO_PKG_VERSION");
    let libraries_text = libraries_dir().to_string_lossy().into_owned();

    let map: Vec<(&str, &str)> = vec![
        ("${auth_player_name}", name),
        ("${version_name}", &id),
        ("${game_directory}", &game_text),
        ("${assets_root}", &assets_text),
        ("${assets_index_name}", &asset_index),
        ("${auth_uuid}", &uuid),
        ("${auth_access_token}", "0"),
        ("${auth_session}", "0"),
        ("${user_type}", "legacy"),
        ("${version_type}", &version_type),
        ("${natives_directory}", &natives_text),
        ("${classpath}", &cp),
        ("${classpath_separator}", classpath_separator()),
        ("${library_directory}", &libraries_text),
        ("${launcher_name}", "GERA"),
        ("${launcher_version}", launcher_version),
        ("${clientid}", ""),
        ("${auth_xuid}", ""),
        ("${resolution_width}", "1280"),
        ("${resolution_height}", "720"),
    ];

    let mut jvm: Vec<String> = Vec::new();
    if let Some(arguments) = client.get("arguments") {
        jvm = arg_list(&arguments["jvm"]);
    }
    if jvm.is_empty() {
        jvm.push(format!("-Djava.library.path={natives_text}"));
        jvm.push("-cp".into());
        jvm.push("${classpath}".into());
    }

    let mut game_args: Vec<String> = Vec::new();
    if let Some(arguments) = client.get("arguments") {
        game_args = arg_list(&arguments["game"]);
    }
    if game_args.is_empty() {
        if let Some(legacy) = client["minecraftArguments"].as_str() {
            game_args = legacy.split_whitespace().map(str::to_string).collect();
        }
    }

    let main_class = client["mainClass"].as_str().unwrap_or("net.minecraft.client.main.Main");

    let logs = paths::Paths::new().logs;
    fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let log_path = logs.join(format!("minecraft-{id}-{stamp}.log"));
    let log = fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;

    let mut command = Command::new(&java);
    for value in &jvm {
        command.arg(substitute(value, &map));
    }
    command.arg(main_class);
    for value in &game_args {
        command.arg(substitute(value, &map));
    }
    command
        .current_dir(&game)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));

    journal.info("start", &format!("java: {}", java.display()));
    journal.info("start", &format!("game dir: {}", game.display()));
    journal.info("start", &format!("assets: {}", assets_text));
    journal.info("start", &format!("asset index: {asset_index}"));
    journal.info("start", &format!("log: {}", log_path.display()));
    journal.info(
        "start",
        &format!(
            "аргументы: {}",
            std::iter::once(main_class.to_string())
                .chain(game_args.iter().map(|value| substitute(value, &map)))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );

    let mut child = match sysutil::quiet(&mut command).spawn() {
        Ok(child) => child,
        Err(error) => {
            let message = format!("Не удалось запустить Minecraft: {error}");
            journal.error("start", &message);
            return Err(message);
        }
    };
    let pid = child.id();
    drop(child.stdin.take());
    let log_path_text = log_path.to_string_lossy().into_owned();
    if let Some(app) = app {
        let watched = log_path.clone();
        let game_id = id.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let status = child.wait();
            let code = status.ok().and_then(|value| value.code());
            let elapsed = started.elapsed();
            let tail = tail_file(&watched, 25);
            let early = elapsed.as_secs() < 20;
            let mut journal = Log::open(Kind::Launch);
            let level = if code == Some(0) && !early { "INFO" } else { "ERROR" };
            journal.write(
                level,
                "exit",
                &format!(
                    "код {code:?} через {:.1} с{}",
                    elapsed.as_secs_f64(),
                    if tail.is_empty() { String::new() } else { format!(", последние строки: {tail}") }
                ),
            );
            let crash = game_dir().join("crash-reports");
            if crash.is_dir() {
                journal.warn("exit", &format!("отчёты о падениях: {}", crash.display()));
            }
            let _ = app.emit(
                "java-exited",
                JavaExited {
                    id: game_id,
                    code,
                    log_path: watched.to_string_lossy().into_owned(),
                    early,
                    tail,
                },
            );
        });
    } else {
        drop(child);
    }

    Ok(JavaLaunch {
        pid,
        log_path: log_path_text,
    })
}

fn tail_file(path: &Path, lines: usize) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    let mut collected: Vec<&str> = text.lines().rev().take(lines).collect();
    collected.reverse();
    collected.join("\n")
}

#[tauri::command]
pub async fn java_versions() -> Result<Vec<JavaVersionInfo>, String> {
    tauri::async_runtime::spawn_blocking(list_versions)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn java_install(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || install_blocking(app, id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn java_launch(
    app: AppHandle,
    id: String,
    player: String,
) -> Result<JavaLaunch, String> {
    tauri::async_runtime::spawn_blocking(move || launch_blocking(Some(app), id, player))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn java_check(id: String) -> Result<JavaCheck, String> {
    tauri::async_runtime::spawn_blocking(move || check_blocking(id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn java_cancel() {
    cancel_install();
}

#[tauri::command]
pub fn java_folder() -> String {
    game_dir().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_mojang_scheme() {
        assert_eq!(offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[test]
    fn rules_respect_os() {
        let linux = serde_json::json!([{ "action": "allow", "os": { "name": "linux" } }]);
        assert_eq!(allowed(Some(&linux)), cfg!(target_os = "linux"));
        assert!(allowed(None));
    }

    #[test]
    fn placeholders_are_replaced() {
        assert_eq!(
            substitute("${auth_player_name}", &[("${auth_player_name}", "gera_2347")]),
            "gera_2347"
        );
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gera-vanilla-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn index_with(objects: serde_json::Value) -> Value {
        serde_json::json!({ "objects": objects })
    }

    #[test]
    fn asset_index_lists_every_object() {
        let dir = temp_dir("index");
        let index = index_with(serde_json::json!({
            "icons/icon_16x16.png": { "hash": "5ff04807c356f1beed0b86ccf659b44b9983e3fa", "size": 781 },
            "icons/icon_32x32.png": { "hash": "aab04807c356f1beed0b86ccf659b44b9983e3fb", "size": 900 }
        }));
        let (tasks, ready) = plan_asset_downloads(&index, &dir).unwrap();
        assert_eq!(ready, 0);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].1, 781);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_asset_index_is_rejected() {
        let dir = temp_dir("broken");
        let index = serde_json::json!({ "objects": [] });
        let error = plan_asset_downloads(&index, &dir).unwrap_err();
        assert!(error.contains("Asset Index"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_object_is_not_downloaded_again() {
        let dir = temp_dir("present");
        let hash = "5ff04807c356f1beed0b86ccf659b44b9983e3fa";
        let target = dir.join(&hash[0..2]).join(hash);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, vec![0u8; 781]).unwrap();
        let index = index_with(serde_json::json!({
            "icons/icon_16x16.png": { "hash": hash, "size": 781 }
        }));
        let (tasks, ready) = plan_asset_downloads(&index, &dir).unwrap();
        assert!(tasks.is_empty());
        assert_eq!(ready, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncated_object_is_downloaded_again() {
        let dir = temp_dir("truncated");
        let hash = "5ff04807c356f1beed0b86ccf659b44b9983e3fa";
        let target = dir.join(&hash[0..2]).join(hash);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, vec![0u8; 10]).unwrap();
        let index = index_with(serde_json::json!({
            "icons/icon_16x16.png": { "hash": hash, "size": 781 }
        }));
        let (tasks, ready) = plan_asset_downloads(&index, &dir).unwrap();
        assert_eq!(ready, 0);
        assert_eq!(tasks.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn paths_with_spaces_and_cyrillic_are_used_as_is() {
        let dir = temp_dir("путь с пробелами");
        let target = dir.join("assets/objects/ab/abc");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"data").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"data");
        let _ = fs::remove_dir_all(&dir);
    }
}
