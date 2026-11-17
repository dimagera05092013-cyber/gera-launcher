use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::net;
use crate::paths;
use crate::store::now_ms;

const INDEX_URL: &str = "https://horizon-rust.github.io/horizon-mods/index.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModVersion {
    pub version: String,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub changelog: String,
    #[serde(default)]
    pub api: String,
    #[serde(default)]
    pub game: String,
    pub file: ModFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModProject {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub downloads: u64,
    /// GERA's Russian summary, bundled with the launcher.
    #[serde(default)]
    pub summary_ru: String,
    /// GERA's Russian description, when one was written.
    #[serde(default)]
    pub description_ru: String,
    pub versions: Vec<ModVersion>,
}

#[derive(Debug, Deserialize)]
struct Index {
    #[serde(default)]
    projects: Vec<ModProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub file: String,
    pub project_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub installed_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ModsState {
    #[serde(default)]
    profiles: BTreeMap<String, Vec<InstalledMod>>,
}

fn state_file() -> PathBuf {
    paths::data_home().join("gera-launcher/mods-state.json")
}

fn load_state() -> ModsState {
    fs::read_to_string(state_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_state(state: &ModsState) -> Result<(), String> {
    let file = state_file();
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    let temporary = file.with_extension("json.tmp");
    fs::write(&temporary, text).map_err(|e| e.to_string())?;
    fs::rename(&temporary, &file).map_err(|e| e.to_string())
}

fn cache_file() -> PathBuf {
    paths::data_home().join("gera-launcher/cache/horizon-mods-index.json")
}

/// The official Horizon Mods registry, fetched directly. Results are cached
/// for ten minutes; the UI can ask for a refresh.
pub fn catalog(force: bool) -> Result<Vec<ModProject>, String> {
    let cache = cache_file();
    if !force {
        if let Ok(meta) = fs::metadata(&cache) {
            if let Ok(modified) = meta.modified() {
                if modified.elapsed().map(|d| d.as_secs() < 600).unwrap_or(false) {
                    if let Ok(text) = fs::read_to_string(&cache) {
                        if let Ok(index) = serde_json::from_str::<Index>(&text) {
                            return Ok(trim(index.projects));
                        }
                    }
                }
            }
        }
    }
    let text = net::fetch_text(INDEX_URL, 45)?;
    let index: Index =
        serde_json::from_str(&text).map_err(|e| format!("Некорректный индекс реестра: {e}"))?;
    if let Some(parent) = cache.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&cache, &text);
    Ok(trim(index.projects))
}

#[derive(Debug, Deserialize)]
struct RuText {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: String,
}

/// Russian text bundled with GERA, keyed by registry project id. Kept offline:
/// the launcher never sends the catalog to a translation service.
fn translations() -> &'static HashMap<String, RuText> {
    static RU: OnceLock<HashMap<String, RuText>> = OnceLock::new();
    RU.get_or_init(|| serde_json::from_str(include_str!("mods_ru.json")).unwrap_or_default())
}

fn trim(mut projects: Vec<ModProject>) -> Vec<ModProject> {
    let ru = translations();
    for project in &mut projects {
        if let Some(text) = ru.get(&project.id) {
            project.summary_ru = text.summary.clone();
            project.description_ru = text.description.clone();
        }
        if project.description.chars().count() > 1200 {
            let text: String = project.description.chars().take(1200).collect();
            project.description = format!("{text}…");
        }
        for version in &mut project.versions {
            if version.changelog.chars().count() > 400 {
                let text: String = version.changelog.chars().take(400).collect();
                version.changelog = format!("{text}…");
            }
        }
    }
    projects
}

fn project<'a>(projects: &'a [ModProject], id: &str) -> Option<&'a ModProject> {
    projects.iter().find(|p| p.id == id)
}

/// Install a content mod (`.hmod`) into the profile's game folder. Native
/// `.hnmod` bundles need Horizon source compilation and are refused.
pub fn install(
    profile_id: &str,
    game_dir: &Path,
    project_id: &str,
    version: Option<&str>,
) -> Result<InstalledMod, String> {
    let projects = catalog(false)?;
    let project =
        project(&projects, project_id).ok_or_else(|| "Мод не найден в реестре.".to_string())?;
    if project.kind != "mod" {
        return Err(if project.kind == "native-mod" {
            "Это нативный мод (.hnmod). Для него нужна компиляция исходников Horizon, поэтому GERA пока не устанавливает такие моды.".into()
        } else {
            format!("Тип «{}» пока не устанавливается через GERA.", project.kind)
        });
    }
    let selected = match version {
        Some(tag) => project
            .versions
            .iter()
            .find(|v| v.version == tag)
            .ok_or_else(|| "Версия мода не найдена.".to_string())?,
        None => project
            .versions
            .first()
            .ok_or_else(|| "У мода нет файлов.".to_string())?,
    };
    if !selected.file.name.ends_with(".hmod") {
        return Err("Файл мода не является .hmod.".into());
    }
    if selected.file.url.trim().is_empty() {
        return Err("В реестре нет ссылки на файл.".into());
    }

    let cache = paths::data_home()
        .join("gera-launcher/cache/downloads/mods")
        .join(&selected.file.name);
    net::download(&selected.file.url, &cache)?;
    let digest = net::sha256_file(&cache)?;
    if !digest.eq_ignore_ascii_case(&selected.file.sha256) {
        let _ = fs::remove_file(&cache);
        return Err("Контрольная сумма мода не совпала — установка отменена.".into());
    }

    let mods_dir = game_dir.join("horizon-mods");
    fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    let destination = mods_dir.join(&selected.file.name);
    if destination.exists() {
        backup_file(&destination)?;
    }
    fs::copy(&cache, &destination).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&cache);

    let installed = InstalledMod {
        file: selected.file.name.clone(),
        project_id: Some(project.id.clone()),
        name: Some(project.name.clone()),
        version: Some(selected.version.clone()),
        sha256: Some(digest),
        installed_at: now_ms(),
    };
    let mut state = load_state();
    let list = state.profiles.entry(profile_id.to_string()).or_default();
    list.retain(|m| m.file != installed.file);
    list.push(installed.clone());
    save_state(&state)?;
    Ok(installed)
}

