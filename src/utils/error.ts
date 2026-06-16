export function getErrorMessage(err: unknown): string {
  if (!err) return "Unknown error";

  if (typeof err === "object") {
    const anyErr = err as { message?: unknown; data?: unknown };
    if (typeof anyErr.message === "string" && anyErr.message) return anyErr.message;
    if (typeof anyErr.data === "string" && anyErr.data) return anyErr.data;
  }

  if (typeof err === "string") return err;
  return String(err);
}
