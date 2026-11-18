use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::journal::{Kind, Log};
use crate::net;
use crate::paths;

const REPO_API: &str = "https://api.github.com/repos/horizon-rust/horizon/releases";
const ASSET_LINUX: &str = "horizon-linux-x86_64.tar.gz";
const ASSET_WINDOWS: &str = "horizon-windows-x64.zip";

/// Official client archive for the platform GERA runs on.
fn asset_name() -> &'static str {
    if cfg!(windows) {
        ASSET_WINDOWS
    } else {
        ASSET_LINUX
    }
}

fn client_binary() -> String {
    crate::sysutil::exe("horizon-client")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRelease {
    pub tag: String,
    pub folder: String,
    pub name: String,
    pub published: String,
    pub nightly: bool,
    pub size: u64,
    pub installed: bool,
    pub asset: String,
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
    tag_name: String,
    name: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Debug, Deserialize)]
struct ApiAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

fn api(url: &str) -> Result<Vec<ApiRelease>, String> {
    let text = net::fetch_text(url, 40)?;
    serde_json::from_str(&text).map_err(|e| format!("Не удалось разобрать ответ GitHub: {e}"))
}

fn release_folder(tag: &str, published: &str) -> String {
    if tag == "nightly" {
        let date = published.get(0..10).unwrap_or("latest");
        format!("nightly-{date}")
    } else {
        tag.to_string()
    }
}

fn installed_folders() -> Vec<String> {
    let mut folders = Vec::new();
    if let Ok(entries) = fs::read_dir(paths::clients_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(client_binary()).is_file() {
                if let Some(name) = path.file_name() {
                    folders.push(name.to_string_lossy().into_owned());
                }
            }
        }
    }
    folders
}

fn asset_of<'a>(release: &'a ApiRelease) -> Option<&'a ApiAsset> {
    release.assets.iter().find(|a| a.name == asset_name())
}

/// Official Horizon client builds published on GitHub. The official graphical
/// launcher is not involved: GERA reads the same release assets directly.
pub fn list_releases() -> Result<Vec<ClientRelease>, String> {
    let releases = api(&format!("{REPO_API}?per_page=60"))?;
    let installed = installed_folders();
    let mut result = Vec::new();

    for release in releases {
        let Some(asset) = asset_of(&release) else {
            continue;
        };
        // The `nightly` channel is published as a prerelease; it is the build
        // the official launcher uses, so it stays available here too.
        if release.prerelease && !release.tag_name.starts_with("nightly") {
            continue;
        }
        let size = asset.size;
        let url = asset.browser_download_url.clone();
        let published = release.published_at.clone().unwrap_or_default();
        let tag = release.tag_name;
        let folder = release_folder(&tag, &published);
        let nightly = tag == "nightly" || tag.starts_with("nightly-");
        let name = release.name.unwrap_or_else(|| tag.clone());
        result.push(ClientRelease {
            installed: installed.contains(&folder),
            folder,
            tag,
            name,
            published,
            nightly,
            size,
            asset: url,
        });
        if result.len() >= 8 {
            break;
        }
    }
    Ok(result)
}

fn release_by_tag(tag: &str) -> Result<ApiRelease, String> {
    let text = net::fetch_text(
        &format!("{REPO_API}/tags/{}", urlencode(tag)),
        40,
    )?;
    serde_json::from_str(&text).map_err(|e| format!("Не удалось разобрать релиз {tag}: {e}"))
}

fn urlencode(value: &str) -> String {
    value.replace('@', "%40").replace('/', "%2F")
}

