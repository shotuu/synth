//! Thin wrapper around the OS keychain (macOS Keychain, Windows Credential Manager,
//! Linux Secret Service) for storing cloud LLM/transcription provider API keys.
//!
//! These used to be written to plain TEXT columns in the app's SQLite database, which
//! meant any local process or a stolen copy of the app's data directory could read every
//! configured provider key in cleartext. Secrets now live only in the OS keychain; the
//! DB is used only to migrate any pre-existing plaintext keys once (see setting.rs).

use keyring::Entry;

const SERVICE_NAME: &str = "com.synth.app";

fn entry(namespace: &str, provider: &str) -> Result<Entry, String> {
    Entry::new(SERVICE_NAME, &format!("{}:{}", namespace, provider))
        .map_err(|e| format!("Failed to access system keychain: {}", e))
}

/// Save a secret under `namespace:provider` (e.g. "summary:openai", "transcript:groq" —
/// namespaced because the same provider name can need a different key for summary
/// generation vs. transcription).
pub fn save_secret(namespace: &str, provider: &str, secret: &str) -> Result<(), String> {
    entry(namespace, provider)?
        .set_password(secret)
        .map_err(|e| format!("Failed to save secret to system keychain: {}", e))
}

/// Read a secret, returning `Ok(None)` (not an error) if nothing has been saved yet.
pub fn get_secret(namespace: &str, provider: &str) -> Result<Option<String>, String> {
    match entry(namespace, provider)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to read secret from system keychain: {}", e)),
    }
}

/// Remove a secret. Treats "already absent" as success, not an error.
pub fn delete_secret(namespace: &str, provider: &str) -> Result<(), String> {
    match entry(namespace, provider)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to delete secret from system keychain: {}", e)),
    }
}
