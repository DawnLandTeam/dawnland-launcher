import { invoke } from "@tauri-apps/api/core";

export function sanitizeTrackingUrl(raw?: string): string | undefined {
  if (!raw) return undefined;
  try {
    const u = new URL(raw.trim());
    u.search = "";
    u.hash = "";
    return u.toString();
  } catch {
    return undefined;
  }
}

export function getErrorType(err: unknown): string {
  if (err instanceof Error) return err.name;
  if (typeof err === "string") return "StringError"; 
  if (err && typeof err === "object" && "type" in err) return String((err as any).type);
  return typeof err;
}

export type AnalyticsEvents = {
  "app_started": undefined;
  "settings_viewed": undefined;
  "account_added": { type: "offline" | "authlib" | "microsoft", api?: string, flow?: string };
  "login_failed": { type: "authlib" | "microsoft", error_type: string, flow?: string, api?: string, [key: string]: any };
  "authlib_added": { type: "deeplink_authlib" | "deeplink_authlib_drop" | "manual_authlib", api?: string };
  "modpack_install_started": { type: "online" | "local" | "deeplink_online", isUpdate?: boolean, source?: string };
  "modpack_install_completed": { instanceName: string };
  "game_launch_started": { instanceId: string, auto?: boolean };
  "game_launched": { instanceId: string, auto?: boolean };
  "java_download_completed": { majorVersion: number, version: string };
  "error_occurred": { context: string, error_type: string, [key: string]: any };
};

export function trackEvent<K extends keyof AnalyticsEvents>(
  name: K,
  ...args: AnalyticsEvents[K] extends undefined ? [props?: undefined] : [props: AnalyticsEvents[K]]
): Promise<void> {
  const props = args[0] as Record<string, any> | undefined;
  // Call the Rust command directly which will log it in the terminal and send to Aptabase
  return invoke<void>("app_track_event", { name, props }).catch((err) => {
    // Only log errors in frontend if IPC completely fails
    if (import.meta.env.DEV) {
      console.error(`[Aptabase] IPC Failed to track event ${name}:`, err);
    }
  });
}
