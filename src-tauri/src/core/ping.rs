use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub online_players: u32,
    pub max_players: u32,
    pub ping: u32,
}

#[tauri::command]
pub async fn ping_server(ip: String, port: u16) -> Result<PingResult, String> {
    let mut ip_str = ip.as_str();
    if ip_str.is_empty() {
        ip_str = "127.0.0.1";
    }

    let start_time = std::time::Instant::now();

    // Connect with a 3-second timeout
    let stream_future = tokio::net::TcpStream::connect((ip_str, port));
    let mut stream =
        match tokio::time::timeout(std::time::Duration::from_secs(3), stream_future).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return Err(format!("Connection failed: {}", e)),
            Err(_) => return Err("Connection timed out".to_string()),
        };

    // Ping with a 3-second timeout
    let ping_future = craftping::tokio::ping(&mut stream, ip_str, port);
    match tokio::time::timeout(std::time::Duration::from_secs(3), ping_future).await {
        Ok(Ok(res)) => {
            let elapsed = start_time.elapsed().as_millis() as u32;
            Ok(PingResult {
                online_players: res.online_players as u32,
                max_players: res.max_players as u32,
                ping: elapsed,
            })
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("Ping timed out".to_string()),
    }
}
