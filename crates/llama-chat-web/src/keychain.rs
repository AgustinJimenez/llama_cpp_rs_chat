/// Secure API key storage backed by the OS credential store.
///
/// On Windows: Windows Credential Manager
/// On macOS: Keychain
/// On Linux: Secret Service (libsecret / gnome-keyring)
///
/// The SQLite `provider_api_keys` column stores `KEYCHAIN_MARKER` once keys
/// have been migrated. Reads check for this marker and fetch from the OS store.
/// Falls back to the raw SQLite value if keychain is unavailable so existing
/// installs keep working without interruption.
use keyring::Entry;

const SERVICE: &str = "com.llamachat.desktop";
const ACCOUNT: &str = "provider_api_keys";
/// Value stored in SQLite after keys are moved to the OS keychain.
pub const KEYCHAIN_MARKER: &str = "__keychain__";

fn entry() -> Result<Entry, keyring::Error> {
    Entry::new(SERVICE, ACCOUNT)
}

/// Read the API keys JSON from the OS keychain.
/// Returns `None` if no entry exists or the keychain is unavailable.
pub fn get() -> Option<String> {
    match entry().and_then(|e| e.get_password()) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// Write the API keys JSON to the OS keychain.
/// Returns `true` on success, `false` if the keychain is unavailable.
pub fn set(keys_json: &str) -> bool {
    match entry().and_then(|e| e.set_password(keys_json)) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("[keychain] Failed to write to OS keychain: {e}");
            false
        }
    }
}

/// Delete the keychain entry (called when all keys are removed).
pub fn delete() {
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
}

/// Resolve API keys JSON from either the OS keychain or the raw SQLite value.
///
/// Migration path:
/// - If `db_value` == `KEYCHAIN_MARKER` → read from keychain
/// - If `db_value` is valid non-empty JSON → migrate it to keychain, return it
///   (caller should persist `KEYCHAIN_MARKER` back to SQLite)
/// - Otherwise → return `None`
///
/// Returns `(keys_json, should_write_marker)`.
pub fn resolve(db_value: &str) -> (Option<String>, bool) {
    if db_value == KEYCHAIN_MARKER {
        return (get(), false);
    }
    // Non-empty JSON in DB: migrate transparently to keychain
    if db_value.trim_start().starts_with('{') && db_value != "{}" {
        if set(db_value) {
            return (Some(db_value.to_string()), true); // caller writes marker to DB
        }
        // Keychain unavailable — leave in DB as-is
        return (Some(db_value.to_string()), false);
    }
    // Empty / null
    (None, false)
}
