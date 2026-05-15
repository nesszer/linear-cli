#[cfg(feature = "secure-storage")]
use anyhow::Context;
use anyhow::Result;
use clap::Subcommand;
use dialoguer::{Confirm, Password};
use serde_json::json;

use crate::api::LinearClient;
use crate::config;
use crate::oauth;
use crate::output::{print_json_owned, OutputOptions};

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Store API key for the current profile
    Login {
        /// API key to store (if omitted, prompt interactively)
        #[arg(long, value_name = "KEY")]
        key: Option<String>,
        /// Validate the API key before saving
        #[arg(long)]
        validate: bool,
        /// Store in OS keyring instead of config file (requires secure-storage feature)
        #[arg(long)]
        secure: bool,
    },
    /// Remove API key for the current profile
    Logout {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Show current auth status
    Status {
        /// Validate API access
        #[arg(long)]
        validate: bool,
    },
    /// Migrate API keys from config file to OS keyring
    #[cfg(feature = "secure-storage")]
    Migrate {
        /// Keep keys in config file after migrating
        #[arg(long)]
        keep_config: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Authenticate via OAuth 2.0 (browser-based)
    Oauth {
        /// OAuth client ID (uses default if not specified)
        #[arg(long)]
        client_id: Option<String>,
        /// OAuth scopes (comma-separated)
        #[arg(long, default_value = "read,write,admin")]
        scopes: String,
        /// Port for localhost callback server
        #[arg(long, default_value = "8484")]
        port: u16,
        /// Store tokens in OS keyring instead of config file
        #[arg(long)]
        secure: bool,
    },
    /// Revoke OAuth tokens for the current profile
    Revoke {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

pub async fn handle(cmd: AuthCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        AuthCommands::Login {
            key,
            validate,
            secure,
        } => login(key, validate, secure, output).await,
        AuthCommands::Logout { force } => logout(force, output).await,
        AuthCommands::Status { validate } => status(validate, output).await,
        #[cfg(feature = "secure-storage")]
        AuthCommands::Migrate { keep_config, force } => migrate(keep_config, force, output).await,
        AuthCommands::Oauth {
            client_id,
            scopes,
            port,
            secure,
        } => oauth_login(client_id, scopes, port, secure, output).await,
        AuthCommands::Revoke { force } => revoke(force, output).await,
    }
}

async fn login(
    key: Option<String>,
    validate: bool,
    secure: bool,
    output: &OutputOptions,
) -> Result<()> {
    let key = match key {
        Some(key) => key,
        None => Password::new().with_prompt("Linear API key").interact()?,
    };

    if validate {
        validate_key(&key).await?;
    }

    let profile = resolve_profile_for_write()?;

    #[cfg(feature = "secure-storage")]
    if secure {
        crate::keyring::set_key(&profile, &key)?;
        let saved = crate::keyring::get_key(&profile)?
            .context("API key was written to keyring but could not be read back")?;
        if saved != key {
            anyhow::bail!("API key stored in keyring could not be verified after saving");
        }

        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({
                    "profile": profile,
                    "saved": true,
                    "storage": "keyring"
                }),
                output,
            )?;
            return Ok(());
        }

        println!("API key saved to keyring for profile '{}'", profile);
        return Ok(());
    }

    #[cfg(not(feature = "secure-storage"))]
    if secure {
        anyhow::bail!("Secure storage requires the 'secure-storage' feature. Rebuild with: cargo build --features secure-storage");
    }

    config::set_workspace_key(&profile, &key)?;

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "profile": profile,
                "saved": true,
                "storage": "config"
            }),
            output,
        )?;
        return Ok(());
    }

    println!("API key saved for profile '{}'", profile);
    Ok(())
}

async fn logout(force: bool, output: &OutputOptions) -> Result<()> {
    let profile = config::current_profile()?;

    if !force && !crate::is_yes() {
        let confirmed = Confirm::new()
            .with_prompt(format!(
                "Remove API key and profile '{}' from config?",
                profile
            ))
            .default(false)
            .interact()?;
        if !confirmed {
            return Ok(());
        }
    }

    // Remove from keyring if feature is enabled
    #[cfg(feature = "secure-storage")]
    {
        let _ = crate::keyring::delete_key(&profile); // Ignore errors (may not exist)
        let _ = crate::keyring::delete_oauth_tokens(&profile); // Ignore errors (may not exist)
    }

    config::clear_oauth_config(&profile)?;
    config::workspace_remove(&profile)?;

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "profile": profile,
                "removed": true
            }),
            output,
        )?;
        return Ok(());
    }

    println!("Removed profile '{}'", profile);
    Ok(())
}

