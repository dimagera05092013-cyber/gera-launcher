use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths::{self, Paths};

pub const SCHEMA: u32 = 1;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// UI performance profile: "light" or "heavy". Interface only.
    pub ui_mode: String,
    /// "system", "light" or "dark".
    pub theme: String,
    /// Seed colour as #rrggbb.
    pub accent: String,
    /// Decorative gradients in the hero, tiles and brand marks.
    #[serde(default)]
    pub gradients: bool,
    /// Extra Horizon version folder supplied by the player.
    #[serde(default)]
    pub custom_horizon_dir: Option<String>,
    /// Official Minecraft Java game folder, used only by horizon-prepare.
    #[serde(default)]
    pub minecraft_dir: Option<String>,
    /// Path to the Minecraft server jar the player supplies for preparation.
    #[serde(default)]
    pub server_jar: Option<String>,
    /// HTTPS address of the GERA update manifest. Empty until a release channel exists.
    #[serde(default)]
    pub update_manifest: String,
    /// Check registry mods for updates when the Mods page opens (Stage 4).
    #[serde(default = "default_true")]
    pub check_mod_updates: bool,
    /// Close the launcher once the game window has had time to appear. The
    /// game keeps running; the launch log stays next to the game folder.
    #[serde(default)]
    pub close_on_launch: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // The player's laptop has 3.7 GiB RAM: light is the safe default.
            ui_mode: "light".into(),
            theme: "system".into(),
            accent: "#6750A4".into(),
            gradients: false,
            custom_horizon_dir: None,
            minecraft_dir: None,
            server_jar: None,
            update_manifest: String::new(),
            check_mod_updates: true,
            close_on_launch: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Short free-form note shown on the build card.
    #[serde(default)]
    pub description: String,
    pub player_name: String,
    pub game_dir: String,
    /// Exact Horizon version tag, or None to follow the newest ready version.
    #[serde(default)]
    pub horizon_version: Option<String>,
    #[serde(default)]
    pub skin: Option<String>,
    /// True for the profile GERA created from the player's existing install.
    #[serde(default)]
    pub detected: bool,
    pub created_at: u64,
    #[serde(default)]
    pub last_played: Option<u64>,
    #[serde(default)]
    pub last_exit_code: Option<i32>,
}

/// A multiplayer server the player keeps in GERA. Horizon's own servers.dat is
/// never written: imported entries are ordinary GERA records from that moment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub description: String,
    /// File name inside `<data>/servers`, not an absolute path.
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    /// Letters on the coloured tile when no icon is set.
    #[serde(default)]
    pub fallback_text: String,
    /// Seed colour of the fallback tile and banner, #rrggbb.
    #[serde(default = "default_fallback_color")]
    pub fallback_color: String,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub position: u32,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub last_played: Option<u64>,
}

fn default_port() -> u16 {
    25565
}

fn default_fallback_color() -> String {
    "#6750A4".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub schema: u32,
    pub settings: Settings,
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
    pub active_profile: Option<String>,
    /// Set when a damaged state file was moved aside, for Diagnostics.
    #[serde(skip)]
    pub recovered_from: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            settings: Settings::default(),
            profiles: Vec::new(),
            servers: Vec::new(),
            active_profile: None,
            recovered_from: None,
        }
    }
}

impl Store {
    pub fn active(&self) -> Option<&Profile> {
        let id = self.active_profile.as_ref()?;
        self.profiles.iter().find(|p| &p.id == id)
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn profile_mut(&mut self, id: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.id == id)
    }

    pub fn server(&self, id: &str) -> Option<&ServerEntry> {
        self.servers.iter().find(|s| s.id == id)
    }

    pub fn server_mut(&mut self, id: &str) -> Option<&mut ServerEntry> {
        self.servers.iter_mut().find(|s| s.id == id)
    }
}

