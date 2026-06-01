mod auth;
mod commands;
mod core;
mod downloader;
mod logger;
mod models;

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
        .manage(core::launcher::RunningInstances(std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()))))
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let size = monitor.size();
                    let scale_factor = monitor.scale_factor();
                    
                    // Convert physical size to logical size based on scale factor
                    let mut width = (size.width as f64 / scale_factor * 0.6) as u32;
                    let mut height = (size.height as f64 / scale_factor * 0.7) as u32;

                    width = width.clamp(800, 1200);
                    height = height.clamp(600, 800);

                    let _ = window.set_size(tauri::LogicalSize::new(width, height));
                }
                
                let _ = window.center();
                let _ = window.show();
            }
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_system_info,
            commands::get_system_memory,
            commands::batch_download,
            commands::modpack::install_modpack,
            commands::modpack::download_and_install_online_modpack,
            commands::modpack::get_modpack_name,
            // Auth commands
            commands::get_accounts,
            commands::add_offline_account,
            commands::remove_account,
            commands::start_microsoft_login,
            commands::poll_microsoft_token,
            commands::refresh_microsoft_token,
            auth::authlib::add_authlib_account,
            auth::authlib::get_authlib_meta,
            auth::authlib::fetch_authlib_servers,
            auth::authlib::add_authlib_server,
            auth::authlib::remove_authlib_server,
            // Core/Game commands
            core::mojang::get_vanilla_versions,
            core::mojang::install_vanilla_version,
            core::mojang::fetch_install_state,
            core::mojang::get_installed_versions,
            core::launcher::launch_instance,
            core::launcher::kill_instance,
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
            core::manager::bind_instance_to_server,
            // Java commands
            core::java::scan_local_javas,
            core::java::clear_java_cache,
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
            core::curseforge::get_cf_files_batch,
            core::curseforge::search_curseforge_modpacks,
            core::curseforge::get_curseforge_modpack_versions,
            // Modrinth commands
            core::modrinth::search_modrinth,
            core::modrinth::get_modrinth_mod_files,
            core::modrinth::get_modrinth_mod_details,
            core::modrinth::get_modrinth_mod_versions,
            core::modrinth::search_modrinth_modpacks,
            core::modrinth::get_modrinth_modpack_versions,
            // Server commands (proxies to Go web backend)
            core::server::get_servers,
            core::server::get_recommended_servers,
            core::server::get_server,
            core::server::create_server,
            core::server::update_server,
            core::server::delete_server,
            core::server::get_pending_servers,
            core::server::approve_server,
            core::server::reject_server,
            core::server::upload_pack_file,
            core::server::download_pack_file,
            core::server::install_server_modpack,
            core::server::get_filter_options,
            core::ping::ping_server,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}