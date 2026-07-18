use anyhow::{Context, Result};
use dialoguer::Password;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg(unix)]
use std::io::Write;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub token_type: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workspace {
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthConfig>,
    /// Default team key/name for commands that accept `--team`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_team: Option<String>,
}

impl Workspace {
    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            oauth: None,
            default_team: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub current: Option<String>,
    #[serde(default)]
    pub workspaces: HashMap<String, Workspace>,
    // Legacy field for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

fn config_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join("linear-cli");

    fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("config.toml"))
}

pub fn load_config() -> Result<Config> {
    let path = config_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let mut config: Config = toml::from_str(&content)?;

        // Migrate legacy api_key to workspaces if needed
        if let Some(legacy_key) = config.api_key.take() {
            if !config.workspaces.contains_key("default") {
                config
                    .workspaces
                    .insert("default".to_string(), Workspace::with_api_key(legacy_key));
                if config.current.is_none() {
                    config.current = Some("default".to_string());
                }
                save_config(&config)?;
            }
        }

        Ok(config)
    } else {
        Ok(Config::default())
    }
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = config_path()?;
    let content = toml::to_string_pretty(config)?;

    // Write to temp file then rename for atomicity
    let dir = path
        .parent()
        .context("Config path has no parent directory")?;
    let temp_path = dir.join(".config.toml.tmp");

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp_path)?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
    }

    #[cfg(not(unix))]
    {
        fs::write(&temp_path, &content)?;
    }

    fs::rename(&temp_path, &path).context("Failed to atomically update config file")?;
    Ok(())
}

/// Resolve profile name for read/write without the process-lifetime cache used by
/// [`current_profile`]. Prefer this when the config may still be mid-setup.
fn resolve_profile_name(config: &Config) -> String {
    std::env::var("LINEAR_CLI_PROFILE")
        .ok()
        .filter(|p| !p.is_empty())
        .or_else(|| config.current.clone())
        .unwrap_or_else(|| "default".to_string())
}

pub fn set_api_key(key: &str) -> Result<()> {
    let mut config = load_config()?;
    let workspace_name = resolve_profile_name(&config);
    let workspace = config
        .workspaces
        .entry(workspace_name.clone())
        .or_insert_with(|| Workspace::with_api_key(""));
    workspace.api_key = key.to_string();
    if config.current.is_none() {
        config.current = Some(workspace_name);
    }
    save_config(&config)?;
    Ok(())
}

/// Stored default team for the active workspace (config file only; no env).
pub fn get_stored_default_team() -> Option<String> {
    let config = load_config().ok()?;
    let profile = resolve_profile_name(&config);
    config
        .workspaces
        .get(&profile)
        .and_then(|w| w.default_team.clone())
        .filter(|t| !t.trim().is_empty())
}

/// Effective default team: `LINEAR_CLI_TEAM` env, then stored workspace default.
pub fn get_default_team() -> Option<String> {
    if let Ok(team) = std::env::var("LINEAR_CLI_TEAM") {
        let team = team.trim().to_string();
        if !team.is_empty() {
            return Some(team);
        }
    }
    get_stored_default_team()
}

/// CLI `--team` / positional override, then effective default team.
pub fn resolve_team_arg(cli: Option<String>) -> Option<String> {
    cli.map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .or_else(get_default_team)
}

/// Like [`resolve_team_arg`], but errors when nothing is configured.
pub fn require_team(cli: Option<String>) -> Result<String> {
    resolve_team_arg(cli).ok_or_else(|| {
        anyhow::anyhow!(
            "--team is required (or set a default: linear config set default-team TEAM)"
        )
    })
}

