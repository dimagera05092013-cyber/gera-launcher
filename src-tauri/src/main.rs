#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(position) = args.iter().position(|arg| arg == "--native-build") {
        let rest = &args[position + 1..];
        let (Some(profile), Some(project)) = (rest.first(), rest.get(1)) else {
            eprintln!("Использование: gera-launcher --native-build <сборка> <мод> [--version версия] [--allow-mismatch]");
            std::process::exit(2);
        };
        let mut version = None;
        let mut allow_mismatch = false;
        let mut index = 2;
        while index < rest.len() {
            match rest[index].as_str() {
                "--version" => {
                    version = rest.get(index + 1).cloned();
                    index += 2;
                }
                "--allow-mismatch" => {
                    allow_mismatch = true;
                    index += 1;
                }
                _ => index += 1,
            }
        }
        std::process::exit(gera_launcher_lib::native_cli(
            profile,
            project,
            version,
            allow_mismatch,
        ));
    }
    if let Some(position) = args.iter().position(|arg| arg == "--import-resources") {
        let rest = &args[position + 1..];
        let (Some(folder), Some(source)) = (rest.first(), rest.get(1)) else {
            eprintln!("Использование: gera-launcher --import-resources <папка версии> <папка ресурсов>");
            std::process::exit(2);
        };
        std::process::exit(gera_launcher_lib::import_cli(folder, source));
    }
    if let Some(position) = args.iter().position(|arg| arg == "--java-install") {
        let Some(id) = args.get(position + 1) else {
            eprintln!("Использование: gera-launcher --java-install <версия>");
            std::process::exit(2);
        };
        std::process::exit(gera_launcher_lib::java_install_cli(id));
    }
    if let Some(position) = args.iter().position(|arg| arg == "--java-play") {
        let (Some(id), Some(player)) = (args.get(position + 1), args.get(position + 2)) else {
            eprintln!("Использование: gera-launcher --java-play <версия> <ник>");
            std::process::exit(2);
        };
        std::process::exit(gera_launcher_lib::java_play_cli(id, player));
    }
    gera_launcher_lib::run()
}
