/**
 * Remote access auth token management.
 * Token is delivered via URL hash (#token=xxx) from the QR code,
 * then persisted to localStorage so it survives page reloads.
 */

const STORAGE_KEY = 'llama_remote_token';

/** Read token from URL hash on first load and persist it. */
export function initRemoteToken(): void {
  const { hash } = window.location;
  const match = hash.match(/[#&]token=([^&]+)/);
  if (match) {
    localStorage.setItem(STORAGE_KEY, match[1]);
    // Clean the token out of the URL bar
    window.history.replaceState(null, '', window.location.pathname + window.location.search);
  }
}

/** Get the stored remote token, or null if not set. */
export function getRemoteToken(): string | null {
  return localStorage.getItem(STORAGE_KEY);
}

/**
 * Return Authorization headers for fetch() calls.
 * Returns an empty object on localhost (no token needed).
 */
export function getAuthHeaders(): Record<string, string> {
  const token = getRemoteToken();
  if (!token) return {};
  return { Authorization: `Bearer ${token}` };
}

/**
 * Return a query string suffix for WebSocket URLs.
 * Browsers cannot set custom headers on WS upgrades, so the token is
 * passed as ?token=<value> instead.
 * Returns '' when no token is stored (localhost use).
 */
export function getWsAuthParam(): string {
  const token = getRemoteToken();
  if (!token) return '';
  return `?token=${encodeURIComponent(token)}`;
}
