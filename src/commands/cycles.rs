use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use serde_json::json;
use tabled::{Table, Tabled};

use crate::api::{resolve_team_id, LinearClient};
use crate::cache::{Cache, CacheType};
use crate::display_options;
use crate::output::{
    ensure_non_empty, filter_values, print_json, print_json_owned, sort_values, OutputOptions,
};
use crate::pagination::paginate_nodes;
use crate::text::truncate;
use crate::types::{Cycle, IssueRef, WorkflowState};

#[derive(Subcommand)]
pub enum CycleCommands {
    /// List cycles for a team
    #[command(alias = "ls")]
    List {
        /// Team ID or name (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
        /// Include completed cycles
        #[arg(short, long)]
        all: bool,
    },
    /// Get cycle details
    Get {
        /// Cycle ID
        id: String,
    },
    /// Show the current active cycle
    Current {
        /// Team ID or name (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
    },
    /// Create a new cycle
    Create {
        /// Team key, name, or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
        /// Cycle name
        #[arg(short, long)]
        name: Option<String>,
        /// Cycle description
        #[arg(short, long)]
        description: Option<String>,
        /// Start date (ISO 8601, e.g. 2024-01-01)
        #[arg(long)]
        starts_at: Option<String>,
        /// End date (ISO 8601, e.g. 2024-01-14)
        #[arg(long)]
        ends_at: Option<String>,
    },
    /// Update an existing cycle
    Update {
        /// Cycle ID
        id: String,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
        /// New start date
        #[arg(long)]
        starts_at: Option<String>,
        /// New end date
        #[arg(long)]
        ends_at: Option<String>,
        /// Preview without updating (dry run)
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete a cycle
    Delete {
        /// Cycle ID
        id: String,
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
    /// Mark a cycle as completed
    Complete {
        /// Cycle ID
        id: String,
    },
}

#[derive(Tabled)]
struct CycleRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Number")]
    number: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Start Date")]
    start_date: String,
    #[tabled(rename = "End Date")]
    end_date: String,
    #[tabled(rename = "Progress")]
    progress: String,
    #[tabled(rename = "ID")]
    id: String,
}

pub async fn handle(cmd: CycleCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        CycleCommands::List { team, all } => {
            let team = crate::config::require_team(team)?;
            list_cycles(&team, all, output).await
        }
        CycleCommands::Get { id } => get_cycle(&id, output).await,
        CycleCommands::Current { team } => {
            let team = crate::config::require_team(team)?;
            current_cycle(&team, output).await
        }
        CycleCommands::Create {
            team,
            name,
            description,
            starts_at,
            ends_at,
        } => {
            let team = crate::config::require_team(team)?;
            create_cycle(&team, name, description, starts_at, ends_at, output).await
        }
        CycleCommands::Update {
            id,
            name,
            description,
            starts_at,
            ends_at,
            dry_run,
        } => {
            let dry_run = dry_run || output.dry_run;
            update_cycle(&id, name, description, starts_at, ends_at, dry_run, output).await
        }
        CycleCommands::Delete { id, force } => delete_cycle(&id, force).await,
        CycleCommands::Complete { id } => complete_cycle(&id, output).await,
    }
}

