import { invoke } from "@tauri-apps/api/core";

export function trackEvent(name: string, props?: Record<string, any>): Promise<void> {
  // Call the Rust command directly which will log it in the terminal and send to Aptabase
  return invoke<void>("app_track_event", { name, props }).catch((err) => {
    // Only log errors in frontend if IPC completely fails
    if (import.meta.env.DEV) {
      console.error(`[Aptabase] IPC Failed to track event ${name}:`, err);
    }
  });
}
