export function logToastError(context: string, message: string, details?: unknown): string {
  if (details !== undefined) {
    console.error(`[ToastError][${context}]`, message, details);
  } else {
    console.error(`[ToastError][${context}]`, message);
  }
  return message;
}

export function logToastWarning(context: string, message: string, details?: unknown): string {
  if (details !== undefined) {
    console.warn(`[ToastWarn][${context}]`, message, details);
  } else {
    console.warn(`[ToastWarn][${context}]`, message);
  }
  return message;
}
