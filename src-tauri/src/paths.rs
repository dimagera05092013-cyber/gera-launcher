use std::env;
use std::path::PathBuf;

/// Where GERA Launcher keeps its own data. Never inside a Minecraft world or
/// Horizon's game folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub data: PathBuf,
    pub logs: PathBuf,
    pub profiles: PathBuf,
    pub cache: PathBuf,
}

pub fn home_dir() -> PathBuf {
    if let Some(home) = env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home);
    }
    if let Some(profile) = env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
        return PathBuf::from(profile);
    }
    #[cfg(windows)]
    if let (Some(drive), Some(path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
        let mut full = drive;
        full.push(path);
        return PathBuf::from(full);
    }
    PathBuf::from("/")
}

pub fn data_home() -> PathBuf {
    if let Some(data) = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return data;
    }
    // On Windows the equivalent of ~/.local/share is %LOCALAPPDATA%.
    #[cfg(windows)]
    if let Some(local) = env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        return PathBuf::from(local);
    }
    home_dir().join(".local/share")
}

/// GERA's own installed Horizon clients.
pub fn clients_dir() -> PathBuf {
    data_home().join("gera-launcher/clients")
}

/// Read-only legacy location used by the official GUI launcher on this machine.
/// GERA may import what it finds here, but never writes to it and never
/// requires it to exist.
pub fn legacy_horizon_dir() -> PathBuf {
    data_home().join("horizon-launcher/versions")
}

pub fn horizon_default_game_dir() -> PathBuf {
    data_home().join("horizon")
}

impl Paths {
    pub fn new() -> Self {
        let data = data_home().join("gera-launcher");
        Self {
            logs: data.join("logs"),
            profiles: data.join("profiles"),
            cache: data.join("cache"),
            data,
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.data, &self.logs, &self.profiles, &self.cache] {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::create_dir_all(clients_dir())?;
        Ok(())
    }

    pub fn store_file(&self) -> PathBuf {
        self.data.join("state.json")
    }
}
