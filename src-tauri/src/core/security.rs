use hmac::{Hmac, Mac, KeyInit};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

type HmacSha256 = Hmac<Sha256>;

const API_SECRET: &str = match option_env!("LAUNCHER_API_SECRET") {
    Some(v) => v,
    None => "Default fallback secret, set LAUNCHER_API_SECRET at compile time",
};

#[derive(Serialize, Deserialize)]
pub struct ApiSignature {
    pub timestamp: String,
    pub signature: String,
}

#[tauri::command]
pub fn generate_api_signature(method: String, path: String, body: String) -> Result<ApiSignature, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs()
        .to_string();

    // method + path + timestamp + body
    let payload = format!("{}{}{}{}", method, path, now, body);

    let mut mac = HmacSha256::new_from_slice(API_SECRET.as_bytes())
        .map_err(|e| format!("HMAC error: {}", e))?;
    
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let signature = hex::encode(result.into_bytes());

    Ok(ApiSignature {
        timestamp: now,
        signature,
    })
}

pub fn sign_request(method: &str, path: &str, body: &str) -> Result<reqwest::header::HeaderMap, String> {
    let sig = generate_api_signature(method.to_string(), path.to_string(), body.to_string())?;
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(ts) = reqwest::header::HeaderValue::from_str(&sig.timestamp) {
        headers.insert("X-Launcher-Time", ts);
    }
    if let Ok(sg) = reqwest::header::HeaderValue::from_str(&sig.signature) {
        headers.insert("X-Launcher-Signature", sg);
    }
    Ok(headers)
}

pub fn secure_request(client: &reqwest::Client, method: reqwest::Method, url: &str, body: &str) -> reqwest::RequestBuilder {
    let mut req = client.request(method.clone(), url);
    
    let path = if let Ok(u) = reqwest::Url::parse(url) {
        let mut p = u.path().to_string();
        if let Some(q) = u.query() {
            p.push('?');
            p.push_str(q);
        }
        p
    } else {
        url.to_string()
    };

    if let Ok(headers) = sign_request(method.as_str(), &path, body) {
        req = req.headers(headers);
    }
    req
}
