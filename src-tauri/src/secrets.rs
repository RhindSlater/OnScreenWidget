//! Secure secret storage backed by the OS keychain via the `keyring` crate.
//!
//! On Windows this uses the Credential Manager; on macOS the Keychain. Secrets
//! are referenced from the config by a stable `secret_ref` string so the plain
//! config JSON never contains credentials.

use keyring::Entry;

const SERVICE: &str = "com.onscreenwidget.app";

fn entry(secret_ref: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, secret_ref).map_err(|e| format!("keychain entry: {e}"))
}

/// Store (or overwrite) a secret under `secret_ref`.
pub fn set_secret(secret_ref: &str, value: &str) -> Result<(), String> {
    entry(secret_ref)?
        .set_password(value)
        .map_err(|e| format!("set secret: {e}"))
}

/// Retrieve a secret. Returns `Ok(None)` if it does not exist.
pub fn get_secret(secret_ref: &str) -> Result<Option<String>, String> {
    match entry(secret_ref)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("get secret: {e}")),
    }
}

/// Delete a secret. Succeeds even if it did not exist.
pub fn delete_secret(secret_ref: &str) -> Result<(), String> {
    match entry(secret_ref)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("delete secret: {e}")),
    }
}