/// Unpack the official client archive. Linux builds are a tar.gz with the
/// binaries at the root; Windows builds are a zip with one top-level folder.
fn unpack_client(archive: &Path, staging: &Path) -> Result<(), String> {
    let zip = archive
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);
    if zip {
        crate::sysutil::extract_zip(archive, staging)?;
    } else {
        crate::sysutil::extract_tar_gz(archive, staging)?;
    }
    if staging.join(client_binary()).is_file() {
        return Ok(());
    }
    let subdirs: Vec<PathBuf> = fs::read_dir(staging)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    if let [only] = subdirs.as_slice() {
        if only.join(client_binary()).is_file() {
            for entry in fs::read_dir(only).map_err(|e| e.to_string())?.flatten() {
                let target = staging.join(entry.file_name());
                fs::rename(entry.path(), &target)
                    .map_err(|e| format!("Не удалось разложить клиент: {e}"))?;
            }
            let _ = fs::remove_dir(only);
        }
    }
    Ok(())
}

/// Download an official build, verify it against the release's SHA256SUMS and
/// unpack it into GERA's own clients directory.
pub fn install(tag: &str) -> Result<String, String> {
    let release = release_by_tag(tag)?;
    let asset = asset_of(&release).ok_or_else(|| {
        format!("В этом релизе нет сборки {}.", asset_name())
    })?;
    let published = release.published_at.clone().unwrap_or_default();
    let folder = release_folder(&release.tag_name, &published);
    let target = paths::clients_dir().join(&folder);
    if target.exists() {
        return Err(format!("Клиент {folder} уже установлен."));
    }

    let checksums = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .ok_or_else(|| "В релизе нет SHA256SUMS — установка отменена.".to_string())?;
    let sums = net::fetch_text(&checksums.browser_download_url, 40)?;
    let expected = net::checksum_for(&sums, asset_name())
        .ok_or_else(|| "Контрольная сумма для сборки не найдена.".to_string())?;

    let cache = paths::data_home()
        .join("gera-launcher/cache/downloads")
        .join(&folder);
    let archive = cache.join(asset_name());
    net::download(&asset.browser_download_url, &archive)?;
    let actual = net::sha256_file(&archive)?;
    if actual != expected {
        let _ = fs::remove_file(&archive);
        return Err(format!(
            "Контрольная сумма не совпала. Ожидалось {expected}, получено {actual}. Установка отменена."
        ));
    }

    let staging = paths::clients_dir().join(format!(".staging-{folder}"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    if let Err(error) = unpack_client(&archive, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if !staging.join(client_binary()).is_file() {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("В архиве нет {}.", client_binary()));
    }
    fs::rename(&staging, &target).map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        format!("Не удалось завершить установку: {e}")
    })?;
    let _ = fs::remove_file(&archive);
    Ok(folder)
}

/// `horizon-prepare` prints
/// `... or save https://piston-meta.mojang.com/v1/packages/<sha1>/<ver>.json as <path> and prepare again`.
/// GERA follows that instruction, but only inside its own client folder.
fn pinned_manifest(output: &str, root: &Path) -> Option<(String, String)> {
    for line in output.lines() {
        let Some(rest) = line.split("save ").nth(1) else {
            continue;
        };
        let Some((url, path)) = rest.split_once(" as ") else {
            continue;
        };
        let url = url.trim().trim_end_matches(',');
        // The sentence continues after the path, and paths may contain spaces.
        let path = path
            .split(" and prepare again")
            .next()
            .unwrap_or(path)
            .trim();
        let lower = path.to_lowercase();
        if !lower.ends_with("manifest.json") || !url.starts_with("https://piston-meta.mojang.com/") {
            continue;
        }
        // Never write anywhere except the client folder the preparer names.
        let folder = root.to_string_lossy().to_lowercase();
        if !lower.starts_with(&folder) {
            continue;
        }
        return Some((url.to_string(), path.to_string()));
    }
    None
}

fn mentions_server_jar(output: &str) -> bool {
    output.contains("server jar")
}

