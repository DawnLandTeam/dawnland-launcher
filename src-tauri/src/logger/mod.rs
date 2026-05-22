use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Returns the log directory path: `~/.dawnland/logs`
fn log_dir() -> Result<PathBuf, String> {
    let base = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .ok_or_else(|| "Could not determine local data directory".to_string())?;
    Ok(base.join("dawnland").join("logs"))
}

/// Initialize the global tracing subscriber with dual output:
/// - Console (stdout) for development visibility
/// - Rolling file appender (daily rotation) for persistent logs
pub fn init() -> Result<(), String> {
    let log_dir = log_dir()?;

    // Ensure the log directory exists
    std::fs::create_dir_all(&log_dir).map_err(|e| {
        format!(
            "Failed to create log directory {}: {e}",
            log_dir.display()
        )
    })?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "dawnland.log");
    let (non_blocking_file, _guard) = tracing_appender::non_blocking(file_appender);

    // Prevent the guard from being dropped — it must live for the entire program.
    // Leaking is acceptable here since it only happens once and the guard must
    // outlive all other references to the non-blocking writer.
    std::mem::forget(_guard);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking_file),
        )
        .init();

    tracing::info!("Dawnland Launcher core initialized.");
    tracing::info!("Log directory: {}", log_dir.display());

    Ok(())
}
