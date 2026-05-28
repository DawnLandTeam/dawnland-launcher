mod auth;
mod commands;
mod core;
mod downloader;
mod logger;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize the logging system before anything else.
    // If logging fails to initialize, we log a warning but continue running.
    if let Err(e) = logger::init() {
        eprintln!("Warning: failed to initialize logger: {e}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_system_info,
            commands::get_system_memory,
            commands::batch_download,
            // Auth commands
            commands::get_accounts,
            commands::add_offline_account,
            commands::remove_account,
            commands::start_microsoft_login,
            commands::poll_microsoft_token,
            commands::refresh_microsoft_token,
            // Core/Game commands
            core::mojang::get_vanilla_versions,
            core::mojang::install_vanilla_version,
            core::mojang::fetch_install_state,
            core::mojang::get_installed_versions,
            core::launcher::launch_instance,
            core::launcher::get_instance_config,
            core::launcher::save_instance_config,
            // Fabric commands
            core::fabric::get_fabric_loaders,
            core::fabric::install_fabric_instance,
            core::fabric::check_vanilla_installed,
            // Forge commands
            core::forge::get_forge_loaders,
            core::forge::get_neoforge_loaders,
            core::forge::install_forge_instance,
            // Manager commands
            core::manager::scan_installed_instances,
            core::manager::get_instance_details,
            core::manager::delete_instance,
            core::manager::open_instance_folder,
            // Local mod management commands
            core::manager::get_installed_mods,
            core::manager::toggle_mod_status,
            core::manager::delete_local_mod,
            core::manager::install_mod_to_instance,
            // Java commands
            core::java::scan_local_javas,
            core::java::download_java,
            core::java::get_recommended_java,
            core::java::add_manual_java,
            core::java::remove_java,
            core::java::get_java_download_path,
            core::java::set_java_download_path,
            core::java::scan_full_disk,
            // CurseForge commands
            core::curseforge::search_curseforge,
            core::curseforge::get_cf_mod_files,
            core::curseforge::get_cf_mod_details,
            // Modrinth commands
            core::modrinth::search_modrinth,
            core::modrinth::get_modrinth_mod_files,
            core::modrinth::get_modrinth_mod_details,
            core::modrinth::get_modrinth_mod_versions,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}