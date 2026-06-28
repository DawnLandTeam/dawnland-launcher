use std::sync::OnceLock;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub fn get_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .user_agent("Dawnland-Launcher/1.0")
            .build()
            .unwrap_or_default()
    })
}

/// Helper to parse a reqwest::Response into JSON, propagating I/O errors and logging JSON parsing failures.
pub async fn parse_json_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, String> {
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;
        
    serde_json::from_str(&body).map_err(|e| {
        let snippet = if body.len() > 200 { &body[..200] } else { &body };
        format!("Failed to parse JSON: {} - body: {}", e, snippet)
    })
}

/// Compare two version strings numerically (segment by segment)
/// e.g., "0.9.1" < "0.10.1" < "0.19.1"
#[async_recursion::async_recursion]
pub async fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let ty = entry.file_type().await?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name())).await?;
        } else {
            tokio::fs::copy(entry.path(), dst.join(entry.file_name())).await?;
        }
    }
    Ok(())
}

pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    // If both strings are exactly the same, skip further logic
    if a == b {
        return std::cmp::Ordering::Equal;
    }

    let a_parts: Vec<u32> = a
        .split(['.', '-', '_'])
        .filter_map(|s| s.parse().ok())
        .collect();
    let b_parts: Vec<u32> = b
        .split(['.', '-', '_'])
        .filter_map(|s| s.parse().ok())
        .collect();

    // If parsing fails for both, fall back to string comparison
    if a_parts.is_empty() && b_parts.is_empty() {
        return a.cmp(b);
    }

    for (a_part, b_part) in a_parts.iter().zip(b_parts.iter()) {
        match a_part.cmp(b_part) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    // If all compared parts are equal, shorter version comes first
    a_parts.len().cmp(&b_parts.len())
}

/// Create a tokio Command that hides the console window on Windows.
pub fn create_hidden_command<S: AsRef<std::ffi::OsStr>>(program: S) -> tokio::process::Command {
    let mut std_cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std_cmd.creation_flags(0x08000000);
    }
    tokio::process::Command::from(std_cmd)
}

/// Create a std Command that hides the console window on Windows.
pub fn create_hidden_std_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    let mut std_cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std_cmd.creation_flags(0x08000000);
    }
    std_cmd
}

#[async_recursion::async_recursion]
pub async fn flatten_instance_json_recursive(
    parent_id: &str,
    child_obj: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let base_dir = crate::core::mojang::get_minecraft_base();
    let mut parent_json_path = base_dir
        .join("versions")
        .join(parent_id)
        .join(format!("{}.json", parent_id));

    if !parent_json_path.exists() {
        let dawnland_cache = crate::core::mojang::get_dawnland_cache();
        let cache_json = dawnland_cache
            .join(parent_id)
            .join(format!("{}.json", parent_id));
        if cache_json.exists() {
            parent_json_path = cache_json;
        } else {
            return Err(format!(
                "Parent version {} not found at {:?}",
                parent_id, parent_json_path
            ));
        }
    }

    let parent_content = tokio::fs::read_to_string(&parent_json_path)
        .await
        .map_err(|e| e.to_string())?;
    let mut parent_obj: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&parent_content).map_err(|e| e.to_string())?;

    // Recursive resolution of inheritsFrom
    if let Some(grandparent_id) = parent_obj
        .get("inheritsFrom")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
    {
        flatten_instance_json_recursive(&grandparent_id, &mut parent_obj).await?;
    }

    // Merge logic: Merge child_obj into parent_obj

    // 1. Merge libraries (array append)
    if let Some(child_libs) = child_obj.get("libraries").and_then(|v| v.as_array()) {
        if let Some(parent_libs) = parent_obj
            .get_mut("libraries")
            .and_then(|v| v.as_array_mut())
        {
            parent_libs.extend(child_libs.clone());
        } else {
            parent_obj.insert(
                "libraries".to_string(),
                serde_json::Value::Array(child_libs.clone()),
            );
        }
    }

    // 2. Merge arguments (minecraftArguments or arguments.game/jvm)
    if let Some(child_mc_args) = child_obj.get("minecraftArguments") {
        parent_obj.insert("minecraftArguments".to_string(), child_mc_args.clone());
    }

    if let Some(child_args) = child_obj.get("arguments").and_then(|v| v.as_object()) {
        if !parent_obj.contains_key("arguments") {
            parent_obj.insert(
                "arguments".to_string(),
                serde_json::Value::Object(serde_json::Map::new()),
            );
        }
        if let Some(parent_args) = parent_obj
            .get_mut("arguments")
            .and_then(|v| v.as_object_mut())
        {
            for (key, val) in child_args {
                if let Some(val_arr) = val.as_array() {
                    if let Some(p_val_arr) = parent_args.get_mut(key).and_then(|v| v.as_array_mut())
                    {
                        p_val_arr.extend(val_arr.clone());
                    } else {
                        parent_args.insert(key.clone(), serde_json::Value::Array(val_arr.clone()));
                    }
                }
            }
        }
    }

    // 3. Override other keys (mainClass, id, type, etc.)
    for (key, val) in child_obj.iter() {
        if key == "libraries"
            || key == "arguments"
            || key == "inheritsFrom"
            || key == "minecraftArguments"
        {
            continue;
        }
        parent_obj.insert(key.clone(), val.clone());
    }

    // Clear inheritsFrom since it's fully resolved
    parent_obj.remove("inheritsFrom");

    // Replace child_obj with the fully merged parent_obj
    *child_obj = parent_obj;

    Ok(())
}

pub async fn copy_jar_if_exists_with_logging<
    S: AsRef<std::path::Path>,
    D: AsRef<std::path::Path>,
>(
    src: S,
    dest: D,
) {
    let src_ref = src.as_ref();
    let dest_ref = dest.as_ref();

    match tokio::fs::metadata(src_ref).await {
        Ok(_) => {
            if let Err(err) = tokio::fs::copy(src_ref, dest_ref).await {
                tracing::warn!(
                    "Failed to copy jar from '{}' to '{}': {:#}",
                    src_ref.display(),
                    dest_ref.display(),
                    err
                );
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // Source jar doesn't exist — nothing to do.
        }
        Err(err) => {
            tracing::warn!(
                "Failed to stat jar source '{}' for copy to '{}': {:#}",
                src_ref.display(),
                dest_ref.display(),
                err
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_versions() {
        assert_eq!(compare_versions("1.0", "1.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_versions("1.0.0", "1.0.1"), std::cmp::Ordering::Less);
        assert_eq!(
            compare_versions("1.10.1", "1.9.5"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_versions("2.0-beta", "2.0-alpha"),
            std::cmp::Ordering::Equal
        ); // Note: Current logic drops non-numbers
        assert_eq!(compare_versions("1.2", "1.2.3"), std::cmp::Ordering::Less);
    }
}
