use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::net;
use crate::paths::Paths;

/// GitHub repository that hosts GERA Launcher releases.
/// The CI workflow publishes `gera-launcher.exe` (Windows) and
/// `gera-launcher-linux` (Linux) plus matching `.sha256` files.
/// An empty repository owner disables update checks.
pub const REPO: &str = "dimagera05092013-cyber/gera-launcher";

/// Current release of this build, as written in Cargo.toml.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn repo() -> String {
    std::env::var("GERA_UPDATE_REPO")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| REPO.to_string())
}

fn configured() -> bool {
    let repo = repo();
    !repo.is_empty() && !repo.starts_with("OWNER/")
}

/// File name of the release asset for this platform.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(windows) {
        Some("gera-launcher.exe")
    } else if cfg!(target_os = "linux") {
        Some("gera-launcher-linux")
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: String,
    pub url: String,
    pub asset: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: u64,
}

fn release_json() -> Result<Value, String> {
    let repo = repo();
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let text = net::fetch_text(&url, 30)?;
    serde_json::from_str(&text).map_err(|e| format!("Некорректный ответ GitHub: {e}"))
}

fn asset_url(release: &Value, name: &str) -> Option<String> {
    release["assets"]
        .as_array()?
        .iter()
        .find(|asset| asset["name"] == name)
        .and_then(|asset| asset["browser_download_url"].as_str())
        .map(str::to_string)
}

fn asset_size(release: &Value, name: &str) -> u64 {
    release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|asset| asset["name"] == name))
        .and_then(|asset| asset["size"].as_u64())
        .unwrap_or(0)
}

fn tag_version(release: &Value) -> Option<String> {
    release["tag_name"]
        .as_str()
        .map(|tag| tag.trim_start_matches('v').trim().to_string())
        .filter(|tag| !tag.is_empty())
}

