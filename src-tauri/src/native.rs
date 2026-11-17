//! GERA's own builder for native `.hnmod` mods.
//!
//! A native mod is not installed like a `.hmod`: it rewrites Horizon's own
//! sources, so the game has to be compiled again. GERA repeats the steps the
//! official launcher takes, without requiring it:
//!
//! 1. download the exact Horizon revision the mod pins (GitHub archive),
//! 2. build `horizon-mixins` at the pinned revision,
//! 3. `horizon-mixins apply` the mod onto a copy of those sources,
//! 4. `cargo build -p horizon-client` that copy,
//! 5. put the binary in GERA's clients folder and reuse the base client's
//!    prepared resources through a symlink.
//!
//! Only mods made of mixin bundles are supported: trait-API bundles (`[api]`)
//! need Horizon's API generator, which GERA does not build yet.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use crate::mods::{self, ModProject, ModVersion};
use crate::net;
use crate::paths;
use crate::store;

const HORIZON_REPO: &str = "horizon-rust/horizon";
const MIXINS_REPO: &str = "horizon-rust/horizon-mixins";
/// The revision the official launcher uses when a bundle names none.
/// A cold native build needs the sources, the crates and the target folder.
const MIN_FREE_BYTES: u64 = 12 * 1024 * 1024 * 1024;
const STEPS: u32 = 8;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeBuild {
    pub folder: String,
    pub project_id: String,
    pub name: String,
    pub version: String,
    pub horizon: String,
    pub mixins: String,
    pub base: String,
    pub game: String,
    pub built_at: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeJob {
    pub running: bool,
    pub step: u32,
    pub total: u32,
    pub stage: String,
    pub detail: String,
    pub log: Vec<String>,
    pub error: Option<String>,
    pub folder: Option<String>,
    pub project: Option<String>,
    pub compile_done: Option<u32>,
    pub compile_total: Option<u32>,
    pub started_at: u64,
    pub finished_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeStatus {
    pub rustc: Option<String>,
    pub cargo: Option<String>,
    pub unzip: bool,
    pub disk_free_gb: f64,
    pub work_dir: String,
    pub job: NativeJob,
    pub builds: Vec<NativeBuild>,
}

static JOB: OnceLock<Mutex<NativeJob>> = OnceLock::new();
static CANCEL: AtomicBool = AtomicBool::new(false);
static CHILD: Mutex<Option<u32>> = Mutex::new(None);

fn job_lock() -> MutexGuard<'static, NativeJob> {
    JOB.get_or_init(|| Mutex::new(NativeJob::default()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn child_lock() -> MutexGuard<'static, Option<u32>> {
    CHILD.lock().unwrap_or_else(|error| error.into_inner())
}

fn log(line: impl Into<String>) {
    let line = line.into();
    let mut job = job_lock();
    job.log.push(line);
    if job.log.len() > 400 {
        let excess = job.log.len() - 400;
        job.log.drain(0..excess);
    }
}

fn set_step(step: u32, stage: &str, detail: impl Into<String>) {
    let mut job = job_lock();
    job.step = step;
    job.stage = stage.to_string();
    job.detail = detail.into();
    if step >= 6 {
        job.compile_done = None;
        job.compile_total = None;
    }
}

fn set_compile(done: u32, total: u32) {
    let mut job = job_lock();
    job.compile_done = Some(done);
    job.compile_total = Some(total);
}

fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

fn kill_child() {
    if let Some(pid) = *child_lock() {
        crate::sysutil::kill_tree(pid);
    }
}

// ---------------------------------------------------------------------------
// Paths

fn work_dir() -> PathBuf {
    paths::data_home().join("gera-launcher/native")
}

fn cache_dir() -> PathBuf {
    work_dir().join("cache")
}

fn source_dir() -> PathBuf {
    work_dir().join("source")
}

fn tools_dir() -> PathBuf {
    work_dir().join("tools")
}

fn stage_dir() -> PathBuf {
    work_dir().join("stage")
}

fn target_dir() -> PathBuf {
    work_dir().join("target")
}

fn mods_dir() -> PathBuf {
    work_dir().join("mods")
}

fn ensure_dirs() -> Result<(), String> {
    for dir in [
        work_dir(),
        cache_dir(),
        source_dir(),
        tools_dir(),
        stage_dir(),
        target_dir(),
        mods_dir(),
    ] {
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Environment checks

fn version_of(binary: &str) -> Option<String> {
    let mut command = Command::new(binary);
    command.arg("--version");
    let output = crate::sysutil::quiet(&mut command)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}


fn gb(bytes: u64) -> f64 {
    (bytes as f64 / (1024.0 * 1024.0 * 1024.0) * 10.0).round() / 10.0
}

pub fn status() -> NativeStatus {
    let free = crate::sysutil::free_bytes(&paths::data_home());
    NativeStatus {
        rustc: version_of("rustc"),
        cargo: version_of("cargo"),
        // Archives are unpacked inside GERA, so no external tool is needed.
        unzip: true,
        disk_free_gb: gb(free),
        work_dir: work_dir().to_string_lossy().into_owned(),
        job: job_lock().clone(),
        builds: builds(),
    }
}

fn builds() -> Vec<NativeBuild> {
    let mut result = Vec::new();
    let Ok(entries) = fs::read_dir(paths::clients_dir()) else {
        return result;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(text) = fs::read_to_string(dir.join("native-build.json")) else {
            continue;
        };
        if let Ok(build) = serde_json::from_str::<NativeBuild>(&text) {
            if dir.join(crate::sysutil::exe("horizon-client")).is_file() {
                result.push(build);
            }
        }
    }
    result.sort_by(|a, b| b.built_at.cmp(&a.built_at));
    result
}

// ---------------------------------------------------------------------------
// Command helpers

fn parse_progress(line: &str) -> Option<(u32, u32)> {
    let text = line.trim();
    let open = text.find('[')?;
    let close = text[open..].find(']')? + open;
    let inside = text[open + 1..close].trim();
    let (_, rest) = inside.rsplit_once(char::is_whitespace)?;
    let (done, all) = rest.split_once('/')?;
    Some((done.trim().parse().ok()?, all.trim().parse().ok()?))
}

fn run_quiet(mut command: Command, what: &str) -> Result<String, String> {
    if cancelled() {
        return Err("Сборка отменена.".into());
    }
    crate::sysutil::quiet(&mut command);
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        let tail: Vec<&str> = text.lines().rev().take(25).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!("{what}: {}\n{}", output.status, tail.join("\n")));
    }
    Ok(text)
}

/// Runs a long command, streaming its output into the job log. Cancelling the
/// job kills the whole process group (cargo's rustc children too).
fn run_stream(mut command: Command, what: &str) -> Result<(), String> {
    if cancelled() {
        return Err("Сборка отменена.".into());
    }
    crate::sysutil::quiet(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // Its own process group, so cancelling reaches rustc as well as cargo.
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|e| format!("{what}: {e}"))?;
    *child_lock() = Some(child.id());
    let stderr = child.stderr.take().ok_or("Нет вывода процесса.")?;
    let reader = std::thread::spawn(move || {
        let mut tail: Vec<String> = Vec::new();
        let mut reader = BufReader::new(stderr);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            // Cargo's progress bar uses `\r` without a newline: read to `\n`
            // and split, so the log holds readable lines.
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&buffer).into_owned();
            for part in text.split(['\r', '\n']) {
                let trimmed = part.trim_end();
                if trimmed.trim().is_empty() {
                    continue;
                }
                if let Some((done, all)) = parse_progress(trimmed) {
                    set_compile(done, all);
                    continue;
                }
                if let Some(name) = trimmed.trim().strip_prefix("Compiling ") {
                    let name = name.split_whitespace().next().unwrap_or(name).to_string();
                    let mut job = job_lock();
                    job.detail = format!("cargo: {name}");
                }
                log(trimmed.to_string());
                tail.push(trimmed.to_string());
                if tail.len() > 30 {
                    tail.remove(0);
                }
            }
        }
        tail
    });
    let mut killed = false;
    let status = loop {
        if cancelled() && !killed {
            killed = true;
            kill_child();
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
            Err(error) => {
                *child_lock() = None;
                return Err(format!("{what}: {error}"));
            }
        }
    };
    *child_lock() = None;
    let tail = reader.join().unwrap_or_default();
    if cancelled() {
        return Err("Сборка отменена.".into());
    }
    if !status.success() {
        return Err(format!("{what}: {}\n{}", status, tail.join("\n")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pins and downloads

#[derive(Debug, Clone)]
struct Pins {
    horizon: String,
    mixins: String,
}

/// `native-1/<horizon commit>/<horizon-mixins commit>` from the registry.
fn parse_pins(api: &str) -> Option<Pins> {
    let rest = api.trim().strip_prefix("native-1/")?;
    let (horizon, mixins) = rest.split_once('/')?;
    let valid = |value: &str| value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit());
    if valid(horizon) && valid(mixins) {
        Some(Pins {
            horizon: horizon.to_string(),
            mixins: mixins.to_string(),
        })
    } else {
        None
    }
}

#[derive(Debug, Deserialize)]
struct NativeManifest {
    #[serde(default)]
    game: String,
    #[serde(default)]
    horizon: String,
    #[serde(default)]
    mixins: String,
    #[serde(default)]
    id: String,
}

fn download_mod(
    project: &ModProject,
    version: &ModVersion,
    verbose: bool,
) -> Result<PathBuf, String> {
    let file = cache_dir().join("hnmod").join(&version.file.name);
    let expected = version.file.sha256.to_ascii_lowercase();
    if file.is_file() && !expected.is_empty() {
        if net::sha256_file(&file).is_ok_and(|sum| sum == expected) {
            if verbose {
                log(format!("Мод уже скачан: {}", version.file.name));
            }
            return Ok(file);
        }
    }
    if verbose {
        set_step(2, "Загрузка мода", version.file.name.clone());
        log(format!("Скачивание {} ({})", version.file.name, project.name));
    }
    net::download(&version.file.url, &file)?;
    let actual = net::sha256_file(&file)?;
    if !expected.is_empty() && actual != expected {
        let _ = fs::remove_file(&file);
        return Err(format!(
            "SHA-256 мода не совпал. Ожидалось {expected}, получено {actual}. Сборка отменена."
        ));
    }
    Ok(file)
}

fn extract_mod(archive: &Path, id: &str, version: &str, verbose: bool) -> Result<PathBuf, String> {
    let _ = verbose;
    let target = mods_dir().join(format!("{id}-{version}"));
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    if let Err(error) = crate::sysutil::extract_zip(archive, &target) {
        let _ = fs::remove_dir_all(&target);
        return Err(format!("Не удалось распаковать .hnmod: {error}"));
    }
    if !target.join("horizon-native.json").is_file() {
        let _ = fs::remove_dir_all(&target);
        return Err("В .hnmod нет horizon-native.json.".into());
    }
    Ok(target)
}

fn source(horizon: &str) -> Result<PathBuf, String> {
    let dir = source_dir().join(horizon);
    if dir.join(".horizon-revision").is_file() && dir.join("Cargo.toml").is_file() {
        log("Исходники Horizon уже распакованы.");
        return Ok(dir);
    }
    set_step(3, "Исходники Horizon", format!("коммит {}", &horizon[..7]));
    let archive = cache_dir().join(format!("horizon-{horizon}.tar.gz"));
    if !archive.is_file() {
        log("Скачивание исходников Horizon с GitHub…");
        net::download(
            &format!("https://codeload.github.com/{HORIZON_REPO}/tar.gz/{horizon}"),
            &archive,
        )?;
    }
    let partial = source_dir().join(format!(".{horizon}.partial"));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    log("Распаковка исходников…");
    if let Err(error) = crate::sysutil::extract_tar_gz(&archive, &partial) {
        let _ = fs::remove_dir_all(&partial);
        let _ = fs::remove_file(&archive);
        return Err(format!("Не удалось распаковать исходники: {error}"));
    }
    let top = fs::read_dir(&partial)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .ok_or("Архив исходников пуст.")?;
    // The archive carries a few symlinks (CLAUDE.md and friends); horizon-mixins
    // refuses staged inputs with them, and the build never needs them.
    remove_links(&top);
    fs::write(top.join(".horizon-revision"), horizon).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&top, &dir).map_err(|e| format!("Не удалось разложить исходники: {e}"))?;
    let _ = fs::remove_dir_all(&partial);
    Ok(dir)
}

fn remove_links(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(&path);
        } else if meta.is_dir() {
            remove_links(&path);
        }
    }
}

fn mixins_tool(mixins: &str) -> Result<PathBuf, String> {
    let dir = tools_dir().join(&mixins[..12]);
    let binary = dir.join("bin").join(crate::sysutil::exe("horizon-mixins"));
    if binary.is_file() {
        log("horizon-mixins уже собран.");
        return Ok(binary);
    }
    set_step(4, "Сборка horizon-mixins", format!("коммит {}", &mixins[..7]));
    log("Сборка horizon-mixins (cargo install)…");
    let mut command = Command::new("cargo");
    command
        .args(["install", "--locked", "--git"])
        .arg(format!("https://github.com/{MIXINS_REPO}"))
        .args(["--rev", mixins, "--root"])
        .arg(&dir)
        .arg("horizon-mixins-cli")
        .env("CARGO_BUILD_JOBS", "1");
    run_stream(command, "Сборка horizon-mixins")?;
    if !binary.is_file() {
        return Err("cargo install не создал horizon-mixins.".into());
    }
    Ok(binary)
}

fn stage_mods(id: &str, extracted: &Path, key: &str) -> Result<PathBuf, String> {
    let bundle = extracted.join("mixins").join(id);
    if !bundle.join("horizon-mixin.toml").is_file() {
        return Err(format!("В .hnmod нет mixins/{id}/horizon-mixin.toml."));
    }
    let manifest = fs::read_to_string(bundle.join("horizon-mixin.toml"))
        .map_err(|e| format!("horizon-mixin.toml: {e}"))?;
    let trait_api = manifest
        .lines()
        .any(|line| line.trim().replace(' ', "") == "[api]");
    if trait_api {
        return Err(
            "Этот нативный мод использует trait-API Horizon ([api]). GERA пока собирает только \
             mixin-моды: API-часть требует генератора Horizon, которого в GERA нет."
                .into(),
        );
    }
    let target = mods_dir().join(key).join(id);
    let _ = fs::remove_dir_all(mods_dir().join(key));
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    copy_dir(&bundle, &target)?;
    Ok(mods_dir().join(key))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else if path.is_file() {
            fs::copy(&path, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn total_ram_bytes() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        0
    } else {
        status.ullTotalPhys
    }
}

#[cfg(not(windows))]
fn total_ram_bytes() -> u64 {
    let Ok(text) = fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    text.lines()
        .find(|line| line.starts_with("MemTotal:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

fn has_mods_feature(stage: &Path) -> bool {
    let Ok(text) = fs::read_to_string(stage.join("crates/horizon-client/Cargo.toml")) else {
        return false;
    };
    let mut in_features = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_features = trimmed == "[features]";
            continue;
        }
        if in_features && trimmed.starts_with("mods") && trimmed.contains('=') {
            return true;
        }
    }
    false
}

fn has_fast_profile(stage: &Path) -> bool {
    fs::read_to_string(stage.join("Cargo.toml"))
        .map(|text| text.lines().any(|line| line.trim() == "[profile.fast]"))
        .unwrap_or(false)
}

fn build_client(stage: &Path, profile: &str) -> Result<PathBuf, String> {
    set_step(6, "Компиляция клиента", "cargo build");
    let low_ram = total_ram_bytes() > 0 && total_ram_bytes() < 6 * 1024 * 1024 * 1024;
    // On a small machine the release optimisation of the game crate itself is
    // what runs out of memory at the end (the dependencies stay optimised).
    // A lighter opt-level and one codegen unit fit into 3.7 GB; if even that is
    // killed, the client is built unoptimised rather than not at all.
    let levels: Vec<Option<u32>> = if low_ram {
        vec![Some(1), Some(0)]
    } else {
        vec![None]
    };
    let binary = target_dir()
        .join(profile)
        .join(crate::sysutil::exe("horizon-client"));
    let mut last = String::new();
    for level in levels {
        log(match level {
            Some(level) => format!(
                "Компиляция horizon-client (сборка {profile}, opt-level={level}, 1 codegen-unit, 1 поток)…"
            ),
            None => format!("Компиляция horizon-client (сборка {profile}, 1 поток)…"),
        });
        let mut command = Command::new("cargo");
        command.arg("build").arg("--locked");
        command.args(["--profile", profile]);
        let config = stage.join(".cargo/config.toml");
        if config.is_file() {
            command.arg("--config").arg(&config);
        }
        command.arg("--manifest-path").arg(stage.join("Cargo.toml"));
        if has_mods_feature(stage) {
            command.args(["--features", "horizon-client/mods"]);
        }
        if let Some(level) = level {
            command.args([
                "--config",
                &format!("profile.fast.package.horizon-client.opt-level={level}"),
                "--config",
                "profile.fast.package.horizon-client.codegen-units=1",
            ]);
        }
        command.args(["-p", "horizon-client", "--bin", "horizon-client"]);
        command
            .env("CARGO_TARGET_DIR", target_dir())
            .env("CARGO_BUILD_JOBS", "1")
            .env("CARGO_TERM_PROGRESS_WHEN", "always")
            .env("CARGO_TERM_PROGRESS_WIDTH", "100")
            .env("CARGO_INCREMENTAL", "0")
            .current_dir(stage);
        match run_stream(command, "Компиляция клиента") {
            Ok(()) => {
                if !binary.is_file() {
                    return Err(format!("cargo не создал {}", binary.display()));
                }
                return Ok(binary);
            }
            Err(error) => {
                let killed = error.contains("signal: 9") || error.contains("SIGKILL");
                if killed && level == Some(1) && !cancelled() {
                    log("Сборку клиента убило из-за нехватки памяти — повтор без оптимизации.");
                    last = error;
                    continue;
                }
                return Err(error);
            }
        }
    }
    Err(last)
}

// ---------------------------------------------------------------------------
// The build itself

fn find_version(project: &ModProject, wanted: Option<&str>) -> Result<ModVersion, String> {
    match wanted {
        Some(version) => project
            .versions
            .iter()
            .find(|item| item.version == version)
            .cloned()
            .ok_or_else(|| format!("У мода {} нет версии {version}.", project.name)),
        None => project
            .versions
            .first()
            .cloned()
            .ok_or_else(|| format!("У мода {} нет версий.", project.name)),
    }
}

struct Base {
    dir: PathBuf,
    tag: String,
    minecraft: String,
    revision: Option<String>,
}

fn base_client(profile_id: &str) -> Result<(store::Profile, store::Settings, Base), String> {
    let data = paths::Paths::new();
    let store = store::load_or_init(&data).map_err(|e| format!("Не удалось прочитать настройки: {e}"))?;
    let profile = store
        .profile(profile_id)
        .cloned()
        .ok_or_else(|| "Сборка не найдена.".to_string())?;
    let settings = store.settings.clone();
    let discovery = crate::horizon::discover(settings.custom_horizon_dir.as_deref());
    let version = crate::horizon::resolve(&profile, &discovery).ok_or_else(|| {
        "Клиент Horizon не найден. Сначала установите и подготовьте сборку в настройках GERA."
            .to_string()
    })?;
    if !version.ready {
        return Err(format!(
            "У сборки {} нет готовых ресурсов. Подготовьте её в настройках GERA — иначе \
             собранному клиенту нечем запускать игру.",
            version.tag
        ));
    }
    let dir = PathBuf::from(&version.dir);
    if !dir.join(".local").is_dir() {
        return Err(format!(
            "В {} нет папки .local с ресурсами. Подготовьте сборку заново.",
            dir.display()
        ));
    }
    // A build GERA made records the exact commit it compiled.
    let revision = fs::read_to_string(dir.join("native-build.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<NativeBuild>(&text).ok())
        .map(|build| build.horizon)
        .or_else(|| version.revision.clone());
    Ok((
        profile,
        settings,
        Base {
            dir,
            tag: version.tag.clone(),
            minecraft: version.minecraft.clone(),
            revision,
        },
    ))
}

fn same_revision(pinned: &str, base: Option<&str>) -> bool {
    base.map(|value| !value.is_empty() && pinned.starts_with(value))
        .unwrap_or(false)
}

fn folder_for(horizon: &str, mixins: &str, game: &str, id: &str, version: &str, base: &Path) -> String {
    let fingerprint = format!("{horizon}|{mixins}|{game}|{id}|{version}|{}", base.display());
    let digest = net::sha256_text(&fingerprint);
    format!("native-{}-{}", &horizon[..7], &digest[..8])
}

fn run_build(
    profile_id: String,
    project_id: String,
    wanted: Option<String>,
    allow_mismatch: bool,
) -> Result<String, String> {
    set_step(1, "Проверка окружения", "rustc, cargo, место на диске");
    ensure_dirs()?;
    if version_of("rustc").is_none() || version_of("cargo").is_none() {
        return Err(
            "Для сборки нативных модов нужен Rust: rustc и cargo не найдены в PATH. \
             Установите rust (пакет rustup или rust) и повторите."
                .into(),
        );
    }
    let free = crate::sysutil::free_bytes(&work_dir());
    if free < MIN_FREE_BYTES {
        return Err(format!(
            "Мало места: свободно {} ГБ, для первой сборки нужно не меньше 12 ГБ.",
            gb(free)
        ));
    }
    log(format!(
        "Свободно {} ГБ; rustc и cargo на месте.",
        gb(free)
    ));

    let (_profile, _settings, base) = base_client(&profile_id)?;
    log(format!(
        "Базовая сборка: {} (Minecraft {})",
        base.tag, base.minecraft
    ));

    set_step(2, "Каталог модов", format!("{project_id}"));
    let catalog = mods::catalog(false)?;
    let project = catalog
        .into_iter()
        .find(|item| item.id == project_id)
        .ok_or_else(|| format!("Мод {project_id} не найден в реестре."))?;
    if project.kind != "native-mod" {
        return Err("Это не нативный мод.".into());
    }
    let version = find_version(&project, wanted.as_deref())?;
    let pins = parse_pins(&version.api).ok_or_else(|| {
        "В реестре у этого мода нет привязки native-1/<Horizon>/<mixins>.".to_string()
    })?;
    if !same_revision(&pins.horizon, base.revision.as_deref()) && !allow_mismatch {
        return Err(format!(
            "Мод собран для Horizon {}, а базовая сборка — {} ({}).\n\n\
             Сборка возможна, но мод с ней не проверяли: включите «Собрать всё равно», чтобы \
             попробовать. Собранный клиент будет на коммите мода.",
            &pins.horizon[..7],
            base.revision.as_deref().unwrap_or("неизвестный коммит"),
            base.tag
        ));
    }

    let archive = download_mod(&project, &version, true)?;
    set_step(2, "Распаковка мода", version.file.name.clone());
    let extracted = extract_mod(&archive, &project.id, &version.version, true)?;
    let manifest_text = fs::read_to_string(extracted.join("horizon-native.json"))
        .map_err(|e| format!("horizon-native.json: {e}"))?;
    let manifest: NativeManifest =
        serde_json::from_str(&manifest_text).map_err(|e| format!("horizon-native.json: {e}"))?;
    let id = if manifest.id.is_empty() {
        project.id.clone()
    } else {
        manifest.id.clone()
    };
    let horizon = if manifest.horizon.is_empty() {
        pins.horizon.clone()
    } else {
        manifest.horizon.clone()
    };
    let mixins = if manifest.mixins.is_empty() {
        pins.mixins.clone()
    } else {
        manifest.mixins.clone()
    };
    let game = if manifest.game.is_empty() {
        version.game.clone()
    } else {
        manifest.game.clone()
    };
    if !game.is_empty() && !base.minecraft.is_empty() && game != base.minecraft {
        log(format!(
            "Внимание: мод собран для Minecraft {game}, а ресурсы базовой сборки — {}. \
             Обычно это не мешает, но возможны расхождения.",
            base.minecraft
        ));
    }

    let folder = folder_for(&horizon, &mixins, &game, &id, &version.version, &base.dir);
    let key = folder.rsplit('-').next().unwrap_or("build").to_string();
    let clients = paths::clients_dir().join(&folder);
    if clients.join(crate::sysutil::exe("horizon-client")).is_file()
        && clients.join("native-build.json").is_file()
    {
        log(format!("Сборка уже есть: {folder}"));
        return Ok(folder);
    }

    let source = source(&horizon)?;
    let tool = mixins_tool(&mixins)?;

    set_step(5, "Применение mixins", format!("мод {id}"));
    let mods_root = stage_mods(&id, &extracted, &key)?;
    let stage = stage_dir().join(&key);
    let _ = fs::remove_dir_all(&stage);
    log("horizon-mixins apply…");
    let mut apply = Command::new(&tool);
    apply
        .arg("apply")
        .arg("--workspace")
        .arg(&source)
        .arg("--mods")
        .arg(&mods_root)
        .arg("--out")
        .arg(&stage);
    run_quiet(apply, "horizon-mixins apply")?;
    if !stage.join("Cargo.toml").is_file() {
        return Err("horizon-mixins не создал рабочую копию исходников.".into());
    }

    let profile_name = if has_fast_profile(&stage) { "fast" } else { "release" };
    let binary = build_client(&stage, profile_name)?;

    set_step(7, "Установка клиента", folder.clone());
    fs::create_dir_all(&clients).map_err(|e| e.to_string())?;
    let destination = clients.join(crate::sysutil::exe("horizon-client"));
    fs::copy(&binary, &destination).map_err(|e| format!("{}: {e}", destination.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    {
        let local = clients.join(".local");
        if !local.exists() {
            crate::sysutil::link_dir(&base.dir.join(".local"), &local)
                .map_err(|e| format!("Не удалось связать ресурсы базовой сборки: {e}"))?;
        }
    }
    let build = NativeBuild {
        folder: folder.clone(),
        project_id: project.id.clone(),
        name: project.name.clone(),
        version: version.version.clone(),
        horizon: horizon.clone(),
        mixins: mixins.clone(),
        base: base.tag.clone(),
        game: game.clone(),
        built_at: store::now_ms(),
    };
    fs::write(
        clients.join("native-build.json"),
        serde_json::to_string_pretty(&build).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    log(format!("Готово: {folder}"));
    Ok(folder)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCheck {
    pub base_tag: String,
    pub base_revision: Option<String>,
    pub horizon: String,
    pub mixins: String,
    pub match_commit: bool,
    pub game: String,
    pub built: Option<String>,
    pub trait_api: bool,
}

/// What a build would do, said before the player starts one.
#[tauri::command]
pub fn native_check(
    profile_id: String,
    project_id: String,
    version: Option<String>,
) -> Result<NativeCheck, String> {
    let (_, _, base) = base_client(&profile_id)?;
    let catalog = mods::catalog(false)?;
    let project = catalog
        .into_iter()
        .find(|item| item.id == project_id)
        .ok_or_else(|| format!("Мод {project_id} не найден в реестре."))?;
    if project.kind != "native-mod" {
        return Err("Это не нативный мод.".into());
    }
    let version = find_version(&project, version.as_deref())?;
    let pins = parse_pins(&version.api).ok_or_else(|| {
        "В реестре у этого мода нет привязки native-1/<Horizon>/<mixins>.".to_string()
    })?;
    let archive = download_mod(&project, &version, false)?;
    let extracted = extract_mod(&archive, &project.id, &version.version, false)?;
    let text = fs::read_to_string(extracted.join("horizon-native.json"))
        .map_err(|e| format!("horizon-native.json: {e}"))?;
    let manifest: NativeManifest =
        serde_json::from_str(&text).map_err(|e| format!("horizon-native.json: {e}"))?;
    let id = if manifest.id.is_empty() { project.id.clone() } else { manifest.id.clone() };
    let horizon = if manifest.horizon.is_empty() { pins.horizon.clone() } else { manifest.horizon.clone() };
    let mixins = if manifest.mixins.is_empty() { pins.mixins.clone() } else { manifest.mixins.clone() };
    let game = if manifest.game.is_empty() { version.game.clone() } else { manifest.game.clone() };
    let manifest_text = fs::read_to_string(extracted.join("mixins").join(&id).join("horizon-mixin.toml"))
        .unwrap_or_default();
    let trait_api = manifest_text
        .lines()
        .any(|line| line.trim().replace(' ', "") == "[api]");
    let folder = folder_for(&horizon, &mixins, &game, &id, &version.version, &base.dir);
    let built = paths::clients_dir()
        .join(&folder)
        .join(crate::sysutil::exe("horizon-client"))
        .is_file()
        .then_some(folder);
    Ok(NativeCheck {
        base_tag: base.tag.clone(),
        base_revision: base.revision.clone(),
        match_commit: same_revision(&horizon, base.revision.as_deref()),
        horizon,
        mixins,
        game,
        built,
        trait_api,
    })
}

/// The same build without the window, for testing and for a terminal:
/// `gera-launcher --native-build <profile> <project> [--version v] [--allow-mismatch]`.
pub fn build_blocking(
    profile: String,
    project: String,
    version: Option<String>,
    allow_mismatch: bool,
) -> Result<String, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(run_build(profile, project, version, allow_mismatch));
    });
    let mut printed = 0usize;
    let flush = |printed: &mut usize| {
        let job = job_lock();
        while *printed < job.log.len() {
            println!("{}", job.log[*printed]);
            *printed += 1;
        }
    };
    loop {
        flush(&mut printed);
        match receiver.try_recv() {
            Ok(result) => {
                flush(&mut printed);
                return result;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("Сборка прервана.".into())
            }
        }
    }
}

#[tauri::command]
pub fn native_status() -> NativeStatus {
    status()
}

#[tauri::command]
pub fn native_build(
    profile_id: String,
    project_id: String,
    version: Option<String>,
    allow_mismatch: bool,
) -> Result<(), String> {
    {
        let job = job_lock();
        if job.running {
            return Err("Сборка уже идёт.".into());
        }
    }
    CANCEL.store(false, Ordering::SeqCst);
    {
        let mut job = job_lock();
        *job = NativeJob {
            running: true,
            total: STEPS,
            stage: "Подготовка".into(),
            project: Some(project_id.clone()),
            started_at: store::now_ms(),
            ..NativeJob::default()
        };
    }
    std::thread::spawn(move || {
        let result = run_build(profile_id, project_id, version, allow_mismatch);
        let mut job = job_lock();
        job.running = false;
        job.finished_at = store::now_ms();
        match result {
            Ok(folder) => {
                job.step = STEPS;
                job.stage = "Готово".into();
                job.detail = folder.clone();
                job.folder = Some(folder);
                job.compile_done = None;
                job.compile_total = None;
            }
            Err(error) => {
                job.stage = "Ошибка".into();
                job.error = Some(error);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn native_cancel() -> NativeJob {
    {
        let job = job_lock();
        if !job.running {
            return job.clone();
        }
    }
    CANCEL.store(true, Ordering::SeqCst);
    log("Отмена сборки…");
    kill_child();
    job_lock().clone()
}

#[tauri::command]
pub fn native_log() -> NativeJob {
    job_lock().clone()
}

/// Folders of native builds GERA made, newest first.
#[tauri::command]
pub fn native_builds() -> Vec<NativeBuild> {
    builds()
}

/// Forget a native build (only GERA's own folder, never the game or a world).
#[tauri::command]
pub fn native_remove(folder: String) -> Result<Vec<NativeBuild>, String> {
    if !folder.starts_with("native-") || folder.contains('/') || folder.contains("..") {
        return Err("Это не папка нативной сборки GERA.".into());
    }
    let data = paths::Paths::new();
    if let Ok(store) = store::load_or_init(&data) {
        if let Some(profile) = store
            .profiles
            .iter()
            .find(|profile| profile.horizon_version.as_deref() == Some(folder.as_str()))
        {
            return Err(format!(
                "Сборка «{}» играет на этом клиенте. Выберите ей другую версию в настройках, затем удаляйте.",
                profile.name
            ));
        }
    }
    let dir = paths::clients_dir().join(&folder);
    if !dir.join("native-build.json").is_file() {
        return Err("Сборка не найдена.".into());
    }
    // The symlink points into the base client: remove it, not what it points to.
    let local = dir.join(".local");
    if local.is_symlink() {
        let _ = fs::remove_file(&local);
    }
    fs::remove_dir_all(&dir).map_err(|e| format!("Не удалось удалить сборку: {e}"))?;
    Ok(builds())
}
