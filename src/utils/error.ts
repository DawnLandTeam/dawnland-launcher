import i18n from '../i18n';

export function getErrorMessage(err: unknown): string {
  if (!err) return "Unknown error";

  let msg = "";
  if (typeof err === "object") {
    const anyErr = err as { message?: unknown; data?: unknown };
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

  return msg || "Unknown error";
}