async fn list_cycles(team: &str, include_all: bool, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    // Resolve team key/name to UUID
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    // Look up team name from the teams cache (populated by resolve_team_id)
    let team_name = Cache::new()
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

    let cycles_query = r#"
        query($teamId: String!, $first: Int, $after: String, $last: Int, $before: String) {
            team(id: $teamId) {
                cycles(first: $first, after: $after, last: $last, before: $before) {
                    nodes {
                        id
                        name
                        number
                        startsAt
                        endsAt
                        completedAt
                        progress
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
    let pagination = output.pagination.with_default_limit(50);
    let cycles = paginate_nodes(
        &client,
        cycles_query,
        vars,
        &["data", "team", "cycles", "nodes"],
        &["data", "team", "cycles", "pageInfo"],
        &pagination,
        50,
    )
    .await?;

    let cycles: Vec<_> = cycles
        .into_iter()
        .filter(|c| include_all || c["completedAt"].is_null())
        .collect();

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "team": team_name,
                "cycles": cycles
            }),
            output,
        )?;
        return Ok(());
    }

    if cycles.is_empty() {
        println!("No cycles found for team '{}'.", team_name);
        return Ok(());
    }

    let mut filtered = cycles;
    filter_values(&mut filtered, &output.filters);

    if let Some(sort_key) = output.json.sort.as_deref() {
        sort_values(&mut filtered, sort_key, output.json.order);
    }

    let width = display_options().max_width(30);
    let rows: Vec<CycleRow> = filtered
        .iter()
        .filter_map(|v| serde_json::from_value::<Cycle>(v.clone()).ok())
        .map(|c| {
            let progress = c.progress.unwrap_or(0.0);

            let status = if c.completed_at.is_some() {
                "Completed".to_string()
            } else {
                "Active".to_string()
            };

            CycleRow {
                name: truncate(c.name.as_deref().unwrap_or("-"), width),
                number: c.number.map(|n| n.to_string()).unwrap_or("-".to_string()),
                status,
                start_date: c
                    .starts_at
                    .as_deref()
                    .map(|s| s.chars().take(10).collect())
                    .unwrap_or("-".to_string()),
                end_date: c
                    .ends_at
                    .as_deref()
                    .map(|s| s.chars().take(10).collect())
                    .unwrap_or("-".to_string()),
                progress: format!("{:.0}%", progress * 100.0),
                id: c.id,
            }
        })
        .collect();

    ensure_non_empty(&filtered, output)?;
    if rows.is_empty() {
        println!(
            "No active cycles found for team '{}'. Use --all to see completed cycles.",
            team_name
        );
        return Ok(());
    }

    println!("{}", format!("Cycles for team '{}'", team_name).bold());
    println!("{}", "-".repeat(40));

    let rows_len = rows.len();
    let table = Table::new(rows).to_string();
    println!("{}", table);
    println!("\n{} cycles shown", rows_len);

    Ok(())
}