async fn status(validate: bool, output: &OutputOptions) -> Result<()> {
    let config_data = config::load_config()?;
    let profile = config::current_profile().ok();
    let env_key = std::env::var("LINEAR_API_KEY")
        .ok()
        .filter(|k| !k.is_empty());
    let env_profile = std::env::var("LINEAR_CLI_PROFILE")
        .ok()
        .filter(|p| !p.is_empty());

    let config_file_configured = profile
        .as_ref()
        .and_then(|p| config_data.workspaces.get(p))
        .map(|w| !w.api_key.is_empty())
        .unwrap_or(false);

    // Check keyring storage
    #[cfg(feature = "secure-storage")]
    let api_key_keyring_configured = profile
        .as_ref()
        .and_then(|p| crate::keyring::get_key(p).ok())
        .flatten()
        .is_some();
    #[cfg(not(feature = "secure-storage"))]
    let api_key_keyring_configured = false;

    #[cfg(feature = "secure-storage")]
    let oauth_keyring_configured = profile
        .as_ref()
        .and_then(|p| crate::keyring::get_oauth_tokens(p).ok())
        .flatten()
        .is_some();
    #[cfg(not(feature = "secure-storage"))]
    let oauth_keyring_configured = false;

    #[cfg(feature = "secure-storage")]
    let keyring_available = crate::keyring::is_available();
    #[cfg(not(feature = "secure-storage"))]
    let keyring_available = false;

    let keyring_configured = api_key_keyring_configured || oauth_keyring_configured;

    // Check OAuth config
    let oauth_metadata = profile
        .as_ref()
        .and_then(|p| config::get_oauth_metadata(p).ok())
        .flatten();
    let oauth_configured = oauth_metadata.is_some();
    let oauth_config = profile
        .as_ref()
        .and_then(|p| config::get_oauth_config(p).ok())
        .flatten();
    let auth_type = if oauth_configured { "oauth" } else { "api_key" };
    let configured = config_file_configured || keyring_configured || oauth_configured;

    let mut validated = None;
    if validate {
        if oauth_configured {
            // Validate OAuth by querying the viewer with the access token
            if let Some(ref oauth) = oauth_config {
                let client = LinearClient::with_api_key(format!("Bearer {}", oauth.access_token));
                validated = match client {
                    Ok(c) => {
                        let query = r#"query { viewer { id } }"#;
                        Some(
                            c.query(query, None)
                                .await
                                .map(|r| !r["data"]["viewer"].is_null())
                                .unwrap_or(false),
                        )
                    }
                    Err(_) => Some(false),
                };
            } else {
                validated = Some(false);
            }
        } else {
            // Validate API key using the priority: env > keyring > config
            let key = env_key.clone().or_else(|| {
                #[cfg(feature = "secure-storage")]
                if let Some(ref p) = profile {
                    if let Ok(Some(k)) = crate::keyring::get_key(p) {
                        return Some(k);
                    }
                }
                profile
                    .as_ref()
                    .and_then(|p| config_data.workspaces.get(p))
                    .map(|w| w.api_key.clone())
            });
            validated = match key {
                Some(key) => Some(validate_key(&key).await.is_ok()),
                None => Some(false),
            };
        }
    }

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "profile": profile,
                "configured": configured,
                "keyring_configured": keyring_configured,
                "oauth_keyring_configured": oauth_keyring_configured,
                "keyring_available": keyring_available,
                "auth_type": auth_type,
                "oauth_configured": oauth_configured,
                "oauth_scopes": oauth_metadata.as_ref().map(|o| &o.scopes),
                "oauth_expires_at": oauth_metadata.as_ref().and_then(|o| o.expires_at),
                "env_api_key": env_key.is_some(),
                "env_profile": env_profile,
                "validated": validated,
            }),
            output,
        )?;
        return Ok(());
    }

    println!(
        "Profile: {}",
        profile.clone().unwrap_or_else(|| "none".to_string())
    );
    println!("Config file: {}", if configured { "yes" } else { "no" });
    println!("Keyring: {}", if keyring_configured { "yes" } else { "no" });
    println!(
        "Keyring available: {}",
        if keyring_available { "yes" } else { "no" }
    );
    println!(
        "OAuth keyring: {}",
        if oauth_keyring_configured {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "Env API key override: {}",
        if env_key.is_some() { "yes" } else { "no" }
    );
    if let Some(validated) = validated {
        println!("Validated: {}", if validated { "yes" } else { "no" });
    }
    println!("Auth type: {}", auth_type);
    if let Some(ref oauth) = oauth_metadata {
        println!("OAuth scopes: {:?}", oauth.scopes);
        if let Some(expires) = oauth.expires_at {
            let dt = chrono::DateTime::from_timestamp(expires, 0)
                .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            println!("OAuth expires: {}", dt);
        }
    }

    Ok(())
}