/// Numeric part-by-part comparison: 0.9.0 < 0.10.0 < 1.0.0.
fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(value: &str) -> Vec<u64> {
        value
            .trim()
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .take(3)
            .map(|part| {
                part.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    }
    let (latest, current) = (parts(latest), parts(current));
    for index in 0..3 {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

/// Ask GitHub for the latest release. Returns `None` when this build is the
/// newest one, when updates are not configured, or on an unexpected platform.
pub fn check() -> Result<Option<UpdateInfo>, String> {
    // Development hook: lets the updater UI be checked without a real release.
    if let Ok(mock) = std::env::var("GERA_UPDATE_MOCK") {
        if !mock.trim().is_empty() {
            return Ok(Some(UpdateInfo {
                version: mock.trim().to_string(),
                current: current_version().to_string(),
                notes: "• Новый экран обновлений\n• Исправлены мелкие ошибки\n• Ускорен запуск".into(),
                url: String::new(),
                asset: asset_name().unwrap_or("gera-launcher").to_string(),
                size: 4_900_000,
            }));
        }
    }
    if !configured() {
        return Ok(None);
    }
    let Some(asset) = asset_name() else {
        return Ok(None);
    };
    let release = release_json()?;
    let Some(version) = tag_version(&release) else {
        return Err("В последнем релизе GitHub нет номера версии.".into());
    };
    let current = current_version().to_string();
    if !is_newer(&version, &current) {
        return Ok(None);
    }
    if asset_url(&release, asset).is_none() {
        return Err(format!("В релизе {version} нет файла {asset}."));
    }
    Ok(Some(UpdateInfo {
        version,
        current,
        notes: release["body"].as_str().unwrap_or("").trim().to_string(),
        url: release["html_url"].as_str().unwrap_or("").to_string(),
        asset: asset.to_string(),
        size: asset_size(&release, asset),
    }))
}

fn update_dir() -> PathBuf {
    Paths::new().data.join("update")
}

fn checksum_path(asset: &str) -> PathBuf {
    update_dir().join(format!("{asset}.sha256"))
}

fn download_with_progress(
    app: &AppHandle,
    url: &str,
    destination: &Path,
    expected_size: u64,
) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(destination);
    let mut command = Command::new("curl");
    command
        .args([
            "-fL",
            "--proto",
            "=https",
            "--retry",
            "2",
            "--connect-timeout",
            "20",
            "--silent",
            "--show-error",
            "-o",
        ])
        .arg(destination)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = crate::sysutil::quiet(&mut command)
        .spawn()
        .map_err(|e| format!("curl недоступен: {e}"))?;

    let mut last = 0u64;
    loop {
        if let Ok(meta) = fs::metadata(destination) {
            let downloaded = meta.len();
            if downloaded != last {
                last = downloaded;
                let _ = app.emit(
                    "update-progress",
                    UpdateProgress {
                        downloaded,
                        total: expected_size.max(downloaded),
                    },
                );
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let mut stderr = String::new();
                    if let Some(mut pipe) = child.stderr.take() {
                        use std::io::Read;
                        let _ = pipe.read_to_string(&mut stderr);
                    }
                    let _ = fs::remove_file(destination);
                    return Err(format!(
                        "Загрузка не удалась: {}",
                        if stderr.trim().is_empty() {
                            format!("код {}", status.code().unwrap_or(-1))
                        } else {
                            stderr.trim().to_string()
                        }
                    ));
                }
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(150)),
            Err(error) => {
                let _ = fs::remove_file(destination);
                return Err(format!("Загрузка прервана: {error}"));
            }
        }
    }

    let size = fs::metadata(destination).map(|m| m.len()).unwrap_or(0);
    let _ = app.emit(
        "update-progress",
        UpdateProgress {
            downloaded: size,
            total: expected_size.max(size),
        },
    );
    Ok(())
}

/// Download the newest release into GERA's data folder, verify its SHA-256 and
/// keep it ready for [`install`].
pub fn download(app: &AppHandle, version: &str) -> Result<PathBuf, String> {
    if !configured() {
        return Err("Обновления через GitHub не настроены.".into());
    }
    let asset = asset_name().ok_or("Для этой системы обновления не поддерживаются.")?;
    let release = release_json()?;
    let Some(latest) = tag_version(&release) else {
        return Err("В релизе GitHub нет номера версии.".into());
    };
    if latest != version {
        return Err(format!(
            "Вышел релиз {latest}, обновите список — и попробуйте снова."
        ));
    }
    let url = asset_url(&release, asset).ok_or_else(|| format!("В релизе нет файла {asset}."))?;
    let sums_url = asset_url(&release, &format!("{asset}.sha256"))
        .ok_or("В релизе нет контрольной суммы — обновление отменено.")?;

    let destination = update_dir().join(asset);
    download_with_progress(app, &url, &destination, asset_size(&release, asset))?;

    let sums = net::fetch_text(&sums_url, 30)?;
    let expected =
        net::checksum_for(&sums, asset).ok_or("Не удалось прочитать контрольную сумму релиза.")?;
    let actual = net::sha256_file(&destination)?;
    if actual != expected {
        let _ = fs::remove_file(&destination);
        return Err("Файл обновления повреждён (SHA-256 не совпал).".into());
    }
    fs::write(checksum_path(asset), format!("{actual}  {asset}\n"))
        .map_err(|e| format!("Не удалось сохранить контрольную сумму: {e}"))?;
    Ok(destination)
}

fn spawn_detached(path: &Path) -> Result<(), String> {
    let mut command = Command::new(path);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::sysutil::quiet(&mut command)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось перезапустить лаунчер: {e}"))
}

#[cfg(windows)]
fn swap_and_restart(staged: &Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| format!("Не удалось найти exe: {e}"))?;
    let file_name = current
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "gera-launcher.exe".into());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = current.with_file_name(format!("{file_name}.{stamp}.old"));

    // A running exe cannot be overwritten, but Windows allows renaming it.
    let _ = fs::remove_file(&backup);
    fs::rename(&current, &backup).map_err(|e| {
        format!("Не удалось заменить файл лаунчера: {e}. Если GERA лежит в защищённой папке, перенесите exe в свою папку.")
    })?;
    if let Err(error) = fs::copy(staged, &current) {
        let _ = fs::rename(&backup, &current);
        return Err(format!("Не удалось записать новый файл: {error}"));
    }
    spawn_detached(&current)
}