async fn get_cycle(id: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    let query = r#"
        query($id: String!) {
            cycle(id: $id) {
                id
                name
                number
                description
                startsAt
                endsAt
                completedAt
                progress
                scopeHistory
                completedScopeHistory
                team { name key }
                issues(first: 250) {
                    nodes {
                        id
                        identifier
                        title
                        state { name type }
                        assignee { name }
                        priority
                    }
                }
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": id }))).await?;
    let raw = &result["data"]["cycle"];

    if raw.is_null() {
        anyhow::bail!("Cycle not found: {}", id);
    }

    if output.is_json() || output.has_template() {
        print_json(raw, output)?;
        return Ok(());
    }

    let cycle: Cycle = serde_json::from_value(raw.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse cycle: {}", e))?;

    let progress = cycle.progress.unwrap_or(0.0);
    let cycle_number = cycle.number.unwrap_or(0);
    let default_name = format!("Cycle {}", cycle_number);
    let cycle_name = cycle.name.as_deref().unwrap_or(&default_name);

    println!("{}", cycle_name.bold());
    println!("{}", "-".repeat(40));

    if let Some(team_name) = raw["team"]["name"].as_str() {
        let team_key = raw["team"]["key"].as_str().unwrap_or("");
        println!("Team: {} ({})", team_name, team_key);
    }

    println!("Number: {}", cycle_number);

    if let Some(desc) = &cycle.description {
        if !desc.is_empty() {
            println!("Description: {}", desc);
        }
    }

    println!(
        "Start: {}",
        cycle
            .starts_at
            .as_deref()
            .map(|s| s.get(..10).unwrap_or(s))
            .unwrap_or("-")
    );
    println!(
        "End: {}",
        cycle
            .ends_at
            .as_deref()
            .map(|s| s.get(..10).unwrap_or(s))
            .unwrap_or("-")
    );
    println!("Progress: {:.0}%", progress * 100.0);

    if cycle.completed_at.is_some() {
        println!("Status: {}", "Completed".green());
    } else {
        println!("Status: {}", "Active".yellow());
    }

    println!("ID: {}", cycle.id);

    // Show issues
    let issues = raw["issues"]["nodes"].as_array();
    if let Some(issues) = issues {
        if !issues.is_empty() {
            println!("\n{}", "Issues:".bold());
            for issue_val in issues {
                let identifier = issue_val["identifier"].as_str().unwrap_or("");
                let title = truncate(
                    issue_val["title"].as_str().unwrap_or(""),
                    display_options().max_width(50),
                );
                let state_name = issue_val["state"]["name"].as_str().unwrap_or("");
                let state_type = issue_val["state"]["type"].as_str().unwrap_or("");
                let assignee = issue_val["assignee"]["name"].as_str().unwrap_or("-");

                let state_colored = match state_type {
                    "completed" => state_name.green().to_string(),
                    "started" => state_name.yellow().to_string(),
                    "canceled" | "cancelled" => state_name.red().to_string(),
                    _ => state_name.dimmed().to_string(),
                };

                println!(
                    "  {} {} [{}] ({})",
                    identifier.cyan(),
                    title,
                    state_colored,
                    assignee
                );
            }
            println!("\n{} issues", issues.len());
        }
    }

    Ok(())
}

async fn current_cycle(team: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    // Resolve team key/name to UUID
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    let query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                id
                name
                activeCycle {
                    id
                    name
                    number
                    startsAt
                    endsAt
                    progress
                    issues(first: 250) {
                        nodes {
                            id
                            identifier
                            title
                            state { name type }
                        }
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

    if output.is_json() || output.has_template() {
        print_json(team_data, output)?;
        return Ok(());
    }

    let team_name = team_data["name"].as_str().unwrap_or("");
    let cycle_val = &team_data["activeCycle"];

    if cycle_val.is_null() {
        println!("No active cycle for team '{}'.", team_name);
        return Ok(());
    }

    let cycle: Cycle = serde_json::from_value(cycle_val.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse cycle data: {}", e))?;

    let progress = cycle.progress.unwrap_or(0.0);
    let cycle_number = cycle.number.unwrap_or(0);
    let default_name = format!("Cycle {}", cycle_number);
    let cycle_name = cycle.name.as_deref().unwrap_or(&default_name);

    println!("{}", format!("Current Cycle: {}", cycle_name).bold());
    println!("{}", "-".repeat(40));

    println!("Team: {}", team_name);
    println!("Cycle Number: {}", cycle_number);
    println!(
        "Start Date: {}",
        cycle
            .starts_at
            .as_deref()
            .map(|s| s.get(..10).unwrap_or(s))
            .unwrap_or("-")
    );
    println!(
        "End Date: {}",
        cycle
            .ends_at
            .as_deref()
            .map(|s| s.get(..10).unwrap_or(s))
            .unwrap_or("-")
    );
    println!("Progress: {:.0}%", progress * 100.0);
    println!("ID: {}", cycle.id);

    // Show issues in the cycle
    let issues = cycle_val["issues"]["nodes"].as_array();
    if let Some(issues) = issues {
        if !issues.is_empty() {
            println!("\n{}", "Issues in this cycle:".bold());
            for issue_val in issues {
                let issue_ref: Option<IssueRef> = serde_json::from_value(issue_val.clone()).ok();
                let state: Option<WorkflowState> =
                    serde_json::from_value(issue_val["state"].clone()).ok();

                let identifier = issue_ref
                    .as_ref()
                    .map(|i| i.identifier.as_str())
                    .unwrap_or("");
                let title = truncate(
                    issue_ref
                        .as_ref()
                        .and_then(|i| i.title.as_deref())
                        .unwrap_or(""),
                    display_options().max_width(50),
                );
                let state_name = state.as_ref().map(|s| s.name.as_str()).unwrap_or("");
                let state_type = state
                    .as_ref()
                    .and_then(|s| s.state_type.as_deref())
                    .unwrap_or("");

                let state_colored = match state_type {
                    "completed" => state_name.green().to_string(),
                    "started" => state_name.yellow().to_string(),
                    "canceled" | "cancelled" => state_name.red().to_string(),
                    _ => state_name.dimmed().to_string(),
                };

                println!("  {} {} [{}]", identifier.cyan(), title, state_colored);
            }
        }
    }

    Ok(())
}

async fn create_cycle(
    team: &str,
    name: Option<String>,
    description: Option<String>,
    starts_at: Option<String>,
    ends_at: Option<String>,
    output: &OutputOptions,
) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    let mut input = json!({ "teamId": team_id });
    if let Some(n) = &name {
        input["name"] = json!(n);
    }
    if let Some(d) = &description {
        input["description"] = json!(d);
    }
    if let Some(s) = &starts_at {
        input["startsAt"] = json!(s);
    }
    if let Some(e) = &ends_at {
        input["endsAt"] = json!(e);
    }

    let mutation = r#"
        mutation($input: CycleCreateInput!) {
            cycleCreate(input: $input) {
                success
                cycle { id name number startsAt endsAt }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "input": input })))
        .await?;

    if result["data"]["cycleCreate"]["success"].as_bool() == Some(true) {
        let cycle = &result["data"]["cycleCreate"]["cycle"];
        if output.is_json() || output.has_template() {
            print_json(cycle, output)?;
            return Ok(());
        }
        let display_name = cycle["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("cycle");
        println!("{} Created cycle: {}", "+".green(), display_name);
        println!("  ID: {}", cycle["id"].as_str().unwrap_or(""));
        if let Some(num) = cycle["number"].as_u64() {
            println!("  Number: {}", num);
        }
    } else {
        anyhow::bail!("Failed to create cycle");
    }

    Ok(())
}

async fn update_cycle(
    id: &str,
    name: Option<String>,
    description: Option<String>,
    starts_at: Option<String>,
    ends_at: Option<String>,
    dry_run: bool,
    output: &OutputOptions,
) -> Result<()> {
    let client = LinearClient::new()?;

    let mut input = json!({});
    if let Some(n) = name {
        input["name"] = json!(n);
    }
    if let Some(d) = description {
        input["description"] = json!(d);
    }
    if let Some(s) = starts_at {
        input["startsAt"] = json!(s);
    }
    if let Some(e) = ends_at {
        input["endsAt"] = json!(e);
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
                    "would_update": { "id": id, "input": input }
                }),
                output,
            )?;
        } else {
            println!("{}", "[DRY RUN] Would update cycle:".yellow().bold());
            println!("  ID: {}", id);
        }
        return Ok(());
    }

    let mutation = r#"
        mutation($id: String!, $input: CycleUpdateInput!) {
            cycleUpdate(id: $id, input: $input) {
                success
                cycle { id name number }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if result["data"]["cycleUpdate"]["success"].as_bool() == Some(true) {
        if output.is_json() || output.has_template() {
            print_json(&result["data"]["cycleUpdate"]["cycle"], output)?;
            return Ok(());
        }
        println!("{} Cycle updated", "+".green());
    } else {
        anyhow::bail!("Failed to update cycle");
    }

    Ok(())
}

async fn delete_cycle(id: &str, force: bool) -> Result<()> {
    if !force && !crate::is_yes() {
        anyhow::bail!(
            "Delete requires --force flag. Use: linear cycles delete {} --force",
            id
        );
    }

    let client = LinearClient::new()?;

    let mutation = r#"
        mutation($id: String!) {
            cycleDelete(id: $id) {
                success
            }
        }
    "#;

    let result = client.mutate(mutation, Some(json!({ "id": id }))).await?;

    let success = result["data"]["cycleDelete"]["success"]
        .as_bool()
        .unwrap_or(false);

    if success {
        println!("Cycle {} deleted.", id);
    } else {
        anyhow::bail!("Failed to delete cycle {}", id);
    }

    Ok(())
}

async fn complete_cycle(id: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    let now = chrono::Utc::now().to_rfc3339();
    let input = json!({ "completedAt": now });

    let mutation = r#"
        mutation($id: String!, $input: CycleUpdateInput!) {
            cycleUpdate(id: $id, input: $input) {
                success
                cycle { id name number completedAt }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if result["data"]["cycleUpdate"]["success"].as_bool() == Some(true) {
        let cycle = &result["data"]["cycleUpdate"]["cycle"];

        if output.is_json() || output.has_template() {
            print_json(cycle, output)?;
            return Ok(());
        }

        let display_name = cycle["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("cycle");
        println!(
            "{} Cycle '{}' marked as completed",
            "+".green(),
            display_name
        );
    } else {
        anyhow::bail!("Failed to complete cycle");
    }

    Ok(())
}
