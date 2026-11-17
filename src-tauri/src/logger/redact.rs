use crate::core::utils::truncate_utf8;

/// Redacts sensitive tokens, API keys, and credentials from log messages.
pub fn redact_sensitive(input: &str) -> String {
    let mut out = input.to_string();

    // Redact CLI flag pairs such as `--accessToken <token>` or `--accessToken=<token>`
    out = redact_flag_value(&out, "--accessToken");
    out = redact_flag_value(&out, "--userProperties");
    out = redact_kv_secret(&out, "-Dhttp.proxyPassword=");
    out = redact_kv_secret(&out, "-Dhttps.proxyPassword=");

    // Redact HTTP Bearer tokens
    out = redact_prefix_token(&out, "Bearer ");
    out = redact_prefix_token(&out, "bearer ");

    // Redact API keys and OpenAI-style sk- keys
    out = redact_kv_secret(&out, "x-api-key=");
    out = redact_kv_secret(&out, "x-api-key: ");
    out = redact_prefix_token(&out, "sk-proj-");
    out = redact_prefix_token(&out, "sk-");

    // Redact common JSON secret fields
    for key in [
        "access_token",
        "refresh_token",
        "accessToken",
        "refreshToken",
        "remoteApiKey",
        "clientToken",
        "device_code",
        "xsts_token",
        "Token",
    ] {
        out = redact_json_string_field(&out, key);
    }

    out
}

/// Redacts local OS user directory names when exporting diagnostic bundles for sharing.
pub fn redact_user_paths(input: &str) -> String {
    let mut out = input.to_string();
    for prefix in ["C:\\Users\\", "C:/Users/", "/Users/", "/home/"] {
        out = mask_path_segment_after(&out, prefix);
    }
    out
}

/// Redacts sensitive CLI arguments before logging JVM or game launch arguments.
pub fn redact_cli_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut redact_next = false;

    for arg in args {
        if redact_next {
            out.push("***".to_string());
            redact_next = false;
            continue;
        }

        if arg == "--accessToken" || arg == "--userProperties" {
            out.push(arg.clone());
            redact_next = true;
        } else if arg.starts_with("--accessToken=") {
            out.push("--accessToken=***".to_string());
        } else if arg.starts_with("-Dhttp.proxyPassword=") {
            out.push("-Dhttp.proxyPassword=***".to_string());
        } else if arg.starts_with("-Dhttps.proxyPassword=") {
            out.push("-Dhttps.proxyPassword=***".to_string());
        } else {
            out.push(redact_sensitive(arg));
        }
    }

    out
}

/// Truncates a string cleanly on UTF-8 boundaries and redacts sensitive fields before logging.
pub fn truncate_for_log(input: &str, max_bytes: usize) -> String {
    let redacted = redact_sensitive(input);
    if redacted.len() <= max_bytes {
        return redacted;
    }
    let slice = truncate_utf8(&redacted, max_bytes);
    format!("{slice}... [truncated, total {} bytes]", redacted.len())
}

fn redact_flag_value(input: &str, flag: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(idx) = rest.find(flag) {
        let before = &rest[..idx];
        result.push_str(before);
        result.push_str(flag);

        let after_flag = &rest[idx + flag.len()..];
        if let Some(after_eq) = after_flag.strip_prefix('=') {
            result.push('=');
            let token_len = after_eq
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ']'))
                .unwrap_or(after_eq.len());
            result.push_str("***");
            rest = &after_eq[token_len..];
        } else if let Some(first_ws) = after_flag.chars().next().filter(|c| c.is_whitespace()) {
            result.push(first_ws);
            let trimmed_start = after_flag[first_ws.len_utf8()..].trim_start();
            let ws_len = after_flag.len() - first_ws.len_utf8() - trimmed_start.len();
            result.push_str(&after_flag[first_ws.len_utf8()..first_ws.len_utf8() + ws_len]);
            let token_len = trimmed_start
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ']'))
                .unwrap_or(trimmed_start.len());
            if token_len > 0 {
                result.push_str("***");
                rest = &trimmed_start[token_len..];
            } else {
                rest = trimmed_start;
            }
        } else {
            rest = after_flag;
        }
    }

    result.push_str(rest);
    result
}

fn redact_kv_secret(input: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(idx) = rest.find(prefix) {
        result.push_str(&rest[..idx + prefix.len()]);
        let after = &rest[idx + prefix.len()..];
        let token_len = after
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | '&'))
            .unwrap_or(after.len());
        if token_len > 0 {
            result.push_str("***");
        }
        rest = &after[token_len..];
    }

    result.push_str(rest);
    result
}

