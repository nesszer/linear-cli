use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use serde_json::{json, Value};
use tabled::{Table, Tabled};

use crate::api::{resolve_state_id, resolve_team_id, LinearClient};
use crate::cache::{Cache, CacheType};
use crate::display_options;
use crate::input::read_ids_from_stdin;
use crate::output::{
    ensure_non_empty, filter_values, print_json, print_json_owned, sort_values, OutputOptions,
};
use crate::pagination::paginate_nodes;
use crate::text::truncate;

fn safe_terminal_value(value: &str) -> String {
    crate::text::sanitize_terminal_text(value)
}

#[derive(Subcommand)]
pub enum StatusCommands {
    /// List all issue statuses for a team
    #[command(alias = "ls")]
    List {
        /// Team name or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
    },
    /// Get details of a specific status
    Get {
        /// Status name(s) or ID(s). Use "-" to read from stdin.
        ids: Vec<String>,
        /// Team name or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
    },
    /// Update a workflow state
    Update {
        /// Status name or ID
        id: String,
        /// Team name or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New color (hex)
        #[arg(short, long)]
        color: Option<String>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
        /// Preview without updating (dry run)
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Tabled)]
struct StatusRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    status_type: String,
    #[tabled(rename = "Color")]
    color: String,
    #[tabled(rename = "Position")]
    position: String,
    #[tabled(rename = "ID")]
    id: String,
}

pub async fn handle(cmd: StatusCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        StatusCommands::List { team } => {
            let team = crate::config::require_team(team)?;
            list_statuses(&team, output).await
        }
        StatusCommands::Get { ids, team } => {
            let team = crate::config::require_team(team)?;
            let final_ids = read_ids_from_stdin(ids);
            if final_ids.is_empty() {
                anyhow::bail!(
                    "No status IDs provided. Provide IDs as arguments or pipe them via stdin.\nExamples:\n  linear statuses get STATUS_ID -t ENG\n  printf '%s\\n' STATUS_ID OTHER_STATUS_ID | linear statuses get - -t ENG"
                );
            }
            get_statuses(&final_ids, &team, output).await
        }
        StatusCommands::Update {
            id,
            team,
            name,
            color,
            description,
            dry_run,
        } => {
            let team = crate::config::require_team(team)?;
            let dry_run = dry_run || output.dry_run;
            update_status(&id, &team, name, color, description, dry_run, output).await
        }
    }
}

async fn list_statuses(team: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    // Resolve team key/name to UUID
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    let can_use_cache = !output.cache.no_cache
        && output.pagination.after.is_none()
        && output.pagination.before.is_none()
        && !output.pagination.all
        && output.pagination.page_size.is_none()
        && output.pagination.limit.is_none();

    let (team_name, states): (String, Vec<Value>) = if can_use_cache {
        let cache = Cache::with_ttl(output.cache.effective_ttl_seconds())?;
        if let Some(cached) = cache.get_keyed(CacheType::Statuses, &team_id) {
            let name = cached["team_name"].as_str().unwrap_or("").to_string();
            let states_data = cached["states"].as_array().cloned().unwrap_or_default();
            (name, states_data)
        } else {
            (String::new(), Vec::new())
        }
    } else {
        (String::new(), Vec::new())
    };

    let (team_name, states) = if !states.is_empty() {
        (team_name, states)
    } else {
        // Look up team name from the teams cache (populated by resolve_team_id)
        let name = Cache::new()
            .ok()
            .and_then(|c| c.get(CacheType::Teams))
            .and_then(|teams| {
                teams.as_array().and_then(|arr| {
                    arr.iter()
                        .find(|t| t["id"].as_str() == Some(&team_id))
                        .and_then(|t| t["name"].as_str().map(|s| s.to_string()))
                })
            })
            .unwrap_or_else(|| team.to_string());

        let states_query = r#"
            query($teamId: String!, $first: Int, $after: String, $last: Int, $before: String) {
                team(id: $teamId) {
                    states(first: $first, after: $after, last: $last, before: $before) {
                        nodes {
                            id
                            name
                            type
                            color
                            position
                            description
                        }
                        pageInfo {
                            hasNextPage
                            endCursor
                            hasPreviousPage
                            startCursor
                        }
                    }
                }
            }
        "#;

        let mut vars = serde_json::Map::new();
        vars.insert("teamId".to_string(), json!(team_id));
        let pagination = output.pagination.with_default_limit(100);
        let states = paginate_nodes(
            &client,
            states_query,
            vars,
            &["data", "team", "states", "nodes"],
            &["data", "team", "states", "pageInfo"],
            &pagination,
            100,
        )
        .await?;

        if !output.cache.no_cache {
            let cache = Cache::with_ttl(output.cache.effective_ttl_seconds())?;
            let cache_data = json!({
                "team_name": name,
                "states": states,
            });
            let _ = cache.set_keyed(CacheType::Statuses, &team_id, cache_data);
        }

        (name, states)
    };

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "team": team_name,
                "statuses": states
            }),
            output,
        )?;
        return Ok(());
    }

    if states.is_empty() {
        println!("No statuses found for team '{}'.", team_name);
        return Ok(());
    }

    println!(
        "{}",
        format!("Issue statuses for team '{}'", team_name).bold()
    );
    println!("{}", "-".repeat(50));

    let width = display_options().max_width(30);
    let mut states = states;
    filter_values(&mut states, &output.filters);
    if let Some(sort_key) = output.json.sort.as_deref() {
        sort_values(&mut states, sort_key, output.json.order);
    }

    ensure_non_empty(&states, output)?;
    let rows: Vec<StatusRow> = states
        .iter()
        .map(|s| {
            let status_type = s["type"].as_str().unwrap_or("");
            let type_colored = match status_type {
                "completed" => status_type.green().to_string(),
                "started" => status_type.yellow().to_string(),
                "canceled" | "cancelled" => status_type.red().to_string(),
                "backlog" => status_type.dimmed().to_string(),
                "unstarted" => status_type.cyan().to_string(),
                _ => status_type.to_string(),
            };

            StatusRow {
                name: truncate(s["name"].as_str().unwrap_or(""), width),
                status_type: type_colored,
                color: s["color"].as_str().unwrap_or("").to_string(),
                position: s["position"]
                    .as_f64()
                    .map(|p| format!("{:.0}", p))
                    .unwrap_or("-".to_string()),
                id: s["id"].as_str().unwrap_or("").to_string(),
            }
        })
        .collect();

    let table = Table::new(rows).to_string();
    println!("{}", table);
    println!("\n{} statuses", states.len());

    Ok(())
}