/// Where the pinned server jar comes from: the version JSON Horizon names in
/// `research-lock.json`, with Mojang's own URL and SHA-1.
fn server_jar_plan(root: &Path, minecraft_dir: &str) -> Option<(String, String, u64, PathBuf)> {
    if minecraft_dir.trim().is_empty() {
        return None;
    }
    let lock = fs::read_to_string(root.join("research-lock.json")).ok()?;
    let lock: serde_json::Value = serde_json::from_str(&lock).ok()?;
    let version = lock["minecraft"]["version"].as_str()?.to_string();
    let manifest = Path::new(minecraft_dir)
        .join("versions")
        .join(&version)
        .join(format!("{version}.json"));
    let text = fs::read_to_string(manifest).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let server = &json["downloads"]["server"];
    let url = server["url"].as_str()?.to_string();
    let sha1 = server["sha1"].as_str().unwrap_or("").to_string();
    let size = server["size"].as_u64().unwrap_or(0);
    let target = root
        .join(".local")
        .join("minecraft")
        .join(&version)
        .join("server.jar");
    Some((url, sha1, size, target))
}

fn manifest_digest(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split('/').collect();
    parts
        .windows(2)
        .find(|pair| pair[0] == "packages")
        .map(|pair| pair[1].to_string())
}

/// Run the official horizon-prepare that ships inside the client release. It
/// reads the player's own Minecraft copy and never downloads game files.
///
/// The exit code, the full output and the resulting resource folder are written
/// to `prepare.log`, so a failure is never just "error 0".
pub fn prepare(folder: &str, minecraft_dir: &str, server_jar: Option<&str>) -> Result<String, String> {
    let mut log = Log::open(Kind::Prepare);
    let root = PathBuf::from(folder);
    let root = if root.is_absolute() {
        root
    } else {
        paths::clients_dir().join(folder)
    };
    let prepare = root.join(crate::sysutil::exe("horizon-prepare"));
    if !prepare.is_file() {
        let message = format!("В этой сборке нет horizon-prepare: {}", prepare.display());
        log.error("check", &message);
        return Err(message);
    }
    let mc_text = minecraft_dir.trim();
    if !mc_text.is_empty() {
        let mc = PathBuf::from(mc_text);
        if !mc.is_dir() {
            let message = format!(
                "Папка Minecraft не найдена: {}. Укажите папку .minecraft, где игра хотя бы раз запускалась официальным лаунчером.",
                mc.display()
            );
            log.error("check", &message);
            return Err(message);
        }
    }
    let mut command = Command::new(&prepare);
    command.arg("--prepare").arg("--root").arg(&root);
    if !mc_text.is_empty() {
        command.arg("--minecraft-dir").arg(mc_text);
    }
    if let Some(jar) = server_jar.filter(|v| !v.trim().is_empty()) {
        command.arg("--server-jar").arg(jar.trim());
    }
    crate::sysutil::quiet(&mut command);
    log.info(
        "run",
        &format!(
            "{} --prepare --root {} --minecraft-dir {}",
            prepare.display(),
            root.display(),
            mc_text
        ),
    );
    let mut attempt = 0;
    let mut server_jar: Option<PathBuf> = None;
    let (text, code) = loop {
        let output = command.output().map_err(|e| {
            let message = format!("Не удалось запустить horizon-prepare: {e}");
            log.error("run", &message);
            message
        })?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        let code = output.status.code();
        if output.status.success() || attempt >= 3 {
            break (text, code);
        }
        attempt += 1;

        // horizon-prepare pins the exact SHA-1 of the version JSON it was built
        // against. When Mojang republishes (or a launcher rewrites) the file, the
        // preparer names the official URL to save; GERA does that and retries.
        if let Some((url, target)) = pinned_manifest(&text, &root) {
            log.warn(
                "pin",
                &format!("horizon-prepare требует официальный version JSON: {url}"),
            );
            if let Err(error) = net::download(&url, Path::new(&target)) {
                log.error("pin", &error);
                break (text, code);
            }
            if let Some(expected) = manifest_digest(&url) {
                let actual = net::sha1_file(Path::new(&target)).unwrap_or_default();
                if !actual.eq_ignore_ascii_case(&expected) {
                    let message = format!(
                        "Официальный version JSON скачался повреждённым ({actual} вместо {expected})."
                    );
                    log.error("pin", &message);
                    return Err(message);
                }
            }
            log.info("pin", &format!("сохранено в {target}, повторяем подготовку"));
            continue;
        }

        // A full preparation also needs the pinned server jar. GERA takes its
        // official URL and SHA-1 from the version JSON and downloads it once.
        if server_jar.is_none() && mentions_server_jar(&text) {
            let Some((url, sha1, size, target)) = server_jar_plan(&root, mc_text) else {
                break (text, code);
            };
            log.info("server jar", &format!("скачиваем официальный {url}"));
            if let Err(error) = net::download_checked(&url, &target, size, Some(&sha1)) {
                log.error("server jar", &error);
                break (text, code);
            }
            command.arg("--server-jar").arg(&target);
            server_jar = Some(target);
            continue;
        }
        break (text, code);
    };

    if code != Some(0) {
        let tail = tail_text(&text, 3000);
        log.error(
            "exit",
            &format!("код {code:?}, вывод: {tail}"),
        );
        return Err(format!(
            "Подготовка ресурсов завершилась с ошибкой (код {}).\n{}\nПодробности: {}",
            code.map(|value| value.to_string())
                .unwrap_or_else(|| "неизвестен".into()),
            tail,
            log.path().display()
        ));
    }
    log.info("exit", &format!("код 0, вывод: {}", tail_text(&text, 2000)));

    // Exit code 0 alone proves nothing: Horizon refuses to start without its own
    // resource manifest, so check what horizon-prepare actually produced.
    let (resources, minecraft, ready) = crate::horizon::prepared_resources(&root);
    let Some(resources) = resources else {
        let message = "horizon-prepare завершился с кодом 0, но папка ресурсов не появилась. Проверьте, что указана папка .minecraft с установленной версией Minecraft.".to_string();
        log.error("check", &message);
        return Err(message);
    };
    if !ready {
        let message = format!(
            "horizon-prepare завершился с кодом 0, но ресурсы неполные: в {} нет asset-index.json или horizon-resources.json.",
            resources.display()
        );
        log.error("check", &message);
        return Err(message);
    }
    let check = validate_resources(&resources);
    if !check.ok {
        let message = format!(
            "horizon-prepare завершился с кодом 0, но проверка нашла проблемы: {}\n{}",
            check.detail,
            check.missing.join("\n")
        );
        log.error("check", &message);
        return Err(message);
    }
    Ok(format!(
        "Ресурсы Minecraft {minecraft} готовы: {}. Проверено файлов: {}.",
        resources.display(),
        check.checked
    ))
}

