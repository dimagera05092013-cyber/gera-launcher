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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use md5::{Digest, Md5};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::net;
use crate::paths;
use crate::sysutil;

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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaLaunch {
    pub pid: u32,
    pub log_path: String,
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

fn emit(app: Option<&AppHandle>, stage: &str, label: &str, done: u64, total: u64) {
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

fn fetch_file(url: &str, destination: &Path, expected: u64) -> Result<(), String> {
    if destination.is_file() {
        if let Ok(meta) = fs::metadata(destination) {
            if expected == 0 || meta.len() == expected {
                return Ok(());
            }
        }
    }
    let mut attempt = 0;
    loop {
        match net::download(url, destination) {
            Ok(_) => break,
            Err(error) if attempt < 2 => {
                attempt += 1;
                let _ = fs::remove_file(destination);
                std::thread::sleep(std::time::Duration::from_millis(800));
                let _ = error;
            }
            Err(error) => return Err(error),
        }
    }
    if expected > 0 {
        let size = fs::metadata(destination).map(|m| m.len()).unwrap_or(0);
        if size != expected {
            let _ = fs::remove_file(destination);
            return Err(format!(
                "Файл скачался не полностью ({} из {} байт).",
                size, expected
            ));
        }
    }
    Ok(())
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
                        });
                    }
                }
            }
        }
    }
    (jars, natives)
}

fn download_many(app: Option<&AppHandle>, stage: &str, base: &Path, items: Vec<Download>) -> Result<(), String> {
    let total: u64 = items.iter().map(|item| item.size).sum();
    let next = AtomicUsize::new(0);
    let done = AtomicU64::new(0);
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let count = items.len();

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                if failure.lock().map(|value| value.is_some()).unwrap_or(true) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= count {
                    break;
                }
                let item = &items[index];
                let destination = base.join(&item.path);
                if let Err(error) = fetch_file(&item.url, &destination, item.size) {
                    if let Ok(mut slot) = failure.lock() {
                        *slot = Some(format!("{}: {error}", item.path));
                    }
                    break;
                }
                let total_done = done.fetch_add(item.size, Ordering::Relaxed) + item.size;
                emit(
                    app,
                    stage,
                    &format!("Файлов: {} из {count}", index + 1),
                    total_done.min(total),
                    total,
                );
            });
        }
    });

    if let Some(error) = failure.into_inner().map_err(|e| e.to_string())? {
        return Err(error);
    }
    Ok(())
}