async fn validate_key(key: &str) -> Result<()> {
    let client = LinearClient::with_api_key(key.to_string())?;
    let query = r#"
        query {
            viewer {
                id
            }
        }
    "#;
    let result = client.query(query, None).await?;
    let viewer = &result["data"]["viewer"];
    if viewer.is_null() {
        anyhow::bail!("API key validation failed");
    }
    Ok(())
}

fn resolve_profile_for_write() -> Result<String> {
    if let Ok(profile) = std::env::var("LINEAR_CLI_PROFILE") {
        if !profile.trim().is_empty() {
            return Ok(profile);
        }
    }
    let config_data = config::load_config()?;
    Ok(config_data.current.unwrap_or_else(|| "default".to_string()))
}

#[cfg(feature = "secure-storage")]
async fn migrate(keep_config: bool, force: bool, output: &OutputOptions) -> Result<()> {
    if !crate::keyring::is_available() {
        anyhow::bail!(
            "Keyring is not available on this system. Check that a secret service is running."
        );
    }

    let config_data = config::load_config()?;

    if config_data.workspaces.is_empty() {
        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({ "migrated": 0, "message": "No workspaces to migrate" }),
                output,
            )?;
            return Ok(());
        }
        println!("No workspaces to migrate.");
        return Ok(());
    }

    let workspace_names: Vec<_> = config_data.workspaces.keys().cloned().collect();

    if !force && !crate::is_yes() {
        println!(
            "This will migrate {} workspace(s) to the keyring:",
            workspace_names.len()
        );
        for name in &workspace_names {
            println!("  - {}", name);
        }
        if !keep_config {
            println!("\nAPI keys will be removed from the config file after migration.");
        }
        let confirmed = Confirm::new()
            .with_prompt("Continue?")
            .default(false)
            .interact()?;
        if !confirmed {
            return Ok(());
        }
    }

    let mut migrated = 0;
    let mut failed: Vec<String> = Vec::new();

    for (name, workspace) in &config_data.workspaces {
        match crate::keyring::set_key(name, &workspace.api_key) {
            Ok(()) => {
                migrated += 1;
                if !output.is_json() && !output.has_template() {
                    println!("Migrated: {}", name);
                }
            }
            Err(e) => {
                failed.push(format!("{}: {}", name, e));
            }
        }
    }

    // Remove keys from config if requested and all succeeded
    if !keep_config && failed.is_empty() {
        let mut new_config = config_data;
        for name in &workspace_names {
            if let Some(ws) = new_config.workspaces.get_mut(name) {
                ws.api_key = String::new();
            }
        }
        config::save_config(&new_config)?;
    }

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "migrated": migrated,
                "failed": failed,
                "config_cleared": !keep_config && failed.is_empty()
            }),
            output,
        )?;
        return Ok(());
    }

    println!("\nMigrated {} workspace(s) to keyring.", migrated);
    if !failed.is_empty() {
        println!("Failed to migrate:");
        for f in &failed {
            println!("  - {}", f);
        }
    }
    if !keep_config && failed.is_empty() {
        println!("API keys removed from config file.");
    }

    Ok(())
}