/// What a prepared resource folder is missing. Horizon needs both manifests and
/// every asset listed in `asset-index.json`; the launcher used to accept the
/// presence of a single file and then the game refused to start.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCheck {
    pub ok: bool,
    pub minecraft: String,
    pub checked: u64,
    pub missing: Vec<String>,
    pub detail: String,
}

pub fn validate_resources(dir: &Path) -> ResourceCheck {
    let manifest = dir.join("horizon-resources.json");
    let minecraft = fs::read_to_string(&manifest)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value["minecraft"].as_str().map(str::to_string))
        .or_else(|| {
            dir.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "26.3".into());

    let mut missing: Vec<String> = Vec::new();
    if !manifest.is_file() {
        missing.push("horizon-resources.json".into());
    }
    let index_path = dir.join("asset-index.json");
    let mut checked = 0u64;
    let mut total = 0u64;
    if !index_path.is_file() {
        missing.push("asset-index.json".into());
    } else if let Ok(text) = fs::read_to_string(&index_path) {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(index) => {
                if let Some(objects) = index["objects"].as_object() {
                    for (name, object) in objects {
                        total += 1;
                        let size = object["size"].as_u64().unwrap_or(0);
                        let path = dir.join("assets/minecraft").join(name);
                        checked += 1;
                        let actual = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        if !path.is_file() || (size > 0 && actual != size) {
                            if missing.len() < 40 {
                                missing.push(format!("assets/minecraft/{name}"));
                            }
                        }
                    }
                } else {
                    missing.push("asset-index.json (нет списка objects)".into());
                }
            }
            Err(error) => missing.push(format!("asset-index.json (повреждён: {error})")),
        }
    }

    let ok = missing.is_empty();
    let detail = if ok {
        format!("Ресурсы {minecraft}: проверено {checked} файлов, всё на месте.")
    } else {
        format!(
            "Ресурсы {minecraft}: проверено {checked} из {total}, не хватает {} файлов.",
            missing.len()
        )
    };
    ResourceCheck {
        ok,
        minecraft,
        checked,
        missing,
        detail,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedResources {
    pub folder: String,
    pub minecraft: String,
    pub path: String,
    pub bytes: u64,
}

fn expand_home(value: &str) -> PathBuf {
    let value = value.trim();
    if value == "~" || value.starts_with("~/") {
        return paths::home_dir().join(value.trim_start_matches("~/"));
    }
    PathBuf::from(value)
}

fn has_index(dir: &Path) -> bool {
    dir.join("asset-index.json").is_file()
}

/// A folder brought from another computer can be any of:
/// the resource folder itself (`.../horizon-client/26.3`), its parent
/// (`.../horizon-client`), or a whole version folder (`.../.local/horizon-client/26.3`).
fn find_resources(source: &Path) -> Result<PathBuf, String> {
    if has_index(source) {
        return Ok(source.to_path_buf());
    }
    for container in [
        source.to_path_buf(),
        source.join("horizon-client"),
        source.join(".local/horizon-client"),
    ] {
        let Ok(entries) = fs::read_dir(&container) else {
            continue;
        };
        let mut ready: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && has_index(path))
            .collect();
        ready.sort();
        match ready.len() {
            1 => return Ok(ready.remove(0)),
            0 => {}
            _ => {
                let names: Vec<String> = ready
                    .iter()
                    .filter_map(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .collect();
                return Err(format!(
                    "В папке несколько наборов ресурсов ({}). Укажите нужный из них.",
                    names.join(", ")
                ));
            }
        }
    }
    // Archives usually keep the resource folder one level down, so look inside
    // the first level too when explaining what the player actually brought.
    let mut look: Vec<PathBuf> = vec![source.to_path_buf()];
    if let Ok(entries) = fs::read_dir(source) {
        look.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir()),
        );
    }
    if look
        .iter()
        .any(|dir| dir.join("instance.cfg").is_file() || dir.join("mmc-pack.json").is_file())
    {
        return Err(
            "Это папка или архив Prism Launcher: в нём настройки и список модов, но нет файлов Minecraft. Для игры нужна папка .local/horizon-client/26.3 (около 700 МБ) или папка .minecraft."
                .into(),
        );
    }
    if look
        .iter()
        .any(|dir| dir.join("versions").is_dir() || dir.join("assets").is_dir())
    {
        return Err(
            "Это папка .minecraft, а не готовые ресурсы Horizon. Укажите её в поле «Папка Minecraft Java Edition» и нажмите «Подготовить»."
                .into(),
        );
    }
    Err("В папке нет asset-index.json. Нужна готовая папка ресурсов: <версия>/.local/horizon-client/<Minecraft>.".into())
}