/// Set the default team for the current workspace. Requires an existing workspace.
pub fn set_default_team(team: Option<&str>) -> Result<()> {
    let mut config = load_config()?;
    let profile = resolve_profile_name(&config);
    let workspace = config.workspaces.get_mut(&profile).with_context(|| {
        format!(
            "Workspace '{}' not found. Run setup or: linear config set-key first.",
            profile
        )
    })?;
    workspace.default_team = team
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());
    if config.current.is_none() {
        config.current = Some(profile);
    }
    save_config(&config)?;
    Ok(())
}

/// Known configuration keys for get/set (for help and error messages).
pub fn known_config_keys() -> &'static [&'static str] {
    &["api-key", "profile", "default-team"]
}

fn read_secret_from_stdin_or_prompt(prompt: &str) -> Result<String> {
    if std::io::stdin().is_terminal() {
        let secret = Password::new()
            .with_prompt(prompt)
            .allow_empty_password(false)
            .interact()
            .context("Failed to read secret from terminal")?;
        return Ok(secret);
    }

    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .context("Failed to read secret from stdin")?;
    let secret = buffer.trim().to_string();

    if secret.is_empty() {
        anyhow::bail!("No secret provided on stdin");
    }

    Ok(secret)
}

pub fn set_api_key_from_stdin_or_prompt() -> Result<()> {
    let key = read_secret_from_stdin_or_prompt("Linear API key")?;
    set_api_key(&key)
}

pub fn get_api_key() -> Result<String> {
    // Check for LINEAR_API_KEY environment variable first
    if let Ok(api_key) = std::env::var("LINEAR_API_KEY") {
        if !api_key.is_empty() {
            return Ok(api_key);
        }
    }

    // Try keyring if feature is enabled
    #[cfg(feature = "secure-storage")]
    {
        let config = load_config()?;
        let profile = std::env::var("LINEAR_CLI_PROFILE")
            .ok()
            .filter(|p| !p.is_empty())
            .or(config.current.clone())
            .unwrap_or_else(|| "default".to_string());

        if let Ok(Some(key)) = crate::keyring::get_key(&profile) {
            return Ok(key);
        }
    }

    // Fall back to config file
    let config = load_config()?;
    let profile = std::env::var("LINEAR_CLI_PROFILE")
        .ok()
        .filter(|p| !p.is_empty());
    let current = profile.or(config.current.clone()).context(
        "No workspace selected. Run: linear config workspace-add <name> or set LINEAR_CLI_PROFILE",
    )?;
    let workspace = config.workspaces.get(&current).context(format!(
        "Workspace '{}' not found. Run: linear config workspace-add <name>",
        current
    ))?;
    Ok(workspace.api_key.clone())
}

pub fn config_file_path() -> Result<PathBuf> {
    config_path()
}

/// Returns the current workspace profile name.
///
/// NOTE: The result is cached via OnceLock for the lifetime of the process.
/// If the profile is changed in-process (e.g., via `workspace_switch`), the
/// cached value will be stale. This is acceptable because profile switches
/// during a single CLI invocation are not a supported use case.
pub fn current_profile() -> Result<String> {
    static PROFILE: OnceLock<String> = OnceLock::new();

    if let Some(cached) = PROFILE.get() {
        return Ok(cached.clone());
    }

    let config = load_config()?;
    let profile = std::env::var("LINEAR_CLI_PROFILE")
        .ok()
        .filter(|p| !p.is_empty());
    let resolved = profile
        .or(config.current)
        .context("No workspace selected")?;

    // Store for future calls (ignore if another thread beat us)
    let _ = PROFILE.set(resolved.clone());
    Ok(resolved)
}

pub fn set_workspace_key(name: &str, api_key: &str) -> Result<()> {
    let mut config = load_config()?;
    let workspace = config
        .workspaces
        .entry(name.to_string())
        .or_insert_with(|| Workspace::with_api_key(""));
    workspace.api_key = api_key.to_string();
    if config.current.is_none() {
        config.current = Some(name.to_string());
    }
    save_config(&config)?;
    Ok(())
}

