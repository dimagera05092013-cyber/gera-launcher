mod clients;
mod commands;
mod horizon;
mod journal;
mod mods;
mod native;
mod net;
mod paths;
mod servers;
mod store;
mod sysutil;
mod update;
mod vanilla;

use commands::AppState;
use tauri::Manager;

/// Headless native build: `gera-launcher --native-build <profile> <project>`.
pub fn native_cli(
    profile: &str,
    project: &str,
    version: Option<String>,
    allow_mismatch: bool,
) -> i32 {
    match native::build_blocking(profile.to_string(), project.to_string(), version, allow_mismatch) {
        Ok(folder) => {
            println!("ГОТОВО: {folder}");
            0
        }
        Err(error) => {
            eprintln!("ОШИБКА: {error}");
            1
        }
    }
}

/// Headless resource import: `gera-launcher --import-resources <версия> <папка>`.
pub fn import_cli(folder: &str, source: &str) -> i32 {
    match clients::import_resources(folder, source) {
        Ok(result) => {
            println!(
                "ГОТОВО: {} (Minecraft {})",
                result.path, result.minecraft
            );
            0
        }
        Err(error) => {
            eprintln!("ОШИБКА: {error}");
            1
        }
    }
}

/// Headless vanilla install: `gera-launcher --java-install <версия>`.
pub fn java_install_cli(id: &str) -> i32 {
    match vanilla::install_cli(id.to_string()) {
        Ok(()) => {
            println!("ГОТОВО: Minecraft {id}");
            0
        }
        Err(error) => {
            eprintln!("ОШИБКА: {error}");
            1
        }
    }
}

/// Headless Horizon preparation: `gera-launcher --horizon-prepare <папка клиента> [<.minecraft>]`.
pub fn prepare_cli(folder: &str, minecraft: &str) -> i32 {
    let mc = if minecraft.trim().is_empty() { None } else { Some(minecraft) };
    match clients::prepare(folder, minecraft, mc) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(error) => {
            eprintln!("ОШИБКА:\n{error}");
            1
        }
    }
}

/// Headless vanilla check: `gera-launcher --java-check <версия>`.
pub fn java_check_cli(id: &str) -> i32 {
    match vanilla::check_blocking(id.to_string()) {
        Ok(result) => {
            println!("{}", result.detail);
            for item in result.missing.iter().take(40) {
                println!("  нет: {item}");
            }
            if result.ok { 0 } else { 1 }
        }
        Err(error) => {
            eprintln!("ОШИБКА: {error}");
            1
        }
    }
}

/// Headless vanilla launch: `gera-launcher --java-play <версия> <ник>`.
pub fn java_play_cli(id: &str, player: &str) -> i32 {
    match vanilla::launch_blocking(None, id.to_string(), player.to_string()) {
        Ok(result) => {
            println!("ГОТОВО: PID {} · лог {}", result.pid, result.log_path);
            0
        }
        Err(error) => {
            eprintln!("ОШИБКА: {error}");
            1
        }
    }
}

/// Windows draws the title bar and menus itself; keep them in step with the
/// theme chosen in GERA. Follows Windows on "system".
pub fn apply_window_theme(app: &tauri::AppHandle, theme: &str) {
    #[cfg(windows)]
    {
        use tauri::{Manager, Theme};
        let theme = match theme {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        };
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_theme(theme);
        }
    }
    #[cfg(not(windows))]
    let _ = (app, theme);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let paths = paths::Paths::new();
            paths.ensure()?;
            let store = store::load_or_init(&paths)?;
            let theme = store.settings.theme.clone();
            app.manage(AppState::new(paths, store));
            apply_window_theme(app.handle(), &theme);
            update::cleanup_old_binary();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::refresh_horizon,
            commands::game_status,
            commands::save_settings,
            commands::save_profile,
            commands::duplicate_profile,
            commands::remove_profile,
            commands::activate_profile,
            commands::launch_game,
            commands::launch_server,
            commands::server_save,
            commands::server_remove,
            commands::server_set_image,
            commands::server_clear_image,
            commands::server_images,
            commands::server_import_scan,
            commands::server_import_add,
            commands::server_reorder,
            commands::server_pin,
            commands::server_set_profile,
            commands::server_status,
            commands::read_game_log,
            commands::diagnostics,
            commands::open_path,
            commands::open_url,
            commands::horizon_releases,
            commands::horizon_install,
            commands::horizon_prepare,
            commands::horizon_import,
            commands::import_folder,
            commands::import_scan,
            commands::mods_catalog,
            commands::mods_installed,
            commands::mods_builtin,
            commands::mods_install,
            commands::mods_remove,
            native::native_status,
            native::native_check,
            native::native_build,
            native::native_cancel,
            native::native_log,
            native::native_builds,
            native::native_remove,
            commands::native_use,
            update::update_check,
            update::update_download,
            update::update_install,
            vanilla::java_versions,
            vanilla::java_install,
            vanilla::java_launch,
            vanilla::java_folder,
            vanilla::java_check,
            vanilla::java_cancel,
            commands::open_logs_folder,
            commands::open_last_log,
            commands::logs_info,
            commands::horizon_check,
        ])
        .run(tauri::generate_context!())
        .expect("GERA Launcher failed to start");
}