pub fn load_or_init(paths: &Paths) -> io::Result<Store> {
    let file = paths.store_file();
    if file.exists() {
        match fs::read(&file).and_then(|bytes| {
            serde_json::from_slice::<Store>(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }) {
            Ok(mut store) => {
                normalise(&mut store);
                return Ok(store);
            }
            Err(err) => {
                // Never lose the player's settings silently: keep the damaged file.
                let backup = file.with_file_name(format!("state.broken-{}.json", now_ms()));
                let _ = fs::rename(&file, &backup);
                let mut store = Store::default();
                store.recovered_from = Some(backup.to_string_lossy().into_owned());
                eprintln!("GERA: state.json was unreadable ({err}); kept as {}", backup.display());
                init_detected_profile(&mut store);
                save(paths, &store)?;
                return Ok(store);
            }
        }
    }

    let mut store = Store::default();
    init_detected_profile(&mut store);
    save(paths, &store)?;
    Ok(store)
}

fn normalise(store: &mut Store) {
    store.schema = SCHEMA;
    let ui = store.settings.ui_mode.to_ascii_lowercase();
    store.settings.ui_mode = if ui == "heavy" { "heavy".into() } else { "light".into() };
    let theme = store.settings.theme.to_ascii_lowercase();
    store.settings.theme = match theme.as_str() {
        "light" | "dark" => theme,
        _ => "system".into(),
    };
    if !is_hex_color(&store.settings.accent) {
        store.settings.accent = Settings::default().accent;
    }
    for value in [
        &mut store.settings.minecraft_dir,
        &mut store.settings.server_jar,
        &mut store.settings.custom_horizon_dir,
    ] {
        if let Some(text) = value.as_mut() {
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                *value = None;
            } else {
                *text = trimmed;
            }
        }
    }
    // The active profile must exist.
    if store
        .active_profile
        .as_ref()
        .map(|id| store.profiles.iter().all(|p| &p.id != id))
        .unwrap_or(true)
    {
        store.active_profile = store.profiles.first().map(|p| p.id.clone());
    }
}

pub fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

/// Build the first profile from the player's existing Horizon install, reading
/// (never writing) the small launcher files and the existing horizon-play script.
fn init_detected_profile(store: &mut Store) {
    let game_dir = paths::horizon_default_game_dir();
    if !game_dir.is_dir() {
        return;
    }

    let player_name = fs::read_to_string(game_dir.join("horizon-player.txt"))
        .ok()
        .and_then(|text| text.lines().next().map(clean_player_name))
        .filter(|name| is_player_name(name))
        .unwrap_or_else(|| "Player".into());

    let candidate = game_dir.join("skins").join(format!("{player_name}.png"));
    let skin = candidate
        .is_file()
        .then(|| candidate.to_string_lossy().into_owned());

    let profile = Profile {
        id: "default".into(),
        name: "Основной".into(),
        description: String::new(),
        player_name,
        game_dir: game_dir.to_string_lossy().into_owned(),
        horizon_version: None,
        skin,
        detected: true,
        created_at: now_ms(),
        last_played: None,
        last_exit_code: None,
    };
    store.profiles.push(profile);
    store.active_profile = Some("default".into());
}

fn clean_player_name(value: &str) -> String {
    value
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .to_string()
}

pub fn is_player_name(value: &str) -> bool {
    let len = value.chars().count();
    (3..=16).contains(&len)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn save(paths: &Paths, store: &Store) -> io::Result<()> {
    paths.ensure()?;
    let file = paths.store_file();
    let tmp = file.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &file)?;
    Ok(())
}

/// Validate a profile coming from the UI. Returns a message for the player.
pub fn validate_profile(profile: &Profile, existing: Option<&Profile>) -> Result<(), String> {
    let name = profile.name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Название сборки: от 1 до 40 символов.".into());
    }
    if !is_player_name(&profile.player_name) {
        return Err(
            "Ник: 3–16 символов, только латинские буквы, цифры и подчёркивание.".into(),
        );
    }
    let game_dir = PathBuf::from(profile.game_dir.trim());
    if !game_dir.is_absolute() {
        return Err("Папка сборки должна быть абсолютным путём.".into());
    }
    if game_dir == Path::new("/") {
        return Err("Нельзя использовать корень файловой системы как папку сборки.".into());
    }
    if let Some(skin) = &profile.skin {
        if !skin.is_empty() {
            let skin_path = PathBuf::from(skin);
            if !skin_path.is_file() {
                return Err(format!("Файл скина не найден: {}", skin_path.display()));
            }
        }
    }
    if let Some(old) = existing {
        if old.detected && PathBuf::from(&old.game_dir) != game_dir {
            return Err(
                "Игровую папку обнаруженной сборки нельзя перемещать из лаунчера.".into(),
            );
        }
    }
    Ok(())
}