pub fn config_get(key: &str, raw: bool) -> Result<()> {
    match key.to_lowercase().as_str() {
        "api-key" | "api_key" => {
            let api_key = get_api_key()?;
            if raw {
                println!("{}", api_key);
            } else {
                println!("{}", mask_api_key_for_display(&api_key));
            }
        }
        "profile" => {
            let profile = current_profile()?;
            println!("{}", profile);
        }
        // Stored value only — env override is separate (see show / get_default_team).
        "default-team" | "default_team" | "team" => match get_stored_default_team() {
            Some(team) => println!("{}", team),
            None => {
                if !raw {
                    println!("(not set)");
                }
            }
        },
        _ => anyhow::bail!(
            "Unknown config key: {}. Known keys: {}",
            key,
            known_config_keys().join(", ")
        ),
    }
    Ok(())
}

pub fn config_set(key: &str, value: &str) -> Result<()> {
    match key.to_lowercase().as_str() {
        "api-key" | "api_key" => anyhow::bail!(
            "Setting api-key via argv is disabled. Use `linear config set-key` and provide the key via stdin or the hidden prompt."
        ),
        "profile" => workspace_switch(value),
        "default-team" | "default_team" | "team" => {
            set_default_team(Some(value))?;
            println!("Default team set to '{}'", value.trim());
            Ok(())
        }
        _ => anyhow::bail!(
            "Unknown config key: {}. Known keys: {}",
            key,
            known_config_keys().join(", ")
        ),
    }
}

pub fn show_config() -> Result<()> {
    let config = load_config()?;
    let path = config_path()?;

    println!("Config file: {}", path.display());
    println!();
    println!("Known keys: {}", known_config_keys().join(", "));
    println!();

    if let Some(current) = &config.current {
        println!("Current workspace: {}", current);
        if let Some(workspace) = config.workspaces.get(current) {
            println!("API Key: {}", mask_api_key_for_display(&workspace.api_key));
            match &workspace.default_team {
                Some(team) if !team.is_empty() => println!("Default team: {}", team),
                _ => println!("Default team: (not set)"),
            }
        }
    } else {
        println!("No workspace configured. Run: linear workspace add <name>");
    }

    if let Ok(env_team) = std::env::var("LINEAR_CLI_TEAM") {
        if !env_team.trim().is_empty() {
            println!("LINEAR_CLI_TEAM (overrides config): {}", env_team.trim());
        }
    }

    Ok(())
}

// Workspace management functions

pub fn workspace_add(name: &str, api_key: &str) -> Result<()> {
    let mut config = load_config()?;

    if config.workspaces.contains_key(name) {
        anyhow::bail!(
            "Workspace '{}' already exists. Use 'workspace remove' first to replace it.",
            name
        );
    }

    config
        .workspaces
        .insert(name.to_string(), Workspace::with_api_key(api_key));

    // If this is the first workspace, make it current
    if config.current.is_none() {
        config.current = Some(name.to_string());
    }

    save_config(&config)?;
    println!("Workspace '{}' added successfully!", name);

    if config.current.as_ref() == Some(&name.to_string()) {
        println!("Switched to workspace '{}'", name);
    }

    Ok(())
}

pub fn workspace_add_from_stdin_or_prompt(name: &str) -> Result<()> {
    let api_key =
        read_secret_from_stdin_or_prompt(&format!("Linear API key for workspace '{name}'"))?;
    workspace_add(name, &api_key)
}

pub fn workspace_list() -> Result<()> {
    let config = load_config()?;

    if config.workspaces.is_empty() {
        println!("No workspaces configured. Run: linear workspace add <name>");
        return Ok(());
    }

    println!("Configured workspaces:");
    println!();

    for (name, workspace) in &config.workspaces {
        let is_current = config.current.as_ref() == Some(name);
        let marker = if is_current { "*" } else { " " };
        let masked = mask_api_key_for_display(&workspace.api_key);
        println!("{} {} ({})", marker, name, masked);
    }

    println!();
    println!("* = current workspace");

    Ok(())
}

