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
  "App Started": undefined;
  "Settings Viewed": undefined;
  "Accounts Viewed": undefined;
  "Instances Viewed": undefined;
  "Servers Viewed": undefined;
  "Downloads Viewed": undefined;
  "Account Added": { type: "offline" | "authlib" | "microsoft", api?: string, flow?: string };
  "Login Failed": { type: "authlib" | "microsoft", error_type: string, flow?: string, api?: string, [key: string]: any };
  "Authlib Added": { type: "deeplink_authlib" | "deeplink_authlib_drop" | "manual_authlib", api?: string };
  "Modpack Install Completed": { name: string, projectId?: string };
  "Instance Install Completed": { version: string, loader?: string };
  "Mod Install Completed": { name: string, projectId?: string, versionId?: string };
  "Resourcepack Install Completed": { name: string, projectId?: string, versionId?: string };
  "Shaderpack Install Completed": { name: string, projectId?: string, versionId?: string };
  "World Install Completed": { name: string, projectId?: string, versionId?: string };
  "Datapack Install Completed": { name: string, projectId?: string, versionId?: string };
  "Game Launched": { accountType: string, auto?: boolean, instanceName?: string };
  "Java Download Completed": { majorVersion: number, version: string };
  "Error Occurred": { context: string, error_type: string, [key: string]: any };
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