fn install_version(app: Option<&AppHandle>, id: &str) -> Result<Value, String> {
    let manifest = manifest()?;
    let entry = version_entry(&manifest, id).ok_or_else(|| format!("Версия {id} не найдена у Mojang."))?;
    let url = entry["url"].as_str().ok_or("Нет ссылки на описание версии.")?;
    emit(app, "version", "Получаем описание версии", 0, 1);
    let client: Value = {
        let text = net::fetch_text(url, 40)?;
        serde_json::from_str(&text).map_err(|e| format!("Некорректное описание версии: {e}"))?
    };

    let dir = version_dir(id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_string_pretty(&client).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    // Client jar.
    let client_download = &client["downloads"]["client"];
    let jar_url = client_download["url"].as_str().ok_or("Нет ссылки на клиент Minecraft.")?;
    emit(app, "client", "Скачиваем клиент Minecraft", 0, client_download["size"].as_u64().unwrap_or(0));
    fetch_file(
        jar_url,
        &dir.join(format!("{id}.jar")),
        client_download["size"].as_u64().unwrap_or(0),
    )?;
    emit(app, "client", "Клиент скачан", 1, 1);

    // Libraries and natives.
    let (jars, natives) = library_plan(&client);
    emit(app, "libraries", "Скачиваем библиотеки", 0, 1);
    download_many(app, "libraries", &libraries_dir(), jars)?;

    let native_dir = natives_dir(id);
    emit(app, "natives", "Готовим нативные библиотеки", 0, 1);
    if !natives.is_empty() {
        // Extraction replaces the previous contents; stale natives crash LWJGL.
        let _ = fs::remove_dir_all(&native_dir);
        fs::create_dir_all(&native_dir).map_err(|e| e.to_string())?;
    }
    for item in natives {
        let archive = libraries_dir().join(&item.path);
        fetch_file(&item.url, &archive, item.size)?;
        sysutil::extract_zip(&archive, &native_dir)?;
        let meta = native_dir.join("META-INF");
        if meta.is_dir() {
            let _ = fs::remove_dir_all(meta);
        }
    }

    // Assets.
    install_assets(app, &client)?;

    // Java runtime.
    let major = client["javaVersion"]["majorVersion"].as_u64().unwrap_or(21);
    ensure_java(app, major)?;

    fs::write(dir.join(".complete"), id).map_err(|e| e.to_string())?;
    emit(app, "done", "Готово", 1, 1);
    Ok(client)
}

fn install_assets(app: Option<&AppHandle>, client: &Value) -> Result<(), String> {
    let index = &client["assetIndex"];
    let index_id = index["id"].as_str().ok_or("Нет идентификатора ресурсов.")?;
    let index_url = index["url"].as_str().ok_or("Нет ссылки на список ресурсов.")?;
    let index_path = assets_dir()
        .join("indexes")
        .join(format!("{index_id}.json"));
    emit(app, "assets", "Скачиваем список ресурсов", 0, 1);
    fetch_file(index_url, &index_path, index["size"].as_u64().unwrap_or(0))?;

    let text = fs::read_to_string(&index_path).map_err(|e| e.to_string())?;
    let index_json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let some_objects = index_json["objects"].as_object().ok_or("Пустой список ресурсов.")?;

    let mut tasks: Vec<(String, u64)> = Vec::new();
    for object in some_objects.values() {
        let Some(hash) = object["hash"].as_str() else {
            continue;
        };
        let size = object["size"].as_u64().unwrap_or(0);
        let target = assets_dir().join("objects").join(&hash[0..2]).join(hash);
        if target.is_file() && fs::metadata(&target).map(|m| m.len() == size).unwrap_or(false) {
            continue;
        }
        tasks.push((hash.to_string(), size));
    }

    let total: u64 = tasks.iter().map(|(_, size)| *size).sum();
    let next = AtomicUsize::new(0);
    let done = AtomicU64::new(0);
    let count = tasks.len();
    let failure: Mutex<Option<String>> = Mutex::new(None);

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                if failure.lock().map(|value| value.is_some()).unwrap_or(true) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= count {
                    break;
                }
                let (hash, size) = &tasks[index];
                let target = assets_dir().join("objects").join(&hash[0..2]).join(hash);
                let url = format!("https://resources.download.minecraft.net/{}/{}", &hash[0..2], hash);
                if let Err(error) = fetch_file(&url, &target, *size) {
                    if let Ok(mut slot) = failure.lock() {
                        *slot = Some(format!("Ресурс {hash}: {error}"));
                    }
                    break;
                }
                let total_done = done.fetch_add(*size, Ordering::Relaxed) + size;
                if index % 16 == 0 || index + 1 == count {
                    emit(
                        app,
                        "assets",
                        &format!("Ресурсов: {} из {count}", index + 1),
                        total_done.min(total),
                        total,
                    );
                }
            });
        }
    });

    if let Some(error) = failure.into_inner().map_err(|e| e.to_string())? {
        return Err(error);
    }
    emit(app, "assets", "Ресурсы готовы", total, total);
    Ok(())
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

fn ensure_java(app: Option<&AppHandle>, major: u64) -> Result<PathBuf, String> {
    if let Some(java) = resolve_java(major) {
        emit(app, "java", "Используем Java с этого компьютера", 1, 1);
        return Ok(java);
    }
    let home = java_store().join(format!("java{major}"));
    if let Some(java) = find_java(&home) {
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
    emit(app, "java", &format!("Скачиваем Java {major}"), 0, 1);
    net::download(&url, &archive)?;
    emit(app, "java", "Распаковываем Java", 0, 1);
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    if archive_name.ends_with(".zip") {
        sysutil::extract_zip(&archive, &home)?;
    } else {
        sysutil::extract_tar_gz(&archive, &home)?;
    }
    let _ = fs::remove_file(&archive);
    find_java(&home).ok_or_else(|| "В скачанной Java не найден исполняемый файл.".into())
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
pub fn launch_blocking(id: String, player: String) -> Result<JavaLaunch, String> {
    let name = player.trim();
    let valid = (3..=16).contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err("Ник: 3–16 символов, только латинские буквы, цифры и _.".into());
    }

    let client = version_json(&id)?;
    let jar = version_dir(&id).join(format!("{id}.jar"));
    if !jar.is_file() {
        return Err(format!("Версия {id} не установлена."));
    }
    let major = client["javaVersion"]["majorVersion"].as_u64().unwrap_or(21);
    let java = resolve_java(major)
        .ok_or_else(|| format!("Нет Java {major}. Установите версию заново."))?;

    let game = game_dir();
    fs::create_dir_all(&game).map_err(|e| e.to_string())?;
    let natives = natives_dir(&id);
    fs::create_dir_all(&natives).map_err(|e| e.to_string())?;

    let cp = classpath(&client, &id).join(classpath_separator());
    let asset_index = client["assetIndex"]["id"].as_str().unwrap_or("").to_string();
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

    let child = sysutil::quiet(&mut command)
        .spawn()
        .map_err(|e| format!("Не удалось запустить Minecraft: {e}"))?;

    Ok(JavaLaunch {
        pid: child.id(),
        log_path: log_path.to_string_lossy().into_owned(),
    })
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
pub async fn java_launch(id: String, player: String) -> Result<JavaLaunch, String> {
    tauri::async_runtime::spawn_blocking(move || launch_blocking(id, player))
        .await
        .map_err(|e| e.to_string())?
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
}