pub fn workspace_switch(name: &str) -> Result<()> {
    let mut config = load_config()?;

    if !config.workspaces.contains_key(name) {
        anyhow::bail!(
            "Workspace '{}' not found. Use 'workspace list' to see available workspaces.",
            name
        );
    }

    config.current = Some(name.to_string());
    save_config(&config)?;
    println!("Switched to workspace '{}'", name);

    Ok(())
}

pub fn workspace_current() -> Result<()> {
    let config = load_config()?;

    if let Some(current) = &config.current {
        println!("Current workspace: {}", current);
        if let Some(workspace) = config.workspaces.get(current) {
            println!("API Key: {}", mask_api_key_for_display(&workspace.api_key));
        }
    } else {
        println!("No workspace selected. Run: linear workspace add <name>");
    }

    Ok(())
}

pub fn workspace_remove(name: &str) -> Result<()> {
    let mut config = load_config()?;

    if !config.workspaces.contains_key(name) {
        anyhow::bail!("Workspace '{}' not found.", name);
    }

    config.workspaces.remove(name);

    // If we removed the current workspace, clear it or switch to another
    if config.current.as_ref() == Some(&name.to_string()) {
        config.current = config.workspaces.keys().next().cloned();
        if let Some(new_current) = &config.current {
            println!("Switched to workspace '{}'", new_current);
        }
    }

    save_config(&config)?;
    println!("Workspace '{}' removed.", name);

    Ok(())
}

/// Save OAuth config for a profile
pub fn save_oauth_config(profile: &str, oauth_config: &OAuthConfig) -> Result<()> {
    let mut config = load_config()?;
    let workspace = config
        .workspaces
        .entry(profile.to_string())
        .or_insert_with(|| Workspace::with_api_key(""));
    workspace.oauth = Some(oauth_config.clone());
    if config.current.is_none() {
        config.current = Some(profile.to_string());
    }
    save_config(&config)?;
    Ok(())
}

fn oauth_config_has_secrets(oauth_config: &OAuthConfig) -> bool {
    !oauth_config.access_token.is_empty()
        || oauth_config
            .refresh_token
            .as_ref()
            .map(|token| !token.is_empty())
            .unwrap_or(false)
}

fn mask_api_key_for_display(api_key: &str) -> String {
    if api_key.len() > 12 {
        format!("{}***{}", &api_key[..4], &api_key[api_key.len() - 4..])
    } else if api_key.starts_with("lin_") {
        "lin_***".to_string()
    } else {
        "***".to_string()
    }
}

#[cfg(feature = "secure-storage")]
fn oauth_metadata_only(oauth_config: &OAuthConfig) -> OAuthConfig {
    OAuthConfig {
        client_id: oauth_config.client_id.clone(),
        access_token: String::new(),
        refresh_token: None,
        expires_at: oauth_config.expires_at,
        token_type: oauth_config.token_type.clone(),
        scopes: oauth_config.scopes.clone(),
    }
}

/// Get OAuth metadata for a profile from config, even if secrets are stored elsewhere.
pub fn get_oauth_metadata(profile: &str) -> Result<Option<OAuthConfig>> {
    let config = load_config()?;
    Ok(config.workspaces.get(profile).and_then(|w| w.oauth.clone()))
}

#[cfg(feature = "secure-storage")]
pub fn oauth_uses_secure_storage(profile: &str) -> Result<bool> {
    if crate::keyring::get_oauth_tokens(profile)?.is_some() {
        return Ok(true);
    }

    Ok(get_oauth_metadata(profile)?
        .map(|oauth| !oauth.client_id.is_empty() && !oauth_config_has_secrets(&oauth))
        .unwrap_or(false))
}