#[cfg(unix)]
fn swap_and_restart(staged: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let current = std::env::current_exe().map_err(|e| format!("Не удалось найти файл: {e}"))?;
    let file_name = current
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "gera-launcher".into());
    let temp = current.with_file_name(format!(".{file_name}.new"));

    // A running binary cannot be written to, but it can be replaced by rename.
    fs::copy(staged, &temp).map_err(|e| format!("Не удалось подготовить файл: {e}"))?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Не удалось выставить права: {e}"))?;
    if let Err(error) = fs::rename(&temp, &current) {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "Не удалось заменить файл лаунчера: {error}. Проверьте права на папку."
        ));
    }
    spawn_detached(&current)
}

/// Replace the running executable with the downloaded one and start it again.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let asset = asset_name().ok_or("Для этой системы обновления не поддерживаются.")?;
    let staged = update_dir().join(asset);
    if !staged.is_file() {
        return Err("Обновление ещё не скачано.".into());
    }
    let expected = fs::read_to_string(checksum_path(asset))
        .map_err(|_| "Нет контрольной суммы обновления.".to_string())?;
    let expected = expected
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let actual = net::sha256_file(&staged)?;
    if expected.len() != 64 || expected != actual {
        return Err("Файл обновления изменился после загрузки. Скачайте заново.".into());
    }

    swap_and_restart(&staged)?;
    // Give the new process a moment to start before this one disappears.
    std::thread::sleep(Duration::from_millis(300));
    app.exit(0);
    let _ = fs::remove_file(&staged);
    let _ = fs::remove_file(checksum_path(asset));
    Ok(())
}

/// Remove the previous executable left behind by the Windows swap.
pub fn cleanup_old_binary() {
    #[cfg(windows)]
    {
        if let Ok(current) = std::env::current_exe() {
            if let (Some(parent), Some(name)) = (current.parent(), current.file_name()) {
                let prefix = format!("{}.", name.to_string_lossy());
                if let Ok(entries) = fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let file_name = entry.file_name().to_string_lossy().into_owned();
                        if file_name.starts_with(&prefix) && file_name.ends_with(".old") {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }
    #[cfg(unix)]
    {
        if let Ok(current) = std::env::current_exe() {
            if let Some(name) = current.file_name() {
                let temp = current.with_file_name(format!(".{}.new", name.to_string_lossy()));
                let _ = fs::remove_file(temp);
            }
        }
    }
}

#[tauri::command]
pub async fn update_check() -> Result<Option<UpdateInfo>, String> {
    tauri::async_runtime::spawn_blocking(check)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_download(app: AppHandle, version: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || download(&app, &version).map(|_| ()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || install(&app))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.9.1", "0.9.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.9.0"));
        assert!(is_newer("v1.2.3", "1.2.2"));
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("v0.1.0", "0.9.0"));
        assert!(!is_newer("0.9", "0.9.0"));
    }

    #[test]
    fn tag_is_parsed() {
        let release = serde_json::json!({"tag_name": "v0.9.0"});
        assert_eq!(tag_version(&release).as_deref(), Some("0.9.0"));
        let empty = serde_json::json!({"tag_name": ""});
        assert_eq!(tag_version(&empty), None);
    }

    #[test]
    fn assets_are_found() {
        let release = serde_json::json!({
            "assets": [
                {"name": "gera-launcher.exe", "browser_download_url": "https://example.com/exe", "size": 4900000},
                {"name": "gera-launcher.exe.sha256", "browser_download_url": "https://example.com/sums", "size": 100}
            ]
        });
        assert_eq!(
            asset_url(&release, "gera-launcher.exe").as_deref(),
            Some("https://example.com/exe")
        );
        assert_eq!(asset_size(&release, "gera-launcher.exe"), 4900000);
        assert!(asset_url(&release, "gera-launcher-linux").is_none());
    }
}