fn size_of(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| size_of(&entry.path()))
        .sum()
}



/// The folder a player can simply copy a prepared resource folder into on a new
/// computer; GERA picks it up on the next launch.
pub fn drop_dir() -> PathBuf {
    let dir = paths::data_home().join("gera-launcher/import");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// A zip unpacked for the import; removed when the import is over.
struct TempDir(Option<PathBuf>);

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Some(dir) = &self.0 {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
}


fn temp_unpack(source: &Path, temp: &mut TempDir) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let into = drop_dir().join(format!(".unpack-{stamp}"));
    let _ = fs::remove_dir_all(&into);
    crate::sysutil::extract_zip(source, &into)?;
    temp.0 = Some(into.clone());
    Ok(into)
}

/// Import whatever prepared resource folder (or zip) lies in `drop_dir()` into
/// `folder`. `Ok(None)` means the drop folder is empty.
pub fn import_from_drop(folder: &str) -> Result<Option<ImportedResources>, String> {
    let drop = drop_dir();
    let Ok(entries) = fs::read_dir(&drop) else {
        return Ok(None);
    };
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    let mut temps: Vec<TempDir> = Vec::new();
    let mut zip_errors: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if let Ok(dir) = find_resources(&path) {
                found.push((name, dir));
            }
        } else if path.is_file() && is_zip(&path) {
            let mut temp = TempDir(None);
            match temp_unpack(&path, &mut temp).and_then(|dir| find_resources(&dir)) {
                Ok(dir) => {
                    found.push((name, dir));
                    temps.push(temp);
                }
                Err(error) => zip_errors.push(format!("{name}: {error}")),
            }
        }
    }
    found.sort_by(|left, right| left.1.cmp(&right.1));
    found.dedup_by(|left, right| left.1 == right.1);
    match found.len() {
        0 => match zip_errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(None),
        },
        1 => {
            let (_, source) = found.remove(0);
            import_prepared(folder, &source).map(Some)
        }
        _ => {
            let names: Vec<String> = found.into_iter().map(|(name, _)| name).collect();
            Err(format!(
                "В папке переноса несколько наборов ресурсов ({}). Оставьте один.",
                names.join(", ")
            ))
        }
    }
}

