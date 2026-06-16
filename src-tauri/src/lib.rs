#![allow(unused)]
mod auth;
mod commands;
mod core;
mod downloader;
mod logger;
mod models;
pub mod error;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

#[cfg(target_os = "windows")]
fn register_deep_link() {
    use winreg::enums::*;
    use winreg::RegKey;
    use std::env;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = "Software\\Classes\\dlml";

    // Register deep link for portable/green version on Windows
    match hkcu.create_subkey(path) {
        Ok((key, _)) => {
            if let Err(e) = key.set_value("", &"URL:dlml") {
                tracing::warn!("Failed to set dlml protocol description in registry: {}", e);
            }
            if let Err(e) = key.set_value("URL Protocol", &"") {
                tracing::warn!("Failed to set URL Protocol value in registry: {}", e);
            }

            match key.create_subkey("shell\\open\\command") {
                Ok((cmd_key, _)) => {
                    match env::current_exe() {
                        Ok(exe_path) => {
                            let exe_path_str = exe_path.to_string_lossy();
                            let command_val = format!("\"{}\" \"%1\"", exe_path_str);
                            if let Err(e) = cmd_key.set_value("", &command_val) {
                                tracing::error!("Failed to set deep link command value in registry: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to get current executable path for deep link registration: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to create shell\\open\\command registry subkey: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to create registry subkey for dlml deep link: {}", e);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize the logging system before anything else.
    // If logging fails to initialize, we log a warning but continue running.
    if let Err(e) = logger::init() {
        eprintln!("Warning: failed to initialize logger: {e}");
    }

    let mut builder = tauri::Builder::default()
        .manage(core::launcher::RunningInstances(std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()))))
        .setup(|app| {
            use tauri::Manager;
            let app_handle = app.handle().clone();
            
            #[cfg(target_os = "windows")]
            register_deep_link();

            let app_dir = core::mojang::get_minecraft_base().parent().unwrap_or_else(|| std::path::Path::new(".")).join(".dawnland");
            std::fs::create_dir_all(&app_dir).unwrap_or_default();
            let db_path = app_dir.join("tasks.db");

            tauri::async_runtime::block_on(async move {
                match core::task::db::TaskDatabase::new(db_path).await {
                    Ok(db) => {
                        cleanup_orphan_temp_files(&app_dir, &db).await;

                        let manager = core::task::TaskManager::new(app_handle.clone(), db).await;
                        app_handle.manage(manager);
                    }
                    Err(e) => {
                        tracing::error!("Failed to initialize task database: {}", e);
                    }
                }
            });

            tauri::async_runtime::spawn(async move {
                crate::core::cache::cleanup_expired_cache().await;
            });

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
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init());

    let aptabase_key = option_env!("APTABASE_KEY")
        .map(String::from)
        .or_else(|| std::env::var("APTABASE_KEY").ok())
        .filter(|k| !k.trim().is_empty());

    let aptabase_url = option_env!("APTABASE_URL")
        .map(String::from)
        .or_else(|| std::env::var("APTABASE_URL").ok())
        .filter(|u| !u.trim().is_empty());

    if let Some(key) = aptabase_key {
        let mut opts = tauri_plugin_aptabase::InitOptions::default();
        if let Some(url) = aptabase_url {
            opts.host = Some(url);
        }
        builder = builder.plugin(tauri_plugin_aptabase::Builder::new(&key).with_options(opts).build());
    }

    builder
        .invoke_handler(tauri::generate_handler![
            greet,
            core::cache::clean_dawnland_cache,
            core::security::generate_api_signature,
            commands::get_system_info,
            commands::get_system_locale,
            commands::get_system_memory,
            commands::batch_download,
            commands::modpack::install_modpack,
            commands::modpack::download_and_install_online_modpack,
            commands::modpack::get_modpack_name,
            core::curseforge::set_curseforge_api_key,
            core::curseforge::get_curseforge_api_key,
            // Auth commands
            commands::get_accounts,
            commands::add_offline_account,
            commands::remove_account,
            commands::start_microsoft_login,
            commands::poll_microsoft_token,
            commands::refresh_microsoft_token,
            commands::login_microsoft_oauth,
            auth::authlib::authenticate_authlib_user,
            auth::authlib::save_authlib_accounts,
            auth::authlib::get_authlib_meta,
            auth::authlib::fetch_authlib_servers,
            auth::authlib::add_authlib_server,
            auth::authlib::remove_authlib_server,
            // Core/Game commands
            core::mojang::get_vanilla_versions,
            core::mojang::install_vanilla_version,
            // removed fetch_install_state
            core::mojang::get_installed_versions,
            // removed cancel_installation
            core::launcher::launch_instance,
            core::launcher::kill_instance,
            core::launcher::is_instance_running,
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
            core::java::fetch_available_javas,
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
            // Task commands
            commands::task::get_task_history,
            commands::task::cancel_task,
            commands::task::clear_task_history,
            commands::task::retry_task,
            // Custom Updater commands
            commands::update_launcher,
            // Settings commands
            core::settings::load_launcher_settings,
            core::settings::save_launcher_settings,

            // Custom Analytics Command
            commands::app_track_event,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn cleanup_orphan_temp_files(app_dir: &std::path::Path, db: &core::task::db::TaskDatabase) {
    if let Ok(tasks) = db.load_all_tasks().await {
        let mut keep_ids = std::collections::HashSet::new();
        for task in tasks {
            if task.status == core::task::TaskStatus::Pending 
                || task.status == core::task::TaskStatus::Running 
                || task.status == core::task::TaskStatus::Paused 
                || task.status == core::task::TaskStatus::Failed {
                keep_ids.insert(task.id);
            }
        }
        
        let temp_dir = app_dir.join("temp");
        if temp_dir.exists() {
            if let Ok(mut entries) = tokio::fs::read_dir(&temp_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_keep = keep_ids.iter().any(|id| name.starts_with(id));
                    
                    if !is_keep {
                        tracing::info!("Cleaning up orphaned temp file/dir: {}", name);
                        if let Ok(meta) = entry.metadata().await {
                            let path = entry.path();
                            if meta.is_dir() {
                                if let Err(err) = tokio::fs::remove_dir_all(&path).await {
                                    tracing::warn!(
                                        "Failed to remove orphaned temp dir {} ({}): {}",
                                        name,
                                        path.display(),
                                        err
                                    );
                                }
                            } else {
                                if let Err(err) = tokio::fs::remove_file(&path).await {
                                    tracing::warn!(
                                        "Failed to remove orphaned temp file {} ({}): {}",
                                        name,
                                        path.display(),
                                        err
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}