#[cfg(feature = "secure-storage")]
pub fn save_oauth_config_secure(profile: &str, oauth_config: &OAuthConfig) -> Result<()> {
    let json = serde_json::to_string(oauth_config)?;
    crate::keyring::set_oauth_tokens(profile, &json)?;

    let stored_json = crate::keyring::get_oauth_tokens(profile)?
        .context("OAuth tokens were written to keyring but could not be read back")?;
    let stored_oauth: OAuthConfig = serde_json::from_str(&stored_json)
        .context("OAuth token payload read back from keyring was invalid")?;

    if stored_oauth.access_token != oauth_config.access_token
        || stored_oauth.refresh_token != oauth_config.refresh_token
    {
        anyhow::bail!("OAuth tokens stored in keyring could not be verified after saving");
    }

    save_oauth_config(profile, &oauth_metadata_only(oauth_config))
}

/// Get OAuth config for a profile
pub fn get_oauth_config(profile: &str) -> Result<Option<OAuthConfig>> {
    // Try keyring first if feature enabled
    #[cfg(feature = "secure-storage")]
    {
        if let Ok(Some(json_str)) = crate::keyring::get_oauth_tokens(profile) {
            if let Ok(oauth) = serde_json::from_str::<OAuthConfig>(&json_str) {
                return Ok(Some(oauth));
            }
        }
    }

    // Fall back to config file
    Ok(get_oauth_metadata(profile)?.filter(oauth_config_has_secrets))
}

