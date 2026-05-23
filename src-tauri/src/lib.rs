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
            commands::batch_download,
            // Auth commands
            commands::get_accounts,
            commands::add_offline_account,
            commands::remove_account,
            commands::start_microsoft_login,
            commands::poll_microsoft_token,
            // Core/Game commands
            core::mojang::get_vanilla_versions,
            core::mojang::install_vanilla_version,
            core::mojang::fetch_install_state,
            core::mojang::get_installed_versions,
            core::launcher::launch_instance,
            core::launcher::get_instance_config,
            core::launcher::save_instance_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}