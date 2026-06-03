use crate::core::curseforge::get_cf_files_batch;
use crate::core::modpack::{copy_overrides, extract_zip, parse_modpack_manifest, ModpackType};
use crate::core::mojang::get_minecraft_base;
use crate::downloader::{run_batch_download, DownloadTask};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[tauri::command]
pub async fn install_modpack(
    zip_path: String,
    instance_name: String,
    is_update: bool,
    project_id: Option<String>,
    app: AppHandle,
) -> Result<(), String> {
    tracing::info!(
        "Starting modpack installation: {} -> {}",
        zip_path,
        instance_name
    );

    let base_dir = get_minecraft_base();
    let temp_dir = base_dir.parent().unwrap_or_else(|| std::path::Path::new(".")).join(".dawnland").join("temp").join(Uuid::new_v4().to_string());
    let instance_dir = base_dir.join("versions").join(&instance_name);

    // 1. Emit phase 1: Extracting
    let _ = app.emit(
        "modpack-install-status",
        serde_json::json!({
            "phase": "extracting",
            "message": "Extracting modpack archive..."
        }),
    );

    tokio::task::spawn_blocking({
        let zip = PathBuf::from(zip_path);
        let temp = temp_dir.clone();
        move || extract_zip(zip, temp)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // 2. Parse Manifest
    let _ = app.emit(
        "modpack-install-status",
        serde_json::json!({
            "phase": "parsing",
            "message": "Reading modpack manifest..."
        }),
    );

    let modpack = parse_modpack_manifest(&temp_dir)?;

    let modpack_type_str = match &modpack {
        ModpackType::CurseForge(_) => "CurseForge",
        ModpackType::Modrinth(_) => "Modrinth",
    };

    let (mc_version, loader, tasks, overrides_folder, modpack_version) = match modpack {
        ModpackType::CurseForge(manifest) => {
            let _ = app.emit(
                "modpack-install-status",
                serde_json::json!({
                    "phase": "resolving_urls",
                    "message": "Resolving CurseForge download links..."
                }),
            );

            let mc_version = manifest.minecraft.version.clone();
            let loader = manifest
                .minecraft
                .mod_loaders
                .first()
                .map(|l| l.id.clone())
                .unwrap_or_default();
            let mp_version = manifest.version.clone();

            let file_ids: Vec<u32> = manifest.files.iter().map(|f| f.file_id).collect();

            // Get URLs from Proxy
            let resolved_files = get_cf_files_batch(file_ids).await?;

            let mut tasks = Vec::new();
            for file in resolved_files {
                let dest = instance_dir.join("mods").join(&file.filename);
                tasks.push(DownloadTask::new(
                    file.download_url,
                    dest.to_string_lossy().to_string(),
                    file.hash,
                    file.file_size,
                ));
            }

            (mc_version, loader, tasks, manifest.overrides, mp_version)
        }
        ModpackType::Modrinth(manifest) => {
            let mc_version = manifest.dependencies.minecraft.clone();
            let mut loader = String::new();
            if let Some(forge) = manifest.dependencies.forge {
                loader = format!("forge-{}", forge);
            } else if let Some(fabric) = manifest.dependencies.fabric_loader {
                loader = format!("fabric-{}", fabric);
            } else if let Some(neoforge) = manifest.dependencies.neoforge {
                loader = format!("neoforge-{}", neoforge);
            }
            let mp_version = manifest.version_id.clone();

            let mut tasks = Vec::new();
            for file in manifest.files {
                if let Some(url) = file.downloads.first() {
                    let dest = instance_dir.join(&file.path);
                    let hash = file.hashes.get("sha1").cloned();
                    tasks.push(DownloadTask::new(
                        url.clone(),
                        dest.to_string_lossy().to_string(),
                        hash,
                        Some(file.file_size),
                    ));
                }
            }

            (
                mc_version,
                loader,
                tasks,
                "overrides".to_string(),
                mp_version,
            )
        }
    };

    // 3. Setup Instance
    let _ = app.emit(
        "modpack-install-status",
        serde_json::json!({
            "phase": "setup_instance",
            "message": format!("Preparing instance {}...", instance_name)
        }),
    );

    ensure_dependencies(&mc_version, &loader, app.clone()).await?;

    std::fs::create_dir_all(&instance_dir).map_err(|e| e.to_string())?;

    let version_json = serde_json::json!({
        "id": instance_name,
        "inheritsFrom": if loader.is_empty() { mc_version.clone() } else { loader.clone() },
        "type": "release",
        "mainClass": if loader.contains("fabric") { "net.fabricmc.loader.impl.launch.knot.KnotClient" } else if loader.contains("forge") { "cpw.mods.bootstraplauncher.BootstrapLauncher" } else { "net.minecraft.client.main.Main" },
        "modpackVersion": modpack_version,
        "modpackType": modpack_type_str,
        "modpackProjectId": project_id
    });

    std::fs::write(
        instance_dir.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&version_json).unwrap(),
    )
    .map_err(|e| e.to_string())?;

    // Smart Cleanup if is_update is true
    let modpack_files_path = instance_dir.join("modpack_files.json");

    let mut expected_mod_filenames = std::collections::HashSet::new();
    for task in &tasks {
        if let Some(filename) = std::path::Path::new(&task.dest_path).file_name() {
            expected_mod_filenames.insert(filename.to_string_lossy().to_string());
        }
    }

    if is_update {
        let _ = app.emit(
            "modpack-install-status",
            serde_json::json!({
                "phase": "cleaning",
                "message": "Cleaning up outdated mods..."
            }),
        );

        if let Ok(old_files_json) = tokio::fs::read_to_string(&modpack_files_path).await {
            if let Ok(old_files) = serde_json::from_str::<Vec<String>>(&old_files_json) {
                let mods_dir = instance_dir.join("mods");
                if let Ok(mut entries) = tokio::fs::read_dir(&mods_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        if let Ok(file_type) = entry.file_type().await {
                            if file_type.is_file() {
                                let filename = entry.file_name().to_string_lossy().to_string();
                                // If it was part of the OLD modpack, but NOT in the NEW modpack, DELETE it.
                                if old_files.contains(&filename)
                                    && !expected_mod_filenames.contains(&filename)
                                {
                                    tracing::info!(
                                        "Update cleanup: Deleting outdated modpack mod {}",
                                        filename
                                    );
                                    let _ = tokio::fs::remove_file(entry.path()).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Download Mods
    let _ = app.emit(
        "modpack-install-status",
        serde_json::json!({
            "phase": "downloading_mods",
            "message": format!("Downloading {} mods...", tasks.len()),
            "totalTasks": tasks.len()
        }),
    );

    // Run batch download
    run_batch_download(tasks.clone(), app.clone()).await;

    // 5. Copy Overrides
    let _ = app.emit(
        "modpack-install-status",
        serde_json::json!({
            "phase": "copying_overrides",
            "message": "Applying modpack overrides..."
        }),
    );

    tokio::task::spawn_blocking({
        let temp = temp_dir.clone();
        let inst = instance_dir.clone();
        let ov = overrides_folder.clone();
        move || copy_overrides(&temp, &inst, &ov)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // Write the new modpack_files.json (for backwards compatibility if needed, but we will upgrade to modpack_tasks.json)
    let new_files_list: Vec<String> = expected_mod_filenames.into_iter().collect();
    if let Ok(json_str) = serde_json::to_string_pretty(&new_files_list) {
        let _ = tokio::fs::write(&modpack_files_path, json_str).await;
    }

    // Save the full download tasks for integrity verification
    let modpack_tasks_path = instance_dir.join("modpack_tasks.json");
    if let Ok(json_str) = serde_json::to_string_pretty(&tasks) {
        let _ = tokio::fs::write(&modpack_tasks_path, json_str).await;
    }

    // Clean up
    let _ = std::fs::remove_dir_all(&temp_dir);

    let _ = app.emit(
        "modpack-install-status",
        serde_json::json!({
            "phase": "complete",
            "message": "Modpack installation complete!"
        }),
    );

    Ok(())
}

async fn ensure_dependencies(mc_version: &str, loader: &str, app: AppHandle) -> Result<(), String> {
    let base_dir = crate::core::mojang::get_minecraft_base();

    // Check if loader is empty -> means only vanilla
    if loader.is_empty() {
        let vanilla_json = base_dir
            .join("versions")
            .join(mc_version)
            .join(format!("{}.json", mc_version));
        if !vanilla_json.exists() {
            let _ = app.emit(
                "modpack-install-status",
                serde_json::json!({
                    "phase": "installing_dependency",
                    "message": format!("Installing Minecraft {}...", mc_version)
                }),
            );

            let versions = crate::core::mojang::get_vanilla_versions().await?;
            let version_info = versions
                .into_iter()
                .find(|v| v.id == mc_version)
                .ok_or_else(|| {
                    format!("Minecraft version {} not found in Mojang API", mc_version)
                })?;

            crate::core::mojang::install_vanilla_version(
                mc_version.to_string(),
                version_info.url.clone(),
                Some(true),
                app.clone(),
            )
            .await?;
        }
        return Ok(());
    }

    // Loader is present
    let loader_json = base_dir
        .join("versions")
        .join(loader)
        .join(format!("{}.json", loader));
    if !loader_json.exists() {
        let _ = app.emit(
            "modpack-install-status",
            serde_json::json!({
                "phase": "installing_dependency",
                "message": format!("Installing dependency {}...", loader)
            }),
        );

        if loader.starts_with("fabric-") {
            let loader_version = loader.strip_prefix("fabric-").unwrap().to_string();
            crate::core::fabric::install_fabric_instance(
                mc_version.to_string(),
                loader_version,
                loader.to_string(),
                Some(true),
                app.clone(),
            )
            .await?;
        } else if loader.starts_with("forge-") {
            let loader_version = loader.strip_prefix("forge-").unwrap().to_string();
            crate::core::forge::install_forge_instance(
                mc_version.to_string(),
                loader_version,
                "forge".to_string(),
                loader.to_string(),
                Some(true),
                app.clone(),
            )
            .await?;
        } else if loader.starts_with("neoforge-") {
            let loader_version = loader.strip_prefix("neoforge-").unwrap().to_string();
            crate::core::forge::install_forge_instance(
                mc_version.to_string(),
                loader_version,
                "neoforge".to_string(),
                loader.to_string(),
                Some(true),
                app.clone(),
            )
            .await?;
        } else {
            return Err(format!("Unsupported loader type: {}", loader));
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn get_modpack_name(zip_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let file =
            std::fs::File::open(&zip_path).map_err(|e| format!("Failed to open zip: {}", e))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip: {}", e))?;

        // Check for CurseForge manifest
        if let Ok(mut manifest_file) = archive.by_name("manifest.json") {
            let mut contents = String::new();
            use std::io::Read;
            if manifest_file.read_to_string(&mut contents).is_ok() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                    if let Some(name) = json.get("name").and_then(|n| n.as_str()) {
                        // Curseforge manifest names often have weird characters, we should sanitize it somewhat
                        // But since it's just the default input box text, it's fine.
                        return Ok(name.to_string());
                    }
                }
            }
        }

        // Check for Modrinth manifest
        if let Ok(mut manifest_file) = archive.by_name("modrinth.index.json") {
            let mut contents = String::new();
            use std::io::Read;
            if manifest_file.read_to_string(&mut contents).is_ok() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                    if let Some(name) = json.get("name").and_then(|n| n.as_str()) {
                        return Ok(name.to_string());
                    }
                }
            }
        }

        Err("Could not find manifest.json or modrinth.index.json with a valid name".to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn download_and_install_online_modpack(
    url: String,
    instance_name: String,
    project_id: Option<String>,
    is_update: bool,
    app: AppHandle,
) -> Result<(), String> {
    tracing::info!(
        "Downloading online modpack from {} to {}",
        url,
        instance_name
    );

    let base_dir = crate::core::mojang::get_minecraft_base();
    let temp_dir = base_dir.parent().unwrap_or_else(|| std::path::Path::new(".")).join(".dawnland").join("temp");
    std::fs::create_dir_all(&temp_dir).unwrap_or_default();
    let temp_zip_path = temp_dir.join(format!("{}.zip", uuid::Uuid::new_v4()));

    let _ = app.emit(
        "modpack-install-status",
        serde_json::json!({
            "phase": "downloading_archive",
            "message": "Downloading modpack archive...",
            "progress": 0.0
        }),
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_default();

    let mut response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: {}", response.status()));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut file = tokio::fs::File::create(&temp_zip_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut last_emit_time = tokio::time::Instant::now();
    let mut speed_calc_time = tokio::time::Instant::now();
    let mut last_downloaded: u64 = 0;
    let mut current_speed: f64 = 0.0;

    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        if total_size > 0 && last_emit_time.elapsed().as_millis() > 200 {
            let elapsed_sec = speed_calc_time.elapsed().as_secs_f64();
            if elapsed_sec > 0.5 {
                current_speed = (downloaded - last_downloaded) as f64 / elapsed_sec;
                last_downloaded = downloaded;
                speed_calc_time = tokio::time::Instant::now();
            }

            let progress = (downloaded as f64 / total_size as f64) * 100.0;
            let downloaded_mb = downloaded as f64 / 1024.0 / 1024.0;
            let total_mb = total_size as f64 / 1024.0 / 1024.0;
            let speed_mb = current_speed / 1024.0 / 1024.0;

            let _ = app.emit(
                "modpack-install-status",
                serde_json::json!({
                    "phase": "downloading_archive",
                    "message": "Downloading modpack archive...",
                    "progress": progress,
                    "speedMb": speed_mb,
                    "totalMb": total_mb,
                    "downloadedMb": downloaded_mb
                }),
            );
            last_emit_time = tokio::time::Instant::now();
        }
    }

    let _ = app.emit(
        "modpack-install-status",
        serde_json::json!({
            "phase": "downloading_archive",
            "message": "Download complete. Starting installation...",
            "progress": 100.0
        }),
    );

    // Call existing install_modpack
    let result = install_modpack(
        temp_zip_path.to_string_lossy().to_string(),
        instance_name,
        is_update,
        project_id,
        app,
    )
    .await;
    result
}