async fn oauth_login(
    client_id: Option<String>,
    scopes: String,
    port: u16,
    secure: bool,
    output: &OutputOptions,
) -> Result<()> {
    let client_id = client_id.unwrap_or_else(|| oauth::DEFAULT_CLIENT_ID.to_string());
    let redirect_uri = format!("http://localhost:{}/callback", port);

    // Generate PKCE challenge and state
    let pkce = oauth::PkceChallenge::generate();
    let state = oauth::generate_state();

    // Build authorization URL
    let authorize_url =
        oauth::build_authorize_url(&client_id, &redirect_uri, &scopes, &state, &pkce)?;

    println!("Opening browser for Linear OAuth authentication...");
    println!("If the browser doesn't open, visit this URL:");
    println!("{}", authorize_url);
    println!();

    // Open browser
    if let Err(e) = open::that(&authorize_url) {
        eprintln!(
            "Failed to open browser: {}. Please open the URL above manually.",
            e
        );
    }

    // Wait for callback
    println!("Waiting for authorization callback on port {}...", port);
    let code = oauth::wait_for_callback(port, &state).await?;

    // Exchange code for tokens
    println!("Exchanging authorization code for tokens...");
    let tokens = oauth::exchange_code(&client_id, &redirect_uri, &code, &pkce.verifier).await?;

    // Validate the tokens by querying the viewer
    let client = LinearClient::with_api_key(format!("Bearer {}", tokens.access_token))?;
    let query = r#"query { viewer { id name email } }"#;
    let result = client.query(query, None).await?;
    let viewer = &result["data"]["viewer"];
    if viewer.is_null() {
        anyhow::bail!("OAuth token validation failed - could not fetch user info");
    }

    let user_name = viewer["name"].as_str().unwrap_or("Unknown");
    let user_email = viewer["email"].as_str().unwrap_or("Unknown");

    // Save tokens
    let profile = resolve_profile_for_write()?;
    let scopes_vec: Vec<String> = scopes.split(',').map(|s| s.trim().to_string()).collect();

    let oauth_config = config::OAuthConfig {
        client_id: client_id.clone(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: tokens.expires_at,
        token_type: tokens.token_type,
        scopes: scopes_vec.clone(),
    };

    #[cfg(feature = "secure-storage")]
    if secure {
        config::save_oauth_config_secure(&profile, &oauth_config).context(
            "Secure OAuth storage failed. On macOS, locally built or unsigned binaries may not be able to read back Keychain items. Try the official signed release or fall back to plain `linear-cli auth oauth`.",
        )?;

        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({
                    "profile": profile,
                    "auth_type": "oauth",
                    "user": user_name,
                    "email": user_email,
                    "scopes": scopes_vec,
                    "storage": "keyring",
                    "saved": true,
                }),
                output,
            )?;
            return Ok(());
        }

        println!();
        println!("OAuth authentication successful!");
        println!("  User: {} ({})", user_name, user_email);
        println!("  Scopes: {}", scopes);
        println!("  Tokens saved to keyring for profile '{}'", profile);
        return Ok(());
    }

    #[cfg(not(feature = "secure-storage"))]
    if secure {
        anyhow::bail!("Secure storage requires the 'secure-storage' feature. Rebuild with: cargo build --features secure-storage");
    }

    config::save_oauth_config(&profile, &oauth_config)?;

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "profile": profile,
                "auth_type": "oauth",
                "user": user_name,
                "email": user_email,
                "scopes": scopes_vec,
                "storage": "config",
                "saved": true,
            }),
            output,
        )?;
        return Ok(());
    }

    println!();
    println!("OAuth authentication successful!");
    println!("  User: {} ({})", user_name, user_email);
    println!("  Scopes: {}", scopes);
    println!("  Tokens saved to config for profile '{}'", profile);

    Ok(())
}

async fn revoke(force: bool, output: &OutputOptions) -> Result<()> {
    let profile = config::current_profile()?;

    let oauth_config = config::get_oauth_config(&profile)?;
    let oauth_config = match oauth_config {
        Some(c) => c,
        None => {
            anyhow::bail!(
                "No OAuth tokens found for profile '{}'. Use 'auth oauth' to authenticate.",
                profile
            );
        }
    };

    if !force && !crate::is_yes() {
        let confirmed = Confirm::new()
            .with_prompt(format!("Revoke OAuth tokens for profile '{}'?", profile))
            .default(false)
            .interact()?;
        if !confirmed {
            return Ok(());
        }
    }

    // Revoke the access token with Linear
    if let Err(e) = oauth::revoke_token(&oauth_config.access_token).await {
        eprintln!("Warning: Failed to revoke token with Linear: {}", e);
        eprintln!("Clearing local tokens anyway...");
    }

    // Clear local tokens
    config::clear_oauth_config(&profile)?;

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "profile": profile,
                "revoked": true,
            }),
            output,
        )?;
        return Ok(());
    }

    println!("OAuth tokens revoked for profile '{}'", profile);
    Ok(())
}