fn redact_prefix_token(input: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(idx) = rest.find(prefix) {
        // Avoid rewriting words where `sk-` is part of an identifier (e.g. `disk-info`)
        let prev_ok = idx == 0
            || !rest[..idx]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if !prev_ok {
            result.push_str(&rest[..idx + prefix.len()]);
            rest = &rest[idx + prefix.len()..];
            continue;
        }

        result.push_str(&rest[..idx + prefix.len()]);
        let after = &rest[idx + prefix.len()..];
        let token_len = after
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | ')' | ']'))
            .unwrap_or(after.len());
        if token_len >= 6 {
            result.push_str("***");
            rest = &after[token_len..];
        } else {
            result.push_str(&after[..token_len]);
            rest = &after[token_len..];
        }
    }

    result.push_str(rest);
    result
}

fn redact_json_string_field(input: &str, field_name: &str) -> String {
    let pattern = format!("\"{}\"", field_name);
    let mut result = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(idx) = rest.find(&pattern) {
        result.push_str(&rest[..idx + pattern.len()]);
        let after_key = &rest[idx + pattern.len()..];

        let ws_and_colon = after_key.trim_start();
        let leading_ws_len = after_key.len() - ws_and_colon.len();
        if let Some(after_colon) = ws_and_colon.strip_prefix(':') {
            let after_colon_trimmed = after_colon.trim_start();
            let post_colon_ws_len = after_colon.len() - after_colon_trimmed.len();
            if let Some(after_quote) = after_colon_trimmed.strip_prefix('"') {
                result.push_str(&after_key[..leading_ws_len]);
                result.push(':');
                result.push_str(&after_colon[..post_colon_ws_len]);
                result.push('"');

                // Find closing quote while respecting escaped quotes
                let mut escaped = false;
                let mut end_idx = None;
                for (i, ch) in after_quote.char_indices() {
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if ch == '\\' {
                        escaped = true;
                        continue;
                    }
                    if ch == '"' {
                        end_idx = Some(i);
                        break;
                    }
                }
                if let Some(end) = end_idx {
                    result.push_str("***\"");
                    rest = &after_quote[end + 1..];
                    continue;
                }
            }
        }

        rest = after_key;
    }

    result.push_str(rest);
    result
}

fn mask_path_segment_after(input: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    let lower_prefix = prefix.to_ascii_lowercase();

    loop {
        let lower_rest = rest.to_ascii_lowercase();
        let Some(idx) = lower_rest.find(&lower_prefix) else {
            break;
        };
        result.push_str(&rest[..idx + prefix.len()]);
        let after = &rest[idx + prefix.len()..];
        let seg_len = after
            .find(['\\', '/', '"', '\'', ' ', '\n', '\r', '\t'])
            .unwrap_or(after.len());
        if seg_len > 0 {
            result.push_str("<USER>");
            rest = &after[seg_len..];
        } else {
            rest = after;
        }
    }

    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_sensitive_tokens_and_keys() {
        let raw = r#"Bearer eyJhbGciOiJIUzI1NiJ9.abc --accessToken secret_mc_tok {"refresh_token":"rt_12345","remoteApiKey":"sk-abcdef123456"} x-api-key: cf_secret_key"#;
        let redacted = redact_sensitive(raw);
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(!redacted.contains("secret_mc_tok"));
        assert!(!redacted.contains("rt_12345"));
        assert!(!redacted.contains("sk-abcdef123456"));
        assert!(!redacted.contains("cf_secret_key"));
        assert!(redacted.contains("Bearer ***"));
        assert!(redacted.contains("--accessToken ***"));
        assert!(redacted.contains("\"refresh_token\":\"***\""));
    }

    #[test]
    fn test_redact_does_not_corrupt_normal_hyphenated_words() {
        let normal = "Downloading disk-manager-1.2.0.jar and task-runner-core";
        assert_eq!(redact_sensitive(normal), normal);
    }

    #[test]
    fn test_redact_cli_args() {
        let args = vec![
            "-Xmx4096M".to_string(),
            "-Dhttp.proxyPassword=mySecretPass".to_string(),
            "--accessToken".to_string(),
            "jwt.payload.sig".to_string(),
            "--username".to_string(),
            "Steve".to_string(),
        ];
        let redacted = redact_cli_args(&args);
        assert_eq!(
            redacted,
            vec![
                "-Xmx4096M",
                "-Dhttp.proxyPassword=***",
                "--accessToken",
                "***",
                "--username",
                "Steve",
            ]
        );
    }

    #[test]
    fn test_redact_user_paths() {
        let msg = r#"File not found: C:\Users\Alice\AppData\test and /Users/Alice/mods"#;
        let masked = redact_user_paths(msg);
        assert!(!masked.contains("Alice"));
        assert!(masked.contains(r"C:\Users\<USER>\AppData\test"));
        assert!(masked.contains("/Users/<USER>/mods"));
    }

    #[test]
    fn test_truncate_for_log_utf8_safe() {
        let s = format!("你好世界{}", "a".repeat(100));
        let truncated = truncate_for_log(&s, 10);
        assert!(truncated.ends_with("... [truncated, total 112 bytes]"));
    }
}