/// Copy a prepared resource folder (asset-index.json inside) into a client
/// version. Game files still come from the player's own copy of Minecraft; this
/// only saves running `horizon-prepare` again on another computer.
pub fn import_resources(folder: &str, source: &str) -> Result<ImportedResources, String> {
    if source.trim().is_empty() {
        return Err("Укажите папку с готовыми ресурсами или zip-архив.".into());
    }
    let path = expand_home(source);
    let mut log = Log::open(Kind::Import);
    log.info("source", &format!("импорт из {}", path.display()));

    // A full `.minecraft` folder is accepted too: GERA runs horizon-prepare on
    // it instead of asking the player to find the prepared subfolder.
    if path.is_dir() && looks_like_minecraft(&path) {
        log.info("prepare", "источник — папка .minecraft, запускаем horizon-prepare");
        prepare(folder, &path.to_string_lossy(), None)?;
        let root = expand_home(folder);
        let root = if root.is_absolute() {
            root
        } else {
            paths::clients_dir().join(root)
        };
        let (resources, minecraft, _ready) = crate::horizon::prepared_resources(&root);
        let resources = resources.ok_or_else(|| {
            "horizon-prepare не создал папку ресурсов. Подробности: prepare.log".to_string()
        })?;
        return Ok(ImportedResources {
            folder: root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            minecraft,
            path: resources.to_string_lossy().into_owned(),
            bytes: size_of(&resources),
        });
    }

    let mut temp = TempDir(None);
    let source_dir = if path.is_dir() {
        find_resources(&path)?
    } else if path.is_file() && is_zip(&path) {
        let unpacked = temp_unpack(&path, &mut temp)?;
        find_resources(&unpacked).map_err(|error| format!("В архиве нет готовых ресурсов. {error}"))?
    } else if path.is_file() {
        return Err("Это не папка и не zip-архив. Укажите папку с ресурсами.".into());
    } else {
        return Err(format!("Папка не найдена: {}", path.display()));
    };
    import_prepared(folder, &source_dir)
}

/// A folder that holds an official `.minecraft` install: versions/ or assets/
/// on the top level. GERA can prepare Horizon resources straight from it.
fn looks_like_minecraft(dir: &Path) -> bool {
    dir.join("versions").is_dir() || dir.join("assets").is_dir()
}

