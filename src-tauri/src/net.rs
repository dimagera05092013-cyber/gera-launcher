//! Small HTTPS helper built on `ureq` (rustls). GERA used to shell out to
//! `curl`, which is missing on some Windows 10 builds and mangles non-ASCII
//! output paths under the system ANSI code page. Doing HTTP in Rust fixes both,
//! and gives us real status codes, timeouts, retries and resume support.

use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const ATTEMPTS: u32 = 4;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const BODY_TIMEOUT: Duration = Duration::from_secs(180);
const BUFFER: usize = 64 * 1024;

fn agent(timeout: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_body(Some(timeout))
        .http_status_as_error(false)
        .user_agent(concat!("GERA-Launcher/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

fn retryable(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
}

/// Seconds the server asked us to wait, capped so a broken mirror cannot stall
/// the launcher for minutes.
fn retry_after(response: &ureq::http::Response<ureq::Body>) -> Option<Duration> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|secs| Duration::from_secs(secs.min(8)))
}

fn status_hint(status: u16) -> &'static str {
    match status {
        401 => "сервер требует вход (401)",
        403 => "сервер запретил доступ (403)",
        404 => "файл не найден на сервере (404)",
        429 => "слишком много запросов (429)",
        500..=599 => "ошибка на стороне сервера (5xx)",
        _ => "неожиданный ответ сервера",
    }
}

fn snippet(response: &mut ureq::http::Response<ureq::Body>) -> String {
    let mut text = String::new();
    if let Ok(bytes) = response.body_mut().with_config().limit(400).read_to_vec() {
        text = String::from_utf8_lossy(&bytes).trim().to_string();
    }
    if text.is_empty() {
        String::new()
    } else {
        format!(" Ответ: {text}")
    }
}

/// GET a small text document (JSON metadata, checksum lists).
pub fn fetch_text(url: &str, timeout_secs: u32) -> Result<String, String> {
    let timeout = Duration::from_secs(timeout_secs.clamp(5, 600) as u64);
    let agent = agent(timeout);
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(400 * (1 << (attempt - 1))));
        }
        let response = match agent.get(url).call() {
            Ok(response) => response,
            Err(error) => {
                last = format!("сетевая ошибка: {error}");
                continue;
            }
        };
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            let mut response = response;
            let bytes = response
                .body_mut()
                .with_config()
                .limit(32 * 1024 * 1024)
                .read_to_vec()
                .map_err(|e| format!("Не удалось прочитать ответ {url}: {e}"))?;
            return String::from_utf8(bytes)
                .map_err(|_| "Ответ сервера не является текстом UTF-8.".to_string());
        }
        if retryable(status) {
            last = format!("{}: HTTP {status}", status_hint(status));
            continue;
        }
        let mut response = response;
        return Err(format!(
            "Не удалось получить {url}: HTTP {status} — {}.{}",
            status_hint(status),
            snippet(&mut response)
        ));
    }
    Err(format!("Не удалось получить {url}: {last}"))
}

fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

