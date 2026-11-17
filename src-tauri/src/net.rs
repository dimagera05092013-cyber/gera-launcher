use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};

/// Small HTTPS helper. Uses the system curl so GERA does not pull a full HTTP
/// stack (and its dependency tree) into the launcher.
pub fn fetch_text(url: &str, timeout_secs: u32) -> Result<String, String> {
    let mut command = Command::new("curl");
    command.args([
        "-fsSL",
        "--proto",
        "=https",
        "--max-time",
        &timeout_secs.to_string(),
        url,
    ]);
    let output = crate::sysutil::quiet(&mut command)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("curl недоступен: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Не удалось получить {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "Ответ не является текстом UTF-8.".into())
}

pub fn download(url: &str, destination: &Path) -> Result<u64, String> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
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
            "-o",
        ])
        .arg(destination)
        .arg(url);
    let output = crate::sysutil::quiet(&mut command)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("curl недоступен: {e}"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(destination);
        return Err(format!(
            "Загрузка не удалась: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    std::fs::metadata(destination)
        .map(|m| m.len())
        .map_err(|e| e.to_string())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
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
