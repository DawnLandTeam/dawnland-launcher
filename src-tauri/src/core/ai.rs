use crate::core::settings::{get_launcher_settings_sync, AiProviderType};
use crate::core::mojang::get_dawnland_dir;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use crate::error::AppError;
use crate::downloader::{DownloadTask, run_batch_download};
use reqwest::Client;
use std::sync::atomic::AtomicBool;
use tauri::{AppHandle, Emitter};

type EngineState = Arc<Mutex<Option<(Child, u16)>>>;
static LLAMA_ENGINE: std::sync::OnceLock<EngineState> = std::sync::OnceLock::new();

fn get_engine_lock() -> EngineState {
    LLAMA_ENGINE.get_or_init(|| Arc::new(Mutex::new(None))).clone()
}

fn get_models_dir() -> PathBuf {
    get_dawnland_dir().join("models")
}

fn get_engine_dir() -> PathBuf {
    get_dawnland_dir().join("ai_engine")
}

fn get_llama_server_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    let bin_name = "llama-server.exe";
    #[cfg(not(target_os = "windows"))]
    let bin_name = "llama-server";
    
    get_engine_dir().join(bin_name)
}

#[tauri::command]
pub async fn list_local_models() -> Result<Vec<String>, String> {
    let models_dir = get_models_dir();
    
    if !models_dir.exists() {
        return Ok(Vec::new());
    }

    let mut models = Vec::new();
    let mut entries = tokio::fs::read_dir(models_dir)
        .await
        .map_err(|e| format!("Failed to read models directory: {}", e))?;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "gguf" {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.contains("-of-") && !name.contains("-00001-of-") {
                            continue;
                        }
                        models.push(name.to_string());
                    }
                }
            }
        }
    }

    Ok(models)
}

#[derive(Serialize, Clone)]
pub struct ModelDownloadProgress {
    pub url: String,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(serde::Deserialize)]
pub struct ModelDownloadTarget {
    pub url: String,
    pub filename: String,
}

#[tauri::command]
pub async fn download_model(targets: Vec<ModelDownloadTarget>, main_filename: String, app: AppHandle) -> Result<(), AppError> {
    let models_dir = get_models_dir();
    tokio::fs::create_dir_all(&models_dir).await.map_err(|e| AppError::from(e.to_string()))?;
    
    let mut tasks = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        let dest_path = models_dir.join(&target.filename);
        let dest_path_str = dest_path.to_string_lossy().into_owned();
        let task_id = if targets.len() > 1 {
            format!("{}|{}", main_filename, i)
        } else {
            main_filename.clone()
        };
        tasks.push(DownloadTask {
            id: task_id,
            url: target.url.clone(),
            dest_path: dest_path_str,
            hash: None,
            expected_size: None,
        });
    }
    
    // Run download in background
    tokio::spawn(async move {
        let cancel_flag = std::sync::Arc::new(AtomicBool::new(false));
        if let Err(e) = run_batch_download(tasks, app.clone(), cancel_flag).await {
            tracing::error!("Failed to download model via batch downloader: {}", e);
        } else {
            tracing::info!("Model {} downloaded successfully", main_filename);
            let _ = app.emit("model-download-complete", main_filename);
        }
    });
    
    Ok(())
}