fn size_of(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Download a file into `destination` atomically: bytes go to `<file>.part`,
/// which is renamed only after the whole body arrived. A leftover `.part` is
/// resumed with an HTTP range request, so a restarted launcher continues where
/// it stopped instead of starting over.
pub fn download(url: &str, destination: &Path) -> Result<u64, String> {
    download_to(url, destination, 0, None, &mut |_, _| {})
}

/// Same as [`download`], with expected size and SHA-1 verification.
pub fn download_checked(
    url: &str,
    destination: &Path,
    size: u64,
    sha1: Option<&str>,
) -> Result<u64, String> {
    download_to(url, destination, size, sha1, &mut |_, _| {})
}

/// `progress(done, total)` is called while the body is written. `total` is 0
/// when the server did not report a length.
pub fn download_to(
    url: &str,
    destination: &Path,
    expected_size: u64,
    expected_sha1: Option<&str>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<u64, String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Не удалось создать {}: {e}", parent.display()))?;
    }
    let part = part_path(destination);

    // A finished file with the right size and hash is never downloaded again.
    if expected_size > 0 && size_of(destination) == expected_size {
        if matches_sha1(destination, expected_sha1)? {
            return Ok(expected_size);
        }
        let _ = fs::remove_file(destination);
    }

    let agent = agent(BODY_TIMEOUT);
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(600 * (1 << (attempt - 1))));
        }
        let resumed = size_of(&part);
        let mut request = agent.get(url);
        if resumed > 0 {
            request = request.header("Range", format!("bytes={resumed}-"));
        }
        let response = match request.call() {
            Ok(response) => response,
            Err(error) => {
                last = format!("сетевая ошибка: {error}");
                continue;
            }
        };
        let status = response.status().as_u16();
        let mut response = response;
        if status == 416 {
            // The part file already covers the whole resource.
            let len = size_of(&part);
            if expected_size == 0 || len == expected_size {
                finish(&part, destination)?;
                if !matches_sha1(destination, expected_sha1)? {
                    let _ = fs::remove_file(destination);
                    last = "файл скачался повреждённым (SHA-1 не совпал)".into();
                    continue;
                }
                return Ok(len);
            }
            let _ = fs::remove_file(&part);
            last = "сервер отверг продолжение загрузки".into();
            continue;
        }
        if status != 200 && status != 206 {
            if retryable(status) {
                let wait = retry_after(&response);
                last = format!("{}: HTTP {status}", status_hint(status));
                if let Some(wait) = wait {
                    std::thread::sleep(wait);
                }
                continue;
            }
            return Err(format!(
                "Загрузка не удалась: HTTP {status} — {}.{}{}",
                status_hint(status),
                snippet(&mut response),
                if status == 403 || status == 404 {
                    "\nПроверьте подключение к интернету и повторите позже."
                } else {
                    ""
                }
            ));
        }

        let (start, total) = if status == 206 && resumed > 0 {
            (resumed, resumed + content_length(&response))
        } else {
            (0, content_length(&response))
        };
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(start > 0)
            .truncate(start == 0)
            .open(&part)
            .map_err(|e| format!("Не удалось записать {}: {e}", part.display()))?;
        if start > 0 {
            use std::io::Seek;
            file.seek(std::io::SeekFrom::Start(start))
                .map_err(|e| format!("Не удалось продолжить {}: {e}", part.display()))?;
        }

        let mut reader = response.body_mut().as_reader();
        let mut buffer = vec![0u8; BUFFER];
        let mut written = start;
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) => {
                    last = format!("обрыв загрузки: {error}");
                    break;
                }
            };
            if let Err(error) = file.write_all(&buffer[..read]) {
                return Err(format!("Не удалось записать {}: {error}", part.display()));
            }
            written += read as u64;
            progress(written, total);
        }
        if let Err(error) = file.flush() {
            return Err(format!("Не удалось записать {}: {error}", part.display()));
        }
        drop(file);

        let complete = if expected_size > 0 {
            written == expected_size
        } else if total > 0 {
            written == total
        } else {
            written > 0
        };
        if !complete {
            let reason = if last.is_empty() {
                String::new()
            } else {
                format!(", {last}")
            };
            last = format!(
                "загрузка прервана: {written} из {}{reason}",
                expected_size.max(total)
            );
            continue;
        }
        finish(&part, destination)?;
        if !matches_sha1(destination, expected_sha1)? {
            let _ = fs::remove_file(destination);
            last = "файл скачался повреждённым (SHA-1 не совпал)".into();
            continue;
        }
        return Ok(written);
    }
    Err(format!(
        "Загрузка не удалась после {ATTEMPTS} попыток: {last}"
    ))
}

fn content_length(response: &ureq::http::Response<ureq::Body>) -> u64 {
    response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn finish(part: &Path, destination: &Path) -> Result<(), String> {
    // Windows refuses to rename over an existing file.
    if destination.exists() {
        let _ = fs::remove_file(destination);
    }
    fs::rename(part, destination).map_err(|e| {
        format!(
            "Не удалось завершить запись {}: {e}",
            destination.display()
        )
    })
}

fn matches_sha1(path: &Path, expected: Option<&str>) -> Result<bool, String> {
    let Some(expected) = expected else {
        return Ok(true);
    };
    if expected.is_empty() {
        return Ok(true);
    }
    let actual = sha1_file(path)?;
    Ok(actual.eq_ignore_ascii_case(expected))
}

pub fn sha1_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; BUFFER];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Parse a `sha256sum`-style file and return the digest for a file name.
pub fn checksum_for(sums: &str, file_name: &str) -> Option<String> {
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let name = parts.next().unwrap_or("");
        let name = name.trim_start_matches('*').trim_start_matches("./");
        if name == file_name && digest.len() == 64 {
            return Some(digest.to_ascii_lowercase());
        }
    }
    None
}

/// SHA-256 of a short text, for stable folder names.
pub fn sha256_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_download_is_resumed() {
        let dir = std::env::temp_dir().join(format!("gera-net-resume-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("файл с пробелом.bin");
        let part = part_path(&target);
        fs::write(&part, b"abc").unwrap();
        assert_eq!(size_of(&part), 3);
        assert_eq!(part.extension().unwrap(), "part");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_ascii_paths_are_kept() {
        let dir = std::env::temp_dir().join("gera-net-кириллица");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("объект.json");
        fs::write(&file, b"{}").unwrap();
        assert!(file.is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn checksum_file_is_parsed() {
        let sums = "abc123  gera-launcher-linux\n";
        assert_eq!(checksum_for(sums, "gera-launcher-linux"), None);
        let digest = "a".repeat(64);
        let sums = format!("{digest}  ./gera-launcher.exe\n");
        assert_eq!(
            checksum_for(&sums, "gera-launcher.exe"),
            Some(digest.clone())
        );
    }
}
