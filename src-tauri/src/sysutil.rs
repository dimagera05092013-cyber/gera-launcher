//! Small platform helpers: archive unpacking, free space, opening files and
//! process control. GERA used to shell out to `tar`, `unzip`, `df` and `kill`,
//! which do not exist on Windows; those jobs are done in Rust here.

use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};

/// Keep helper programs (PowerShell, cargo, the game itself) from flashing a
/// console window on Windows. A no-op on other systems.
pub fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Drop the `\\?\` prefix that Windows adds to canonical paths: `cmd`, `mklink`
/// and Explorer do not understand it.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn plain_path(path: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return std::path::PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return std::path::PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

/// Name of a bundled binary on this platform.
pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Unpack a `.tar.gz` archive, skipping links (Horizon sources carry a few
/// documentation symlinks that Windows cannot create and the build never uses).
pub fn extract_tar_gz(archive: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut tar = tar::Archive::new(decoder);
    #[cfg(unix)]
    tar.set_preserve_permissions(true);
    let entries = tar
        .entries()
        .map_err(|e| format!("Не удалось прочитать архив: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("Не удалось прочитать архив: {e}"))?;
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            continue;
        }
        entry
            .unpack_in(destination)
            .map_err(|e| format!("Не удалось распаковать архив: {e}"))?;
    }
    Ok(())
}

/// Unpack a zip archive. Used for `.hnmod` files and Windows client builds.
pub fn extract_zip(archive: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    let file = fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut zip =
        zip::ZipArchive::new(BufReader::new(file)).map_err(|e| format!("Не удалось прочитать zip: {e}"))?;
    zip.extract(destination)
        .map_err(|e| format!("Не удалось распаковать {}: {e}", archive.display()))
}

/// Free space in bytes on the volume that holds `path`.
pub fn free_bytes(path: &Path) -> u64 {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut())
        };
        return if ok != 0 { free } else { 0 };
    }
    #[cfg(not(windows))]
    {
        let Ok(output) = Command::new("df").args(["-Pk"]).arg(path).output() else {
            return 0;
        };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .nth(1)
            .and_then(|line| line.split_whitespace().nth(3))
            .and_then(|value| value.parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .unwrap_or(0)
    }
}

/// Open a file, folder or https link in the user's default application.
pub fn open_target(target: &str) -> Result<(), String> {
    let mut command;
    #[cfg(windows)]
    {
        command = Command::new("explorer");
        command.arg(target);
    }
    #[cfg(not(windows))]
    {
        command = Command::new("xdg-open");
        command.arg(target);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось открыть: {e}"))
}

/// Kill a process and everything it started (cargo's rustc children too).
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        let _ = quiet(&mut command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("-{pid}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Whether a process with this id is still running.
pub fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
        unsafe { CloseHandle(handle) };
        return ok != 0 && code == STILL_ACTIVE as u32;
    }
    #[cfg(not(windows))]
    {
        Path::new(&format!("/proc/{pid}")).is_dir()
    }
}

/// Make `link` point at `target` without copying. Falls back to a junction on
/// Windows, which works without developer mode.
pub fn link_dir(target: &Path, link: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", "mklink", "/J"]);
        command.arg(plain_path(link));
        command.arg(plain_path(target));
        let status = quiet(&mut command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("Не удалось связать папку ресурсов.".into())
        }
    }
}

/// Recreate a link. Windows falls back to a junction for folders and to a copy
/// for files, because creating symlinks there needs extra privileges.
pub fn copy_link(path: &Path, target: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let link = fs::read_link(path).map_err(|e| e.to_string())?;
        std::os::unix::fs::symlink(&link, target).map_err(|e| e.to_string())
    }
    #[cfg(windows)]
    {
        let real = fs::canonicalize(path)
            .map(|p| plain_path(&p))
            .unwrap_or_else(|_| path.to_path_buf());
        if real.is_dir() {
            link_dir(&real, target)
        } else {
            fs::copy(&real, target)
                .map(|_| ())
                .map_err(|e| format!("{}: {e}", target.display()))
        }
    }
}

pub fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            copy_link(&path, &target)?;
        } else if meta.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            fs::copy(&path, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gera-sysutil-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unpacks_tar_gz() {
        let dir = temp_dir("tar");
        let archive = dir.join("a.tar.gz");
        {
            let file = fs::File::create(&archive).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
            let mut builder = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(5);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, "bin/tool", &b"hello"[..]).unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        extract_tar_gz(&archive, &out).unwrap();
        assert_eq!(fs::read_to_string(out.join("bin/tool")).unwrap(), "hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpacks_zip() {
        let dir = temp_dir("zip");
        let archive = dir.join("a.zip");
        {
            let file = fs::File::create(&archive).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("mixins/demo/horizon-mixin.toml", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"[mod]").unwrap();
            writer.finish().unwrap();
        }
        let out = dir.join("out");
        extract_zip(&archive, &out).unwrap();
        assert_eq!(
            fs::read_to_string(out.join("mixins/demo/horizon-mixin.toml")).unwrap(),
            "[mod]"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    mod windows_tests {
        use super::*;

        #[test]
        fn strips_verbatim_prefix() {
            assert_eq!(
                plain_path(Path::new(r"\\?\C:\Users\Gera\.local\share")),
                std::path::PathBuf::from(r"C:\Users\Gera\.local\share")
            );
            assert_eq!(
                plain_path(Path::new(r"\\?\UNC\server\share")),
                std::path::PathBuf::from(r"\\server\share")
            );
            assert_eq!(
                plain_path(Path::new(r"C:\plain")),
                std::path::PathBuf::from(r"C:\plain")
            );
        }

        #[test]
        fn reports_free_space_of_a_volume() {
            let free = free_bytes(Path::new(r"C:\"));
            assert!(free > 0, "free space on C: should be known");
        }

        #[test]
        fn tells_running_and_finished_processes_apart() {
            assert!(process_alive(std::process::id()));
            // No process can have this id, so opening it must fail.
            assert!(!process_alive(0xFFFF_FFF0));
        }
    }
}