#[tauri::command]
pub async fn download_engine(app: AppHandle) -> Result<(), AppError> {
    let engine_dir = get_engine_dir();
    tokio::fs::create_dir_all(&engine_dir).await.map_err(|e| AppError::from(e.to_string()))?;
    
    // Fetch latest tag
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::from(e.to_string()))?;
        
    let res = client.get("https://github.com/ggerganov/llama.cpp/releases/latest")
        .send()
        .await
        .map_err(|e| AppError::from(format!("Failed to fetch latest release (Network error): {}", e)))?;
        
    if !res.status().is_success() {
        return Err(AppError::from(format!("Failed to fetch latest release (HTTP {}): {}", res.status(), res.text().await.unwrap_or_default())));
    }
        
    let final_url = res.url().as_str();
    let tag = final_url.split('/').next_back().unwrap_or("b3248").to_string();
    tracing::info!("Resolved latest llama.cpp tag: {}", tag);
    
    #[cfg(target_os = "windows")]
    let url = format!("https://github.com/ggerganov/llama.cpp/releases/download/{}/llama-{}-bin-win-vulkan-x64.zip", tag, tag);
    #[cfg(not(target_os = "windows"))]
    let url = format!("https://github.com/ggerganov/llama.cpp/releases/download/{}/llama-{}-bin-ubuntu-x64.zip", tag, tag);
    
    let zip_name = "engine.zip";
    let zip_path = engine_dir.join(zip_name);
    
    let task = DownloadTask {
        id: "engine_zip".to_string(),
        url,
        dest_path: zip_path.to_string_lossy().into_owned(),
        hash: None,
        expected_size: None,
    };
    
    tokio::spawn(async move {
        let cancel_flag = std::sync::Arc::new(AtomicBool::new(false));
        tracing::info!("Trying to download engine directly from GitHub");
        if let Err(e) = run_batch_download(vec![task], app.clone(), cancel_flag).await {
            tracing::error!("Failed to download AI engine: {}", e);
            let _ = app.emit("engine-download-error", format!("引擎下载失败 (Engine download failed): {}", e));
        } else {
            tracing::info!("Engine downloaded, extracting...");
            if let Err(e) = crate::core::modpack::extract_zip(&zip_path, &engine_dir).await {
                tracing::error!("Failed to extract engine: {}", e);
                let _ = app.emit("engine-download-error", format!("引擎解压失败: {}", e));
            } else {
                let _ = tokio::fs::remove_file(&zip_path).await;
                tracing::info!("Engine downloaded and extracted successfully");
                let _ = app.emit("engine-download-complete", ());
            }
        }
    });
    
    Ok(())
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: Option<u32>,
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

/// SSE stream chunk from an OpenAI-compatible /chat/completions stream response.
#[derive(Deserialize, Debug)]
struct ChatStreamChunk {
    choices: Vec<ChatStreamChoice>,
}

#[derive(Deserialize, Debug)]
struct ChatStreamChoice {
    delta: ChatStreamDelta,
}

#[derive(Deserialize, Debug)]
struct ChatStreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize, Debug)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize, Debug)]
struct ChatMessageResponse {
    content: String,
}

fn get_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(18080)
}

pub async fn start_llama_engine(model_name: &str) -> Result<u16, String> {
    let engine_lock_arc = get_engine_lock();
    let mut engine_lock = engine_lock_arc.lock().await;
    if let Some((_, port)) = engine_lock.as_ref() {
        return Ok(*port); // Return existing port if already running
    }

    let bin_path = get_llama_server_path();
    if !bin_path.exists() {
        return Err("llama-server executable not found. Please download it first.".to_string());
    }

    let model_path = get_models_dir().join(model_name);
    if !model_path.exists() {
        return Err(format!("Model {} not found.", model_name));
    }

    let port = get_free_port();
    
    let log_file = std::fs::File::create(get_engine_dir().join("llama-server.log"))
        .map_err(|e| format!("Failed to create log file: {}", e))?;
        
    let settings = crate::core::settings::get_launcher_settings_sync();
    
    // Pre-flight memory check
    let model_meta = std::fs::metadata(&model_path)
        .map_err(|e| format!("Failed to read model metadata: {}", e))?;
    let model_size_mb = (model_meta.len() / 1024 / 1024) as u32;
    // Rough estimation: 8192 context ~ 1GB RAM, 0 (unlocked) ~ 2GB RAM.
    let ctx_ram_estimation = if settings.ai_config.unlock_context_size { 2048 } else { 1024 };
    let estimated_total_ram = model_size_mb + ctx_ram_estimation;
    
    if estimated_total_ram > settings.ai_config.max_ram_usage {
        return Err(format!(
            "Insufficient Max RAM Limit. The model ({} MB) and context (~{} MB) are estimated to require {} MB of RAM, which exceeds your configured limit of {} MB. Please increase the Max RAM Limit in Settings.",
            model_size_mb, ctx_ram_estimation, estimated_total_ram, settings.ai_config.max_ram_usage
        ));
    }

    let ctx_size = if settings.ai_config.unlock_context_size { "0" } else { "8192" };

    let mut cmd = Command::new(&bin_path);
    cmd.current_dir(get_engine_dir())
       .arg("-m")
       .arg(&model_path)
       .arg("-c")
       .arg(ctx_size)
       .arg("--port")
       .arg(port.to_string())
       .stdout(log_file.try_clone().unwrap())
       .stderr(log_file);

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn llama-server: {}", e))?;
    
    // Wait for the server to bind and be ready by polling /v1/models
    let client = reqwest::Client::new();
    let mut ready = false;
    for _ in 0..60 { // wait up to 60 seconds for large models to load
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        
        // Also check if process crashed
        if let Ok(Some(status)) = child.try_wait() {
            let mut err_msg = format!("llama-server exited unexpectedly with status: {}", status);
            let log_path = get_engine_dir().join("llama-server.log");
            if let Ok(log_content) = std::fs::read_to_string(&log_path) {
                let lines: Vec<&str> = log_content.lines().collect();
                let tail = if lines.len() > 10 { &lines[lines.len()-10..] } else { &lines[..] };
                err_msg = format!("{}\n\nEngine Log:\n{}", err_msg, tail.join("\n"));
            }
            return Err(err_msg);
        }
        
        if let Ok(res) = client.get(format!("http://localhost:{}/v1/models", port)).send().await {
            if res.status().is_success() {
                ready = true;
                break;
            }
        }
    }
    
        if !ready {
            let _ = child.kill().await;
            return Err("AI 引擎启动超时 (可能模型过大加载缓慢，或内存不足)。".to_string());
        }

        *engine_lock = Some((child, port));

        Ok(port)
    }

    pub async fn stop_llama_engine() {
        let engine_lock_arc = get_engine_lock();
        let mut engine_lock = engine_lock_arc.lock().await;
        if let Some((mut engine, _)) = engine_lock.take() {
            let _ = engine.kill().await;
        }
    }