fn import_prepared(folder: &str, source_dir: &Path) -> Result<ImportedResources, String> {
    let mut log = Log::open(Kind::Import);
    let source_check = validate_resources(source_dir);
    if !source_check.ok {
        let message = format!(
            "В папке неполные ресурсы: {}\nНе хватает:\n{}",
            source_check.detail,
            source_check.missing.join("\n")
        );
        log.error("check", &message);
        return Err(message);
    }
    log.info(
        "check",
        &format!("источник проверен: {}", source_check.detail),
    );

    let root = expand_home(folder);
    let root = if root.is_absolute() {
        root
    } else {
        paths::clients_dir().join(root)
    };
    if !root.join(client_binary()).is_file() {
        return Err(format!(
            "В папке клиента нет {}: {}.",
            client_binary(),
            root.display()
        ));
    }
    let minecraft = source_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "26.3".into());

    // Native builds keep `.local` as a symlink to their base version, so write
    // through it to the real folder instead of replacing the link. The `\\?\`
    // prefix Windows adds is stripped: the path goes to the game and the UI.
    let local = root.join(".local");
    let local = crate::sysutil::plain_path(&fs::canonicalize(&local).unwrap_or(local));
    let resources_root = local.join("horizon-client");
    let target = resources_root.join(&minecraft);
    if has_index(&target) {
        let current = validate_resources(&target);
        if current.ok {
            return Err(format!(
                "Ресурсы Minecraft {minecraft} уже есть и целые: {}.",
                target.display()
            ));
        }
        log.warn(
            "replace",
            &format!("старые ресурсы неполные, заменяем: {}", current.detail),
        );
    }

    let bytes = size_of(source_dir);
    let free = crate::sysutil::free_bytes(&resources_root);
    if free > 0 && free < bytes + 50 * 1024 * 1024 {
        return Err(format!(
            "Не хватает места: нужно {} ГБ, свободно {} ГБ.",
            (bytes as f64 / 1024.0 / 1024.0 / 1024.0 * 10.0).round() / 10.0,
            (free as f64 / 1024.0 / 1024.0 / 1024.0 * 10.0).round() / 10.0
        ));
    }

    fs::create_dir_all(&resources_root).map_err(|e| e.to_string())?;
    let staging = resources_root.join(format!(".import-{minecraft}"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    crate::sysutil::copy_tree(source_dir, &staging).map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        format!("Не удалось скопировать ресурсы: {e}")
    })?;
    let copied = validate_resources(&staging);
    if !copied.ok {
        let _ = fs::remove_dir_all(&staging);
        let message = format!(
            "Копирование испортило ресурсы: {}. Импорт отменён. Проверьте свободное место и права на папку.",
            copied.detail
        );
        log.error("copy", &message);
        return Err(message);
    }

    let backup = resources_root.join(format!(".old-{minecraft}"));
    if target.exists() {
        let _ = fs::remove_dir_all(&backup);
        fs::rename(&target, &backup).map_err(|e| {
            let _ = fs::remove_dir_all(&staging);
            format!("Не удалось заменить неполные ресурсы: {e}")
        })?;
    }
    fs::rename(&staging, &target).map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        format!("Не удалось завершить импорт: {e}")
    })?;
    let _ = fs::remove_dir_all(&backup);
    let final_check = validate_resources(&target);
    log.info("done", &final_check.detail);

    Ok(ImportedResources {
        folder: root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        minecraft,
        path: target.to_string_lossy().into_owned(),
        bytes,
    })
}

fn tail_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let skip = trimmed.chars().count() - limit;
    trimmed.chars().skip(skip).collect()
}