async fn get_statuses(ids: &[String], team: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    // Resolve team key/name to UUID
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    // First get all states for the team and find the matching one
    let query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                id
                name
                states {
                    nodes {
                        id
                        name
                        type
                        color
                        position
                        description
                    }
                }
            }
        }
    "#;

    let result = client
        .query(query, Some(json!({ "teamId": team_id })))
        .await?;
    let team_data = &result["data"]["team"];

    if team_data.is_null() {
        anyhow::bail!("Team not found: {}", team);
    }

    let empty = vec![];
    let states = team_data["states"]["nodes"].as_array().unwrap_or(&empty);

    let mut found: Vec<serde_json::Value> = Vec::new();
    for id in ids {
        let status = states.iter().find(|s| {
            s["id"].as_str() == Some(id.as_str())
                || s["name"].as_str().map(|n| n.to_lowercase()) == Some(id.to_lowercase())
        });

        if let Some(s) = status {
            found.push(s.clone());
        } else if !output.is_json() && !output.has_template() {
            eprintln!("{} Status not found: {}", "!".yellow(), id);
        }
    }

    if output.is_json() || output.has_template() {
        print_json_owned(serde_json::json!(found), output)?;
        return Ok(());
    }

    for (idx, status) in found.iter().enumerate() {
        if idx > 0 {
            println!();
        }
        println!(
            "{}",
            safe_terminal_value(status["name"].as_str().unwrap_or("")).bold()
        );
        println!("{}", "-".repeat(40));
        println!(
            "Type: {}",
            safe_terminal_value(status["type"].as_str().unwrap_or("-"))
        );
        println!(
            "Color: {}",
            safe_terminal_value(status["color"].as_str().unwrap_or("-"))
        );
        println!(
            "Position: {}",
            status["position"]
                .as_f64()
                .map(|p| format!("{:.0}", p))
                .unwrap_or("-".to_string())
        );
        if let Some(desc) = status["description"].as_str() {
            if !desc.is_empty() {
                println!("Description: {}", safe_terminal_value(desc));
            }
        }
        println!(
            "ID: {}",
            safe_terminal_value(status["id"].as_str().unwrap_or("-"))
        );
    }

    Ok(())
}

async fn update_status(
    id: &str,
    team: &str,
    name: Option<String>,
    color: Option<String>,
    description: Option<String>,
    dry_run: bool,
    output: &OutputOptions,
) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;
    let state_id = resolve_state_id(&client, &team_id, id).await?;

    let mut input = json!({});
    if let Some(n) = name {
        input["name"] = json!(n);
    }
    if let Some(c) = color {
        input["color"] = json!(c);
    }
    if let Some(d) = description {
        input["description"] = json!(d);
    }

    if input.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        println!("No updates specified.");
        return Ok(());
    }

    if dry_run {
        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({
                    "dry_run": true,
                    "would_update": { "id": state_id, "input": input }
                }),
                output,
            )?;
        } else {
            println!("{}", "[DRY RUN] Would update status:".yellow().bold());
            println!("  ID: {}", state_id);
        }
        return Ok(());
    }

    let mutation = r#"
        mutation($id: String!, $input: WorkflowStateUpdateInput!) {
            workflowStateUpdate(id: $id, input: $input) {
                success
                workflowState { id name color description }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "id": state_id, "input": input })))
        .await?;

    if result["data"]["workflowStateUpdate"]["success"].as_bool() == Some(true) {
        let state = &result["data"]["workflowStateUpdate"]["workflowState"];

        if output.is_json() || output.has_template() {
            print_json(state, output)?;
            return Ok(());
        }

        println!("{} Status updated", "+".green());

        // Invalidate statuses cache after successful update
        let _ = Cache::new().and_then(|c| c.clear_type(CacheType::Statuses));
    } else {
        anyhow::bail!("Failed to update status");
    }

    Ok(())
}
