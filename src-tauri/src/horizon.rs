use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::paths;
use crate::store::Profile;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HorizonVersion {
    pub tag: String,
    pub dir: String,
    pub client: String,
    pub resources: String,
    pub ready: bool,
    pub source: String,
    pub revision: Option<String>,
    pub minecraft: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningGame {
    pub pid: u32,
    pub game_dir: String,
    pub player: Option<String>,
    pub client: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchApiInfo {
    pub launch_api: String,
    pub horizon: String,
    pub minecraft: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub versions: Vec<HorizonVersion>,
}

/// Discover clients installed by GERA, plus read-only imports of versions that
/// already exist on disk. The official GUI launcher is never required.
pub fn discover(custom_dir: Option<&str>) -> Discovery {
    let mut versions = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut tags: HashSet<String> = HashSet::new();

    for (root, source) in [
        (paths::clients_dir(), "gera"),
        (paths::legacy_horizon_dir(), "legacy"),
    ] {
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let Some(version) = inspect(&dir, source) else {
                    continue;
                };
                if source == "legacy" && tags.contains(&version.tag) {
                    continue;
                }
                tags.insert(version.tag.clone());
                seen.insert(canonical(&dir));
                versions.push(version);
            }
        }
    }

    if let Some(custom) = custom_dir.map(str::trim).filter(|s| !s.is_empty()) {
        let dir = PathBuf::from(custom);
        if dir.is_dir() && !seen.contains(&canonical(&dir)) {
            if let Some(mut version) = inspect(&dir, "custom") {
                version.tag = format!("{} (своя)", version.tag);
                versions.push(version);
            }
        }
    }

    versions.sort_by(|a, b| b.tag.cmp(&a.tag));
    Discovery { versions }
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn inspect(dir: &Path, source: &str) -> Option<HorizonVersion> {
    let client = dir.join(crate::sysutil::exe("horizon-client"));
    if !client.is_file() {
        return None;
    }
    let tag = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Horizon".into());
    let (resources, minecraft, ready) = prepared_resources(dir);
    Some(HorizonVersion {
        source: source.into(),
        revision: revision_hint(&tag),
        tag,
        dir: dir.to_string_lossy().into_owned(),
        client: client.to_string_lossy().into_owned(),
        resources: resources.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        ready,
        minecraft,
    })
}

/// Resources live in <version>/.local/horizon-client/<minecraft>/. Horizon
/// itself refuses to start without `horizon-resources.json`, so both manifests
/// are required before GERA calls a folder ready.
pub fn prepared_resources(version_dir: &Path) -> (Option<PathBuf>, String, bool) {
    let root = version_dir.join(".local/horizon-client");
    let mut fallback: Option<(PathBuf, String, bool)> = None;
    let Ok(entries) = fs::read_dir(&root) else {
        return (None, "26.3".into(), false);
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "26.3".into());
        let ready = dir.join("asset-index.json").is_file()
            && dir.join("horizon-resources.json").is_file();
        if ready {
            return (Some(dir), name, true);
        }
        fallback.get_or_insert((dir, name, false));
    }
    match fallback {
        Some((dir, name, ready)) => (Some(dir), name, ready),
        None => (None, "26.3".into(), false),
    }
}

fn revision_hint(tag: &str) -> Option<String> {
    let tail = tag.rsplit('-').next()?;
    (tail.len() >= 7 && tail.chars().all(|c| c.is_ascii_hexdigit())).then(|| tail.to_string())
}

pub fn resolve<'a>(profile: &Profile, discovery: &'a Discovery) -> Option<&'a HorizonVersion> {
    if let Some(tag) = &profile.horizon_version {
        if let Some(found) = discovery.versions.iter().find(|v| &v.tag == tag) {
            return Some(found);
        }
    }
    discovery
        .versions
        .iter()
        .find(|v| v.ready)
        .or_else(|| discovery.versions.first())
}

pub fn effective_game_dir(paths: &paths::Paths, profile: &Profile) -> PathBuf {
    let dir = PathBuf::from(&profile.game_dir);
    if dir.is_absolute() {
        dir
    } else {
        paths.data.join(dir)
    }
}

pub fn process_alive(pid: u32) -> bool {
    crate::sysutil::process_alive(pid)
}

