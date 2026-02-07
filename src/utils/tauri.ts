// Check if we're running in Tauri or web environment
export const isTauriEnv = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Send a desktop notification when the app is not focused (Tauri only, no-op in web). */
export async function notifyIfUnfocused(title: string, body: string): Promise<void> {
  if (!isTauriEnv() || document.hasFocus()) return;
  try {
    const { isPermissionGranted, requestPermission, sendNotification } =
      await import('@tauri-apps/plugin-notification');
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === 'granted';
    }
    if (granted) {
      sendNotification({ title, body });
    }
  } catch {
    // Notification plugin not available — silently ignore
  }
}
