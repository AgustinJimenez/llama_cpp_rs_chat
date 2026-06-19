/// Provider API key storage.
///
/// Keys are stored as plain JSON in the SQLite `config.provider_api_keys` column.
/// Earlier versions kept them in the OS keychain (Windows Credential Manager / macOS
/// Keychain / Linux Secret Service) and left a `KEYCHAIN_MARKER` in SQLite. That caused
/// a credential-store authorization prompt on every config read (e.g. opening the Agents
/// modal), which on unsigned dev builds reappeared constantly. We no longer use the
/// keychain; this module only handles the **one-time migration** of any keys still in the
/// keychain back into SQLite, after which the keychain is never touched again.
use keyring::Entry;
use std::sync::OnceLock;

const SERVICE: &str = "com.llamachat.desktop";
const ACCOUNT: &str = "provider_api_keys";
/// Legacy marker previously stored in SQLite when keys lived in the keychain.
pub const KEYCHAIN_MARKER: &str = "__keychain__";

fn entry() -> Result<Entry, keyring::Error> {
    Entry::new(SERVICE, ACCOUNT)
}

/// Recover keys from the legacy keychain entry exactly once per process (then delete it).
/// `OnceLock` guarantees a single keychain access even if several requests race on the
/// marker before the migration is persisted to SQLite — so at most one OS prompt, ever.
fn recover_once() -> Option<String> {
    static MIGRATED: OnceLock<Option<String>> = OnceLock::new();
    MIGRATED
        .get_or_init(|| {
            let recovered = entry()
                .and_then(|e| e.get_password())
                .ok()
                .filter(|v| !v.is_empty());
            delete_legacy();
            recovered
        })
        .clone()
}

/// Resolve provider API keys JSON from the SQLite column value.
///
/// Returns `(keys_json, migrated)`. When `migrated` is true the caller should rewrite the
/// SQLite column with the returned RAW JSON (we've pulled it out of the legacy keychain);
/// after that the column holds plain JSON and the keychain is never read again.
pub fn resolve(db_value: &str) -> (Option<String>, bool) {
    if db_value == KEYCHAIN_MARKER {
        return (recover_once(), true);
    }
    if db_value.trim_start().starts_with('{') && db_value != "{}" {
        return (Some(db_value.to_string()), false);
    }
    (None, false)
}

/// Best-effort removal of the legacy keychain entry (no-op if it doesn't exist).
pub fn delete_legacy() {
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
}