/// Helper to get the latest crash report from a game directory
pub async fn get_latest_crash_report(game_dir: &Path, since: std::time::SystemTime) -> (Option<String>, bool) {
    let crash_dir = game_dir.join("crash-reports");
    let mut latest_file: Option<(PathBuf, std::time::SystemTime)> = None;
    
    if crash_dir.exists() {
        if let Ok(mut entries) = fs::read_dir(&crash_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|e| e == "txt") {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            if let Some((_, latest_time)) = latest_file.clone() {
                                if modified > latest_time {
                                    latest_file = Some((path, modified));
                                }
                            } else {
                                latest_file = Some((path, modified));
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some((path, modified)) = latest_file {
        if modified >= since {
            return (fs::read_to_string(path).await.ok(), true);
        }
    }

    // Fallback to latest.log if no recent crash report exists
    let log_path = game_dir.join("logs").join("latest.log");
    if log_path.exists() {
        if let Ok(content) = fs::read_to_string(&log_path).await {
            // Return only the last 150 lines to avoid massive context
            let lines: Vec<&str> = content.lines().collect();
            let start = if lines.len() > 150 { lines.len() - 150 } else { 0 };
            return (Some(lines[start..].join("\n")), false);
        }
    }

    (None, false)
}

#[derive(Deserialize, Debug)]
struct ModelData {
    id: String,
}

#[derive(Deserialize, Debug)]
struct ModelListResponse {
    data: Vec<ModelData>,
}

#[tauri::command]
pub async fn fetch_remote_models(base_url: String, api_key: String) -> Result<Vec<String>, AppError> {
    let client = Client::new();
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    
    let mut request = client.get(&url);
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    
    let response = request.send()
        .await
        .map_err(|e| AppError::from(format!("API request failed: {}", e)))?;
        
    if !response.status().is_success() {
        let err_text = response.text().await.unwrap_or_default();
        return Err(AppError::from(format!("API error: {}", err_text)));
    }
    
    let model_res: ModelListResponse = response.json().await
        .map_err(|e| AppError::from(format!("Failed to parse response: {}", e)))?;
        
    let models = model_res.data.into_iter().map(|m| m.id).collect();
    Ok(models)
}

fn denoise_crash_log(log: &str) -> String {
    let mut denoised = Vec::new();
    let mut skip_stack_trace = false;

    for line in log.lines() {
        let lower = line.to_lowercase();
        
        if lower.contains("cancellationexception") {
            skip_stack_trace = true;
            continue;
        }
        
        let is_stack_trace = line.trim_start().starts_with("at ") || line.trim_start().starts_with("... ");
        
        if skip_stack_trace && is_stack_trace {
            continue;
        } else if !is_stack_trace {
            skip_stack_trace = false;
        }

        let is_error_or_warn = lower.contains("error") 
            || lower.contains("warn")
            || lower.contains("exception")
            || lower.contains("failed")
            || lower.contains("missing")
            || lower.contains("incompatible");

        let is_info_or_debug = lower.contains("/info]") 
            || lower.contains("info/")
            || lower.contains("info:")
            || lower.contains("/debug]") 
            || lower.contains("debug/")
            || lower.contains("gl info:")
            || lower.contains("jvm uptime");

        if is_error_or_warn || !is_info_or_debug {
            denoised.push(line);
        }
    }

    let result = denoised.join("\n");
    if result.trim().is_empty() {
        log.to_string()
    } else {
        result
    }
}

#[tauri::command]
pub async fn analyze_crash(crash_log: String, language: Option<String>, context_type: Option<String>, app: AppHandle) -> Result<String, AppError> {
    let settings = get_launcher_settings_sync();
    let ai_config = settings.ai_config;
    
    let (base_url, api_key, model) = match ai_config.provider_type {
        AiProviderType::RemoteApi => {
            let url = ai_config.remote_base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let key = ai_config.remote_api_key.unwrap_or_default();
            let model_name = ai_config.remote_model.unwrap_or_else(|| "gpt-3.5-turbo".to_string());
            (url, key, model_name)
        }
        AiProviderType::EmbeddedLlm => {
            let model_name = ai_config.active_embedded_model
                .ok_or_else(|| AppError::from("No active embedded model selected. Please select one in settings.".to_string()))?;
            
            let port = start_llama_engine(&model_name)
                .await
                .map_err(|e| AppError::from(e.to_string()))?;
                
            (format!("http://localhost:{}/v1", port), "".to_string(), model_name)
        }
    };
    
    let lang_str = match language.as_deref() {
        Some("zh-CN") | Some("zh") | Some("zh-Hans") => "Simplified Chinese (简体中文)",
        Some("zh-TW") | Some("zh-HK") | Some("zh-Hant") => "Traditional Chinese (繁体中文)",
        Some("en-US") | Some("en") => "English",
        Some("ja") => "Japanese (日本語)",
        Some("ko") => "Korean (한국어)",
        Some(other) => other,
        None => "English", // Default
    };
    
    let (role_prompt, rules_prompt) = match context_type.as_deref() {
        Some("task") => {
            (
                "You are an expert software diagnostic analyzer.\n\
                You must diagnose why a launcher background task (e.g. downloading, extracting, installing) failed based on the provided error logs.",
                "RULES FOR 'actions' ARRAY (This is a JSON array of objects):\n\
                - If the failure seems temporary (e.g. network timeout or file lock), provide a retry action. Example: [{{ \"type\": \"retry-task\", \"payload\": \"\" }}]\n\
                - If the issue is related to Java configuration, you can provide a goto action. Example: [{{ \"type\": \"goto\", \"payload\": \"settings-java\" }}]\n\
                - If no specific action is needed, return an empty array: []"
            )
        },
        _ => { // "crash" or None
            (
                "You are an expert Minecraft crash log analyzer.\n\
                You must diagnose why the game crashed based on the provided crash logs.",
                "RULES FOR 'actions' ARRAY (This is a JSON array of objects):\n\
                - If a mod is missing, you MUST provide a search action. Example: [{{ \"type\": \"search-mod\", \"payload\": \"example_mod_name\" }}]\n\
                - If two mods are conflicting, you MUST provide a disable-mod action with the mod name (mod_id or filename) to disable. Example: [{{ \"type\": \"disable-mod\", \"payload\": \"create\" }}]\n\
                - If there is a Java version mismatch, you MUST provide a goto action. Example: [{{ \"type\": \"goto\", \"payload\": \"settings-java\" }}]\n\
                - If no specific action is needed, return an empty array: []"
            )
        }
    };

    let system_prompt = format!(
        "{}\n\
        CRITICAL REQUIREMENT: You MUST reply entirely in {}. The cause and solution fields MUST be written in {}!\n\n\
        You MUST return your analysis strictly in the following JSON format, and NOTHING ELSE:\n\
        {{\n\
            \"cause\": \"Brief explanation of the cause in {}\",\n\
            \"solution\": \"Proposed solution in {}\",\n\
            \"actions\": []\n\
        }}\n\n\
        {}\n\n\
        Do NOT wrap the JSON in Markdown code blocks. Output pure JSON only.",
        role_prompt,
        lang_str,
        lang_str,
        lang_str,
        lang_str,
        rules_prompt
    );
    
    let mut final_log = denoise_crash_log(&crash_log);
    if final_log.len() > 8000 {
        let truncate_msg = "\n...[truncated for length]...\n";
        let mut start_idx = final_log.len() - 8000;
        while start_idx < final_log.len() && !final_log.is_char_boundary(start_idx) {
            start_idx += 1;
        }
        final_log = format!("{}{}", truncate_msg, &final_log[start_idx..]);
    }
    
    let user_prompt = format!(
        "Review the following crash log and explain the cause and solution. Limit your response to 150 words.\n\
        CRITICAL: The values in your JSON response MUST be written in {0}! If {0} is Chinese, you MUST reply in 中文!\n\n\
        Crash Log:\n{1}",
        lang_str, final_log
    );
    
    tracing::info!("--- AI ANALYZE CRASH CALLED ---");
    tracing::info!("Original Log Length: {}", crash_log.len());
    tracing::info!("Denoised Log Length: {}", final_log.len());
    tracing::info!("Final Log Sent to AI:\n{}", final_log);
    
    let request = ChatRequest {
        model,
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt,
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt,
            }
        ],
        temperature: 0.1,
        max_tokens: Some(2048),
        stop: Some(vec!["<|im_end|>".to_string(), "<|endoftext|>".to_string()]),
        stream: Some(true),
    };

    // Notify frontend that streaming analysis has started.
    let _ = app.emit("ai-analysis-started", ());

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::from(format!("Failed to build client: {}", e)))?;

    let mut req = client.post(format!("{}/chat/completions", base_url)).json(&request);

    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }

    let response = req.send()
        .await
        .map_err(|e| {
            let msg = format!("API request failed: {}", e);
            let _ = app.emit("ai-analysis-error", &msg);
            AppError::from(msg)
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let err_text = response.text().await.unwrap_or_default();
        let msg = format!("API error ({}): {}", status, err_text);
        let _ = app.emit("ai-analysis-error", &msg);
        return Err(AppError::from(msg));
    }

    // Stream the response body as SSE and forward each content delta to the frontend.
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut full_content = String::new();
    let mut buffer = String::new();
    let mut stream_done = false;

    while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("Stream error: {}", e);
                let _ = app.emit("ai-analysis-error", &msg);
                return Err(AppError::from(msg));
            }
        };

        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete lines from the buffer (SSE is line-delimited).
        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            // Retain the remainder (may be a partial line continued in the next chunk).
            buffer = buffer[newline_pos + 1..].to_string();

            // Skip empty lines and SSE comments (lines starting with ':').
            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            // SSE data lines are prefixed with "data: ".
            let data = match line.strip_prefix("data: ") {
                Some(d) => d.trim(),
                None => continue,
            };

            // "[DONE]" marks the end of the stream.
            if data == "[DONE]" {
                stream_done = true;
                break;
            }

            // Parse the JSON chunk and extract the content delta.
            if let Ok(chunk_data) = serde_json::from_str::<ChatStreamChunk>(data) {
                if let Some(content) = chunk_data
                    .choices
                    .first()
                    .and_then(|c| c.delta.content.as_ref())
                {
                    if !content.is_empty() {
                        full_content.push_str(content);
                        let _ = app.emit("ai-analysis-chunk", content);
                    }
                }
            }
        }

        if stream_done {
            break;
        }
    }

    // For embedded, stop the engine after analysis completes.
    if ai_config.provider_type == AiProviderType::EmbeddedLlm {
        stop_llama_engine().await;
    }

    if full_content.trim().is_empty() {
        let msg = "Empty response from AI".to_string();
        let _ = app.emit("ai-analysis-error", &msg);
        return Err(AppError::from(msg));
    }

    // Notify frontend that streaming has completed with the full text.
    let _ = app.emit("ai-analysis-completed", &full_content);

    Ok(full_content)
}
