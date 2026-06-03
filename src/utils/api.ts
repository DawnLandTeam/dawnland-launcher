import { invoke } from "@tauri-apps/api/core";

export interface ApiSignature {
  timestamp: string;
  signature: string;
}

export async function fetchApi(url: string, options: RequestInit = {}): Promise<Response> {
  const method = options.method || "GET";
  
  // Extract path from url
  let path = "";
  try {
    const urlObj = new URL(url);
    path = urlObj.pathname + urlObj.search;
  } catch (e) {
    path = url; // fallback
  }

  // Extract body
  let bodyStr = "";
  if (options.body) {
    if (typeof options.body === "string") {
      bodyStr = options.body;
    } else {
      // For simple cases, we assume body is stringified JSON.
      // If it's FormData, it's harder to sign. We will leave it empty.
      try {
        bodyStr = JSON.stringify(options.body);
      } catch (e) {
        bodyStr = "";
      }
    }
  }

  try {
    // Get signature from Rust backend
    const sig: ApiSignature = await invoke("generate_api_signature", {
      method,
      path,
      body: bodyStr,
    });

    // Attach headers
    const headers = new Headers(options.headers || {});
    headers.set("X-Launcher-Time", sig.timestamp);
    headers.set("X-Launcher-Signature", sig.signature);

    return fetch(url, {
      ...options,
      headers,
    });
  } catch (err) {
    console.error("Failed to generate API signature or fetch:", err);
    throw err;
  }
}
