//! GERA's own logs. One file per stage, all in `~/.local/share/gera-launcher/logs`
//! (`%LOCALAPPDATA%\gera-launcher\logs` on Windows):
//!
//! - `launcher.log` — start-up, versions, update checks;
//! - `assets-download.log` — Mojang metadata, client, libraries and resources;
//! - `import.log` — resources brought from another computer;
//! - `prepare.log` — `horizon-prepare` runs with their exit code and output;
//! - `game-launch.log` — the exact command line and how the game ended.
//!
//! Every line is `UTC time | LEVEL | stage | message`. Nothing secret is written:
//! there are no tokens or account data in this launcher.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths;

const ROTATE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Launcher,
    Assets,
    Import,
    Prepare,
    Launch,
}

impl Kind {
    pub fn file(self) -> &'static str {
        match self {
            Kind::Launcher => "launcher.log",
            Kind::Assets => "assets-download.log",
            Kind::Import => "import.log",
            Kind::Prepare => "prepare.log",
            Kind::Launch => "game-launch.log",
        }
    }
}

pub fn logs_dir() -> PathBuf {
    paths::Paths::new().logs
}

pub fn ensure_dir() -> Result<PathBuf, String> {
    let dir = logs_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
    Ok(dir)
}

/// `2026-11-18 08:00:00` in UTC. Local time is not used on purpose: it must not
/// depend on the system time zone or on Windows regional settings.
pub fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// Howard Hinnant's days-to-civil algorithm, valid for any positive day count.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A log file that is opened once and appended to. If the file cannot be
/// created, writing is silently skipped: a full disk must not break the game.
pub struct Log {
    path: PathBuf,
    file: Option<fs::File>,
}

impl Log {
    pub fn open(kind: Kind) -> Log {
        let path = logs_dir().join(kind.file());
        let file = ensure_dir().ok().and_then(|_| {
            rotate(&path);
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()
        });
        Log { path, file }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&mut self, level: &str, stage: &str, message: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let line = format!("{} | {level:<5} | {stage} | {}\n", timestamp(), one_line(message));
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }

    pub fn info(&mut self, stage: &str, message: &str) {
        self.write("INFO", stage, message);
    }

    pub fn warn(&mut self, stage: &str, message: &str) {
        self.write("WARN", stage, message);
    }

    pub fn error(&mut self, stage: &str, message: &str) {
        self.write("ERROR", stage, message);
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ⏎ ")
}

fn rotate(path: &Path) {
    let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if size < ROTATE_BYTES {
        return;
    }
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".1");
    let backup = PathBuf::from(backup);
    let _ = fs::remove_file(&backup);
    let _ = fs::rename(path, &backup);
}

/// Newest log file, for the "open last log" button.
pub fn last_log() -> Option<PathBuf> {
    let mut files: Vec<(SystemTime, PathBuf)> = fs::read_dir(logs_dir())
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    files.sort_by(|left, right| right.0.cmp(&left.0));
    files.into_iter().map(|(_, path)| path).next()
}

/// Short human-readable list of the log files that exist.
pub fn summary() -> String {
    let dir = logs_dir();
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_is_ordered_text() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), 19);
        assert!(stamp.starts_with("20"), "{stamp}");
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], " ");
    }

    #[test]
    fn civil_dates_are_correct() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_704), (2026, 9, 8));
    }

    #[test]
    fn newlines_do_not_split_a_record() {
        let text = one_line("первая\nвторая");
        assert!(!text.contains('\n'));
        assert!(text.contains('⏎'));
    }

    #[test]
    fn unwritable_log_does_not_panic() {
        let log = Log {
            path: PathBuf::from("Z:/нет/такой/папки/launcher.log"),
            file: None,
        };
        let mut log = log;
        log.error("test", "ничего не произошло");
    }
}