pub fn client_dir(folder: &str) -> PathBuf {
    paths::clients_dir().join(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gera-clients-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn prepared(dir: &Path, with_asset: bool) {
        fs::create_dir_all(dir.join("assets/minecraft/icons")).unwrap();
        fs::write(
            dir.join("horizon-resources.json"),
            b"{\"minecraft\":\"26.3\"}",
        )
        .unwrap();
        fs::write(
            dir.join("asset-index.json"),
            b"{\"objects\":{\"icons/icon_16x16.png\":{\"hash\":\"5ff04807c356f1beed0b86ccf659b44b9983e3fa\",\"size\":4}}}",
        )
        .unwrap();
        if with_asset {
            fs::write(dir.join("assets/minecraft/icons/icon_16x16.png"), b"data").unwrap();
        }
    }

    #[test]
    fn complete_resources_pass_the_check() {
        let dir = temp_dir("pleno");
        prepared(&dir, true);
        let check = validate_resources(&dir);
        assert!(check.ok, "{:?}", check.missing);
        assert_eq!(check.minecraft, "26.3");
        assert_eq!(check.checked, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_manifest_is_reported() {
        let dir = temp_dir("no-manifest");
        prepared(&dir, true);
        fs::remove_file(dir.join("horizon-resources.json")).unwrap();
        let check = validate_resources(&dir);
        assert!(!check.ok);
        assert!(check.missing.iter().any(|item| item.contains("horizon-resources.json")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_asset_is_named() {
        let dir = temp_dir("no-asset");
        prepared(&dir, false);
        let check = validate_resources(&dir);
        assert!(!check.ok);
        assert!(check
            .missing
            .iter()
            .any(|item| item.contains("icons/icon_16x16.png")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_manifest_is_reported() {
        let dir = temp_dir("broken");
        prepared(&dir, true);
        fs::write(dir.join("asset-index.json"), b"not json").unwrap();
        let check = validate_resources(&dir);
        assert!(!check.ok);
        assert!(check.missing.iter().any(|item| item.contains("повреждён")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_manifest_is_parsed_from_the_preparer_message() {
        let root = Path::new("/tmp/gera-client");
        let line = "error: /home/u/.minecraft/versions/26.3/26.3.json is not a 26.3 version file: save https://piston-meta.mojang.com/v1/packages/11f1aefe473eefba0af62e3286c33b692807b2eb/26.3.json as /tmp/gera-client/.local/minecraft/26.3/manifest.json and prepare again.";
        let (url, path) = pinned_manifest(line, root).unwrap();
        assert!(url.ends_with("/26.3.json"));
        assert!(path.ends_with("manifest.json"), "{path}");
        assert_eq!(
            manifest_digest(&url).unwrap(),
            "11f1aefe473eefba0af62e3286c33b692807b2eb"
        );
        // A path outside the client folder is never used.
        assert!(pinned_manifest(line, Path::new("/other")).is_none());
        // Unrelated output is ignored.
        assert!(pinned_manifest("ошибка сети", root).is_none());
    }

    #[test]
    fn minecraft_folder_is_recognised() {
        let dir = temp_dir("minecraft");
        assert!(!looks_like_minecraft(&dir));
        fs::create_dir_all(dir.join("versions/26.3")).unwrap();
        assert!(looks_like_minecraft(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn zip_with_prepared_resources_is_found() {
        let dir = temp_dir("zip");
        let unpacked = dir.join("unpacked");
        prepared(&unpacked.join("horizon-client/26.3"), true);
        let archive = dir.join("resources.zip");
        {
            let file = fs::File::create(&archive).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            for path in ["horizon-client/26.3/asset-index.json", "horizon-client/26.3/horizon-resources.json"] {
                writer
                    .start_file(path, zip::write::SimpleFileOptions::default())
                    .unwrap();
                use std::io::Write;
                writer.write_all(b"{\"objects\":{}}").unwrap();
            }
            writer.finish().unwrap();
        }
        let target = dir.join("extracted");
        crate::sysutil::extract_zip(&archive, &target).unwrap();
        let found = find_resources(&target).unwrap();
        assert_eq!(found, target.join("horizon-client/26.3"));
        let _ = fs::remove_dir_all(&dir);
    }
}
