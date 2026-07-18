use anyhow::{Context, Result};
use dialoguer::Password;
use std::io::{self, IsTerminal, Write};

use crate::api::{self, fetch_all_teams};
use crate::config;
use crate::output::{print_json_owned, OutputOptions};

/// Guided onboarding wizard: API key, default team, output-format tip.
pub async fn handle(output: &OutputOptions) -> Result<()> {
    println!("Linear CLI Setup");
    println!("{}", "-".repeat(40));
    println!();

    // Step 1: API Key
    println!("Step 1: Authentication");
    println!("  Get your API key from: https://linear.app/settings/api");
    println!();
    let api_key = if io::stdin().is_terminal() {
        Password::new()
            .with_prompt("  Enter your Linear API key")
            .allow_empty_password(false)
            .interact()
            .context("Failed to read Linear API key")?
            .trim()
            .to_string()
    } else {
        print!("  Enter your Linear API key: ");
        io::stdout().flush()?;
        let mut api_key = String::new();
        io::stdin().read_line(&mut api_key)?;
        api_key.trim().to_string()
    };

    if api_key.is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    println!("  Validating API key...");
    println!();

    // Step 2: Validate the key and pick default team
    println!("Step 2: Default Team");
    let client = api::LinearClient::with_api_key(api_key.clone())?;

    // Paginate all teams — Linear pages connections; large workspaces need this (#34).
    let teams_arr = fetch_all_teams(&client)
        .await
        .context("Could not validate API key or fetch teams")?;

    config::set_api_key(&api_key)?;
    println!("  API key validated and saved.");

    let mut saved_team: Option<String> = None;

    if teams_arr.is_empty() {
        println!("  No teams found. Skipping default team.");
    } else {
        println!("  Available teams ({}):", teams_arr.len());
        for (i, team) in teams_arr.iter().enumerate() {
            let key = team["key"].as_str().unwrap_or("?");
            let name = team["name"].as_str().unwrap_or("?");
            println!("    {}. {} ({})", i + 1, name, key);
        }
        println!();
        print!("  Select team number, key, or name (or press Enter to skip): ");
        io::stdout().flush()?;

        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;
        let choice = choice.trim();

        if !choice.is_empty() {
            saved_team = select_team_key(&teams_arr, choice);
            if let Some(ref key) = saved_team {
                config::set_default_team(Some(key))?;
                println!("  Default team saved: {}", key);
                println!(
                    "  Tip: Override with -t {} or LINEAR_CLI_TEAM={}",
                    key, key
                );
            } else {
                println!("  Invalid selection, skipping.");
            }
        }
    }

    println!();

    // Step 3: Output format (tip only — not persisted in config)
    println!("Step 3: Output Format (shell tip; not written to config)");
    println!("  1. table (default, human-readable)");
    println!("  2. json (machine-readable, for scripts/agents)");
    println!();
    print!("  Select format [1]: ");
    io::stdout().flush()?;

    let mut format_choice = String::new();
    io::stdin().read_line(&mut format_choice)?;
    let format_choice = format_choice.trim();

    match format_choice {
        "2" | "json" => {
            println!("  Output format: json");
            println!("  Tip: Set LINEAR_CLI_OUTPUT=json in your shell profile.");
        }
        _ => {
            println!("  Output format: table (default)");
        }
    }

    println!();
    println!("Setup complete!");
    if let Some(ref team) = saved_team {
        println!("  Default team: {} (persisted in config)", team);
        println!("  Change later: linear config set default-team <KEY>");
    } else {
        println!("  Default team: not set");
        println!("  Set later: linear config set default-team <KEY>");
    }

    if output.is_json() || output.has_template() {
        print_json_owned(
            serde_json::json!({
                "setup": true,
                "api_key_saved": true,
                "default_team": saved_team,
            }),
            output,
        )?;
    }

    Ok(())
}

fn select_team_key(teams: &[serde_json::Value], choice: &str) -> Option<String> {
    if let Ok(num) = choice.parse::<usize>() {
        if num >= 1 && num <= teams.len() {
            return teams[num - 1]["key"].as_str().map(|s| s.to_string());
        }
        return None;
    }

    teams
        .iter()
        .find(|t| {
            t["key"]
                .as_str()
                .is_some_and(|k| k.eq_ignore_ascii_case(choice))
                || t["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(choice))
        })
        .and_then(|t| t["key"].as_str().map(|s| s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn select_team_by_number() {
        let teams = vec![
            json!({"key": "ENG", "name": "Engineering"}),
            json!({"key": "OPS", "name": "Operations"}),
        ];
        assert_eq!(select_team_key(&teams, "2").as_deref(), Some("OPS"));
    }

    #[test]
    fn select_team_by_key() {
        let teams = vec![json!({"key": "ENG", "name": "Engineering"})];
        assert_eq!(select_team_key(&teams, "eng").as_deref(), Some("ENG"));
    }

    #[test]
    fn select_team_invalid() {
        let teams = vec![json!({"key": "ENG", "name": "Engineering"})];
        assert!(select_team_key(&teams, "99").is_none());
        assert!(select_team_key(&teams, "nope").is_none());
    }
}
