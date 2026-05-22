use std::env::consts;

/// Returns a human-readable OS identifier string.
#[tauri::command]
pub fn get_system_info() -> Result<String, String> {
    let os = consts::OS;
    let arch = consts::ARCH;
    let family = consts::FAMILY;

    let info = format!(
        "Operating System: {os} | Architecture: {arch} | Family: {family}"
    );

    tracing::info!("System info requested: {info}");
    Ok(info)
}
