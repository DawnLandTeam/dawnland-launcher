// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![allow(unused)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Provide a global Tokio reactor context for plugins (like Aptabase) 
    // that might synchronously spawn background tasks during app startup.
    let _rt = tokio::runtime::Runtime::new().expect("Failed to initialize Tokio runtime");
    let _guard = _rt.enter();

    dawnland_launcher_lib::run()
}