/// Remove a mod from the game folder, keeping a backup copy in GERA's data.
pub fn remove(profile_id: &str, game_dir: &Path, file: &str) -> Result<(), String> {
    let name = Path::new(file)
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .filter(|v| v.ends_with(".hmod"))
        .ok_or_else(|| "Некорректное имя файла мода.".to_string())?;
    let target = game_dir.join("horizon-mods").join(&name);
    if target.is_file() {
        backup_file(&target)?;
    }
    let mut state = load_state();
    if let Some(list) = state.profiles.get_mut(profile_id) {
        list.retain(|m| m.file != name);
    }
    save_state(&state)?;
    Ok(())
}

fn backup_file(file: &Path) -> Result<PathBuf, String> {
    let backups = paths::data_home().join("gera-launcher/backups/mods");
    fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
    let name = file
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mod.hmod".into());
    let mut destination = backups.join(format!("{}-{name}", now_ms()));
    let mut counter = 1;
    while destination.exists() {
        destination = backups.join(format!("{}-{counter}-{name}", now_ms()));
        counter += 1;
    }
    // Copy + remove instead of rename: the game folder may live on another
    // filesystem than GERA's data, where rename fails with EXDEV.
    fs::copy(file, &destination).map_err(|e| format!("Не удалось сохранить копию мода: {e}"))?;
    fs::remove_file(file).map_err(|e| format!("Не удалось заменить старый файл мода: {e}"))?;
    Ok(destination)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinMod {
    pub id: String,
    pub title: String,
    pub enabled: bool,
}

fn builtin_title(id: &str) -> String {
    match id {
        "appleskin" => "AppleSkin",
        "chatheads" => "Chat Heads",
        "fullbright" => "Fullbright",
        "minimap" => "Minimap",
        "mod_menu" => "Mod Menu",
        "mousetricks" => "Mouse Tricks",
        "shulker_peek" => "ShulkerPeek",
        "smartf3" => "SmartF3",
        "zoom" => "Zoom",
        other => other,
    }
    .to_string()
}

/// Native mods built into the client. They are not `.hmod` files, so the
/// installed list cannot see them; Horizon keeps their switches in
/// `horizon-mods.txt` as `mod.<id>:true|false`.
pub fn builtin(game_dir: &Path) -> Vec<BuiltinMod> {
    let Ok(text) = fs::read_to_string(game_dir.join("horizon-mods.txt")) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Some(id) = key.strip_prefix("mod.") else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        result.push(BuiltinMod {
            id: id.to_string(),
            title: builtin_title(id),
            enabled: value.trim().eq_ignore_ascii_case("true"),
        });
    }
    result.sort_by(|a, b| a.title.cmp(&b.title));
    result
}

#[cfg(test)]
mod builtin_tests {
    use super::*;

    #[test]
    fn reads_builtin_mod_switches() {
        let dir = std::env::temp_dir().join(format!("gera-builtin-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(
            dir.join("horizon-mods.txt"),
            "mod.zoom:true\nmod.minimap:false\nkey.zoom.zoom:key.mouse.5\n",
        )
        .unwrap();
        let list = builtin(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "Minimap");
        assert!(!list[0].enabled);
        assert_eq!(list[1].title, "Zoom");
        assert!(list[1].enabled);
    }
}

/// Files actually present in the profile's mod folder, enriched with GERA's
/// install metadata. Manually copied mods are shown too.
pub fn installed(profile_id: &str, game_dir: &Path) -> Vec<InstalledMod> {
    let state = load_state();
    let known = state
        .profiles
        .get(profile_id)
        .cloned()
        .unwrap_or_default();
    let mut result = Vec::new();
    let mods_dir = game_dir.join("horizon-mods");
    if let Ok(entries) = fs::read_dir(&mods_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path
                .file_name()
                .map(|v| v.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !name.ends_with(".hmod") {
                continue;
            }
            if let Some(found) = known.iter().find(|m| m.file == name) {
                result.push(found.clone());
            } else {
                result.push(InstalledMod {
                    file: name,
                    project_id: None,
                    name: None,
                    version: None,
                    sha256: None,
                    installed_at: 0,
                });
            }
        }
    }
    result.sort_by(|a, b| a.file.cmp(&b.file));
    result
}

