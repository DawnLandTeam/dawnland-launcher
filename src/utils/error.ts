export function getErrorMessage(err: unknown): string {
  if (!err) return "Unknown error";
  
  // Handle new AppError structure { code: string, message: string }
  if (typeof err === "object" && "message" in err && err.message) {
    return String(err.message);
  }
  
  if (typeof err === "string") {
    return err;
  }
  
  return String(err);
}
