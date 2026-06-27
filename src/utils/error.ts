import i18n from '../i18n';

export function getErrorMessage(err: unknown): string {
  if (!err) return "Unknown error";

  let msg = "";
  if (typeof err === "object") {
    const anyErr = err as { code?: string; message?: unknown; data?: unknown };
    if (anyErr.code === "MD5_MISMATCH") {
      return (i18n.global.t as any)('errors.md5Mismatch');
    }
    if (typeof anyErr.message === "string" && anyErr.message) {
      msg = anyErr.message;
    } else if (typeof anyErr.data === "string" && anyErr.data) {
      msg = anyErr.data;
    }
  } else if (typeof err === "string") {
    msg = err;
  } else {
    msg = String(err);
  }

  if (msg.startsWith("Database error: CONFLICTING_TASK:")) {
    const taskName = msg.replace("Database error: CONFLICTING_TASK:", "").trim();
    return (i18n.global.t as any)('errors.conflictingTask', { taskName });
  }
  
  if (msg.includes("Failed to write launcher settings") && msg.includes("os error 5")) {
    return (i18n.global.t as any)('errors.settingsAccessDenied');
  }
  if (msg.startsWith("Instance with name '") && msg.includes("' already exists")) {
    const instanceName = msg.split("'")[1];
    return (i18n.global.t as any)('errors.instanceExists', { instanceName });
  }


  return msg || "Unknown error";
}