/// Clear OAuth config for a profile
pub fn clear_oauth_config(profile: &str) -> Result<()> {
    #[cfg(feature = "secure-storage")]
    {
        crate::keyring::delete_oauth_tokens(profile)?;
    }

    let mut config = load_config()?;
    if let Some(workspace) = config.workspaces.get_mut(profile) {
        workspace.oauth = None;
    }
    save_config(&config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.current.is_none());
        assert!(config.workspaces.is_empty());
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_config_serialize_deserialize() {
        let mut config = Config {
            current: Some("prod".to_string()),
            ..Default::default()
        };
        config.workspaces.insert(
            "prod".to_string(),
            Workspace::with_api_key("lin_api_prod123"),
        );
        let mut staging = Workspace::with_api_key("lin_api_staging456");
        staging.default_team = Some("ENG".to_string());
        config.workspaces.insert("staging".to_string(), staging);

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.current, Some("prod".to_string()));
        assert_eq!(parsed.workspaces.len(), 2);
        assert_eq!(parsed.workspaces["prod"].api_key, "lin_api_prod123");
        assert_eq!(parsed.workspaces["staging"].api_key, "lin_api_staging456");
        assert_eq!(
            parsed.workspaces["staging"].default_team.as_deref(),
            Some("ENG")
        );
        assert!(parsed.workspaces["prod"].default_team.is_none());
    }

    #[test]
    fn test_config_legacy_migration_parse() {
        // Legacy config format with top-level api_key
        let toml_str = r#"
            api_key = "lin_api_legacy_key"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.api_key, Some("lin_api_legacy_key".to_string()));
        assert!(config.workspaces.is_empty());
        assert!(config.current.is_none());
    }

    #[test]
    fn test_config_with_workspaces_parse() {
        let toml_str = r#"
            current = "default"

            [workspaces.default]
            api_key = "lin_api_key1"

            [workspaces.staging]
            api_key = "lin_api_key2"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.current, Some("default".to_string()));
        assert_eq!(config.workspaces.len(), 2);
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_config_api_key_not_serialized_when_none() {
        let config = Config {
            current: Some("default".to_string()),
            workspaces: HashMap::new(),
            api_key: None,
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(!toml_str.contains("api_key"));
    }

    #[test]
    fn test_config_with_oauth_parse() {
        let toml_str = r#"
            current = "oauth-profile"

            [workspaces.oauth-profile]
            api_key = ""

            [workspaces.oauth-profile.oauth]
            client_id = "abc123"
            access_token = "lin_oauth_xxx"
            refresh_token = "lin_refresh_yyy"
            expires_at = 1700000000
            token_type = "Bearer"
            scopes = ["read", "write"]
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.current, Some("oauth-profile".to_string()));
        let ws = &config.workspaces["oauth-profile"];
        assert!(ws.oauth.is_some());
        let oauth = ws.oauth.as_ref().unwrap();
        assert_eq!(oauth.client_id, "abc123");
        assert_eq!(oauth.access_token, "lin_oauth_xxx");
        assert_eq!(oauth.refresh_token, Some("lin_refresh_yyy".to_string()));
        assert_eq!(oauth.expires_at, Some(1700000000));
        assert_eq!(oauth.scopes, vec!["read", "write"]);
    }

    #[test]
    fn test_config_without_oauth_still_parses() {
        let toml_str = r#"
            current = "default"

            [workspaces.default]
            api_key = "lin_api_key1"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        let ws = &config.workspaces["default"];
        assert!(ws.oauth.is_none());
    }

    #[test]
    fn test_oauth_config_serialize() {
        let oauth = OAuthConfig {
            client_id: "test".to_string(),
            access_token: "acc".to_string(),
            refresh_token: Some("ref".to_string()),
            expires_at: Some(1700000000),
            token_type: "Bearer".to_string(),
            scopes: vec!["read".to_string()],
        };
        let json = serde_json::to_string(&oauth).unwrap();
        let parsed: OAuthConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.client_id, "test");
    }

    #[test]
    fn test_config_with_mixed_profiles() {
        let toml_str = r#"
            current = "default"

            [workspaces.default]
            api_key = "lin_api_key1"

            [workspaces.oauth-ws]
            api_key = ""

            [workspaces.oauth-ws.oauth]
            client_id = "cid"
            access_token = "at"
            token_type = "Bearer"
            scopes = ["read"]
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.workspaces["default"].oauth.is_none());
        assert!(config.workspaces["oauth-ws"].oauth.is_some());
    }

    #[test]
    fn test_oauth_config_roundtrip_toml() {
        let mut config = Config {
            current: Some("oauth-test".to_string()),
            ..Default::default()
        };
        let mut oauth_ws = Workspace::with_api_key("");
        oauth_ws.oauth = Some(OAuthConfig {
            client_id: "my-app".to_string(),
            access_token: "lin_oauth_token123".to_string(),
            refresh_token: Some("lin_refresh_abc".to_string()),
            expires_at: Some(1700000000),
            token_type: "Bearer".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
        });
        config
            .workspaces
            .insert("oauth-test".to_string(), oauth_ws);

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        let ws = &parsed.workspaces["oauth-test"];
        let oauth = ws.oauth.as_ref().unwrap();
        assert_eq!(oauth.client_id, "my-app");
        assert_eq!(oauth.access_token, "lin_oauth_token123");
        assert_eq!(oauth.refresh_token.as_deref(), Some("lin_refresh_abc"));
        assert_eq!(oauth.expires_at, Some(1700000000));
        assert_eq!(oauth.token_type, "Bearer");
        assert_eq!(oauth.scopes, vec!["read", "write"]);
    }

    #[test]
    fn test_oauth_not_serialized_when_none() {
        let mut config = Config {
            current: Some("default".to_string()),
            ..Default::default()
        };
        config
            .workspaces
            .insert("default".to_string(), Workspace::with_api_key("lin_api_key"));

        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(
            !toml_str.contains("[workspaces.default.oauth]"),
            "oauth section should not appear when None"
        );
        assert!(!toml_str.contains("client_id"));
        assert!(!toml_str.contains("access_token"));
    }

    #[test]
    fn test_oauth_config_empty_scopes() {
        let toml_str = r#"
            current = "test"

            [workspaces.test]
            api_key = ""

            [workspaces.test.oauth]
            client_id = "cid"
            access_token = "tok"
            token_type = "Bearer"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        let oauth = config.workspaces["test"].oauth.as_ref().unwrap();
        assert!(
            oauth.scopes.is_empty(),
            "scopes should default to empty vec"
        );
        assert!(oauth.refresh_token.is_none());
        assert!(oauth.expires_at.is_none());
    }

    #[test]
    fn test_oauth_config_json_roundtrip() {
        let oauth = OAuthConfig {
            client_id: "app-id".to_string(),
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: Some(1700086400),
            token_type: "Bearer".to_string(),
            scopes: vec![
                "read".to_string(),
                "write".to_string(),
                "issues:create".to_string(),
            ],
        };
        let json = serde_json::to_string(&oauth).unwrap();
        let parsed: OAuthConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.client_id, "app-id");
        assert_eq!(parsed.scopes.len(), 3);
        assert_eq!(parsed.scopes[2], "issues:create");
    }

    #[test]
    fn test_workspace_with_both_apikey_and_oauth() {
        let toml_str = r#"
            current = "dual"

            [workspaces.dual]
            api_key = "lin_api_key_primary"

            [workspaces.dual.oauth]
            client_id = "cid"
            access_token = "oauth_tok"
            token_type = "Bearer"
            scopes = ["read"]
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        let ws = &config.workspaces["dual"];
        assert_eq!(ws.api_key, "lin_api_key_primary");
        assert!(ws.oauth.is_some());
        assert_eq!(ws.oauth.as_ref().unwrap().access_token, "oauth_tok");
    }

    #[test]
    fn test_mask_api_key_for_display_masks_short_linear_keys() {
        assert_eq!(mask_api_key_for_display("lin_short"), "lin_***");
    }

    #[test]
    fn test_mask_api_key_for_display_shows_prefix_and_suffix_for_long_keys() {
        assert_eq!(mask_api_key_for_display("lin_api_prod123"), "lin_***d123");
    }

    #[test]
    fn test_mask_api_key_for_display_masks_non_linear_short_keys() {
        assert_eq!(mask_api_key_for_display("secret"), "***");
    }

    #[test]
    fn test_config_set_rejects_api_key_on_argv() {
        let err = config_set("api-key", "lin_api_secret").unwrap_err();
        assert!(err
            .to_string()
            .contains("Setting api-key via argv is disabled"));
    }

    #[test]
    fn test_config_get_unknown_key_lists_known_keys() {
        let err = config_get("nope", false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Unknown config key"));
        assert!(msg.contains("default-team"));
        assert!(msg.contains("api-key"));
        assert!(msg.contains("profile"));
    }

    #[test]
    fn test_default_team_roundtrip_toml() {
        let mut config = Config {
            current: Some("default".to_string()),
            ..Default::default()
        };
        let mut ws = Workspace::with_api_key("lin_api_key");
        ws.default_team = Some("ENG".to_string());
        config.workspaces.insert("default".to_string(), ws);

        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("default_team"));
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(
            parsed.workspaces["default"].default_team.as_deref(),
            Some("ENG")
        );
    }

    #[test]
    fn test_known_config_keys() {
        let keys = known_config_keys();
        assert!(keys.contains(&"api-key"));
        assert!(keys.contains(&"profile"));
        assert!(keys.contains(&"default-team"));
    }

    #[test]
    fn test_resolve_team_arg_prefers_cli() {
        assert_eq!(
            resolve_team_arg(Some("CLI".to_string())).as_deref(),
            Some("CLI")
        );
    }

    #[test]
    fn test_resolve_team_arg_empty_cli_falls_through() {
        // Without env/config this is None; empty CLI must not win.
        let result = resolve_team_arg(Some("  ".to_string()));
        // May be Some if env is set in the process; never equal to whitespace.
        if let Some(t) = result {
            assert_ne!(t.trim(), "");
        }
    }
}