/// Horizon processes started by GERA or by another launcher/script.
#[cfg(windows)]
pub fn running_games() -> Vec<RunningGame> {
    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-CimInstance Win32_Process -Filter \"Name='horizon-client.exe'\" | Select-Object ProcessId,CommandLine | ConvertTo-Json -Compress",
    ]);
    let output = crate::sysutil::quiet(&mut command)
        .stdin(Stdio::null())
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim().trim_start_matches('\u{feff}');
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let items = match value {
        serde_json::Value::Array(items) => items,
        other => vec![other],
    };
    let mut games = Vec::new();
    for item in items {
        let Some(pid) = item.get("ProcessId").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(line) = item.get("CommandLine").and_then(|v| v.as_str()) else {
            continue;
        };
        let args = split_windows_args(line);
        let mut game_dir = None;
        let mut player = None;
        let mut iter = args.iter().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--game-dir" => game_dir = iter.next().cloned(),
                "--name" => player = iter.next().cloned(),
                _ => {}
            }
        }
        games.push(RunningGame {
            pid: pid as u32,
            game_dir: game_dir.unwrap_or_default(),
            player,
            client: args.first().cloned().unwrap_or_default(),
        });
    }
    games
}

/// Split a Windows command line the way the C runtime does: quotes group an
/// argument and are not part of it.
#[cfg(windows)]
fn split_windows_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            ' ' | '\t' if !quoted => {
                if started || !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                current.push(ch);
                started = true;
            }
        }
    }
    if started || !current.is_empty() {
        args.push(current);
    }
    args
}

/// Horizon processes started by GERA or by another launcher/script.
#[cfg(not(windows))]
pub fn running_games() -> Vec<RunningGame> {
    let mut games = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return games;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(raw) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if raw.is_empty() {
            continue;
        }
        let args: Vec<String> = raw
            .split(|b| *b == 0)
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .filter(|s| !s.is_empty())
            .collect();
        let Some(client) = args.first() else {
            continue;
        };
        let is_client = Path::new(client)
            .file_name()
            .map(|n| n == crate::sysutil::exe("horizon-client").as_str())
            .unwrap_or(false);
        if !is_client {
            continue;
        }
        let mut game_dir = None;
        let mut player = None;
        let mut iter = args.iter().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--game-dir" => game_dir = iter.next().cloned(),
                "--name" => player = iter.next().cloned(),
                _ => {}
            }
        }
        games.push(RunningGame {
            pid,
            game_dir: game_dir.unwrap_or_default(),
            player,
            client: client.clone(),
        });
    }
    games
}

/// Reads `horizon-client --launch-api` with a hard timeout. This starts no game.
pub fn launch_api(client: &Path) -> Option<LaunchApiInfo> {
    let mut probe = Command::new(client);
    probe.arg("--launch-api");
    crate::sysutil::quiet(&mut probe);
    let output = run_with_timeout(&mut probe, Duration::from_secs(4))?;
    let value: serde_json::Value = serde_json::from_slice(&output).ok()?;
    Some(LaunchApiInfo {
        launch_api: value.get("launch_api")?.as_str()?.to_string(),
        horizon: value
            .get("horizon")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        minecraft: value
            .get("minecraft")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        options: value
            .get("options")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn run_with_timeout(command: &mut Command, timeout: Duration) -> Option<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().ok().flatten() {
            if !status.success() {
                let _ = child.kill();
            }
            return rx.recv_timeout(Duration::from_secs(1)).ok();
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Last lines of the game's own log (horizon writes logs/latest.log into --home).
pub fn tail_log(game_dir: &Path, lines: usize) -> Option<String> {
    let path = game_dir.join("logs/latest.log");
    let text = fs::read_to_string(&path).ok()?;
    let mut collected: Vec<&str> = text.lines().rev().take(lines).collect();
    collected.reverse();
    Some(collected.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_version(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gera-horizon-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn half_prepared_folder_is_not_ready() {
        let dir = temp_version("half");
        let resources = dir.join(".local/horizon-client/26.3");
        fs::create_dir_all(&resources).unwrap();
        fs::write(resources.join("asset-index.json"), b"{\"objects\":{}}").unwrap();
        let (_path, _minecraft, ready) = prepared_resources(&dir);
        assert!(!ready, "asset-index.json alone must not count as ready");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fully_prepared_folder_is_ready() {
        let dir = temp_version("full");
        let resources = dir.join(".local/horizon-client/26.3");
        fs::create_dir_all(&resources).unwrap();
        fs::write(resources.join("asset-index.json"), b"{\"objects\":{}}").unwrap();
        fs::write(
            resources.join("horizon-resources.json"),
            b"{\"minecraft\":\"26.3\"}",
        )
        .unwrap();
        let (path, minecraft, ready) = prepared_resources(&dir);
        assert!(ready);
        assert_eq!(minecraft, "26.3");
        assert_eq!(path.unwrap(), resources);
        let _ = fs::remove_dir_all(&dir);
    }
}
