//! Secure API key storage using OS keyring.
//!
//! This module provides cross-platform credential storage:
//! - macOS: Keychain (`apple-native`)
//! - Windows: Credential Manager (`windows-native`)
//! - Linux: keyutils + Secret Service (`linux-native-sync-persistent`)
//!
//! keyring 3.x has no default platform backends. Without those features it
//! silently uses an in-memory mock store — see issue #33.

use anyhow::{bail, Context, Result};

const SERVICE_NAME: &str = "linear-cli";
const OAUTH_SERVICE_NAME: &str = "linear-cli-oauth";

fn entry(service: &str, profile: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(service, profile).context("Failed to create keyring entry")
}

fn is_mock_entry(entry: &keyring::Entry) -> bool {
    entry
        .get_credential()
        .downcast_ref::<keyring::mock::MockCredential>()
        .is_some()
}

/// Human-readable name of the expected OS backend on this target.
pub fn backend_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else if cfg!(target_os = "linux") {
        "Linux keyutils + Secret Service"
    } else {
        "platform credential store"
    }
}

/// Reject keyring's in-memory mock backend (non-persistent).
fn require_persistent_backend() -> Result<()> {
    let probe = entry(SERVICE_NAME, "__linear_cli_backend_probe__")?;
    if is_mock_entry(&probe) {
        bail!(
            "secure-storage is using keyring's in-memory mock backend instead of {backend}. \
             Credentials will not persist. Rebuild with `--features secure-storage` after \
             enabling keyring platform features (apple-native, windows-native, \
             linux-native-sync-persistent).",
            backend = backend_name()
        );
    }
    Ok(())
}

fn get_secret(service: &str, profile: &str, fallback_label: &str) -> Result<Option<String>> {
    let entry = entry(service, profile)?;
    if is_mock_entry(&entry) {
        // Treat mock as missing so auth falls through; writes must call require_persistent_backend.
        return Ok(None);
    }

    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(keyring::Error::NoStorageAccess(_)) => {
            if !crate::output::is_quiet() {
                eprintln!("Warning: Keyring not available, falling back to {fallback_label}");
            }
            Ok(None)
        }
        Err(e) => {
            if !crate::output::is_quiet() {
                eprintln!(
                    "Warning: Keyring error ({e}), falling back to {fallback_label}"
                );
            }
            Ok(None)
        }
    }
}

fn set_secret(service: &str, profile: &str, secret: &str) -> Result<()> {
    require_persistent_backend()?;
    entry(service, profile)?
        .set_password(secret)
        .with_context(|| format!("Failed to store secret in {}", backend_name()))?;
    Ok(())
}

fn delete_secret(service: &str, profile: &str) -> Result<()> {
    let entry = entry(service, profile)?;
    if is_mock_entry(&entry) {
        return Ok(());
    }
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).with_context(|| format!("Failed to delete secret from {}", backend_name())),
    }
}

/// Get an API key from the keyring for a profile.
/// Returns Ok(None) if no key is stored, Ok(Some(key)) if found.
pub fn get_key(profile: &str) -> Result<Option<String>> {
    get_secret(SERVICE_NAME, profile, "config file")
}

/// Store an API key in the keyring for a profile.
pub fn set_key(profile: &str, api_key: &str) -> Result<()> {
    set_secret(SERVICE_NAME, profile, api_key)
}

/// Delete an API key from the keyring for a profile.
/// Returns Ok(()) even if no key was stored.
pub fn delete_key(profile: &str) -> Result<()> {
    delete_secret(SERVICE_NAME, profile)
}

/// Check if a durable OS keyring backend is available.
pub fn is_available() -> bool {
    match entry(SERVICE_NAME, "__test__") {
        Ok(entry) => {
            if is_mock_entry(&entry) {
                return false;
            }
            !matches!(entry.get_password(), Err(keyring::Error::NoStorageAccess(_)))
        }
        Err(_) => false,
    }
}

/// Get OAuth tokens JSON from keyring for a profile
pub fn get_oauth_tokens(profile: &str) -> Result<Option<String>> {
    get_secret(OAUTH_SERVICE_NAME, profile, "config")
}

/// Store OAuth tokens JSON in keyring for a profile
pub fn set_oauth_tokens(profile: &str, json: &str) -> Result<()> {
    set_secret(OAUTH_SERVICE_NAME, profile, json)
}

/// Delete OAuth tokens from keyring for a profile
pub fn delete_oauth_tokens(profile: &str) -> Result<()> {
    delete_secret(OAUTH_SERVICE_NAME, profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PROFILE: &str = "linear-cli-test-profile";
    const TEST_KEY: &str = "lin_api_test_key_12345";

    #[test]
    fn default_backend_is_not_mock() {
        let probe = entry(SERVICE_NAME, "__backend_test__").expect("create probe entry");
        assert!(
            !is_mock_entry(&probe),
            "secure-storage must not use keyring's mock backend; enable apple-native, windows-native, and linux-native-sync-persistent on the keyring dependency"
        );
    }

    #[test]
    fn test_is_available() {
        // Availability depends on OS services; mock must never count as available.
        if is_mock_entry(&entry(SERVICE_NAME, "__test__").expect("probe")) {
            assert!(!is_available());
        }
        let _ = is_available();
    }

    #[test]
    fn test_set_get_delete_key() {
        if !is_available() {
            eprintln!("Skipping keyring test - keyring not available");
            return;
        }

        // Clean up any leftover from previous test runs
        let _ = delete_key(TEST_PROFILE);

        // Set a key - check for errors
        if let Err(e) = set_key(TEST_PROFILE, TEST_KEY) {
            eprintln!("Skipping test - set_key failed: {}", e);
            return;
        }

        // Get the key back
        match get_key(TEST_PROFILE) {
            Ok(Some(key)) => {
                assert_eq!(key, TEST_KEY, "Key should match");
            }
            Ok(None) => {
                // Some systems (like CI) may have keyring available but not persistent
                eprintln!("Warning: Key not found after set - keyring may not be persistent in this environment");
            }
            Err(e) => {
                eprintln!("Warning: get_key failed: {}", e);
            }
        }

        // Clean up
        let _ = delete_key(TEST_PROFILE);
    }

    #[test]
    fn test_delete_nonexistent_key() {
        if !is_available() {
            eprintln!("Skipping keyring test - keyring not available");
            return;
        }

        // Deleting a key that doesn't exist should not error
        let result = delete_key("nonexistent-profile-xyz");
        assert!(result.is_ok(), "Deleting nonexistent key should succeed");
    }

    #[test]
    fn test_overwrite_key() {
        if !is_available() {
            eprintln!("Skipping keyring test - keyring not available");
            return;
        }

        let profile = "linear-cli-test-overwrite";
        let _ = delete_key(profile); // Clean up

        // Set initial key - check for errors
        if let Err(e) = set_key(profile, "key1") {
            eprintln!("Skipping test - set_key failed: {}", e);
            return;
        }

        // Verify or skip if not persistent
        match get_key(profile) {
            Ok(Some(key)) if key == "key1" => {
                // Overwrite with new key
                set_key(profile, "key2").expect("Failed to set key2");
                if let Ok(Some(key2)) = get_key(profile) {
                    assert_eq!(key2, "key2", "Overwritten key should match");
                }
            }
            _ => {
                eprintln!("Warning: Keyring not persistent in this environment");
            }
        }

        // Clean up
        let _ = delete_key(profile);
    }
}
