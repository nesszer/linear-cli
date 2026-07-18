use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use tabled::{Table, Tabled};

use crate::api::{resolve_team_id, LinearClient};
use crate::output::{print_json, print_json_owned, OutputOptions};

#[derive(Subcommand)]
pub enum SprintCommands {
    /// Show current sprint status and progress
    Status {
        /// Team key, name, or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
    },
    /// Show sprint progress (completion %)
    Progress {
        /// Team key, name, or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
    },
    /// List issues planned for next cycle
    Plan {
        /// Team key, name, or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
    },
    /// Move incomplete issues from current cycle to next
    CarryOver {
        /// Team key, name, or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Show ASCII burndown chart for current sprint
    Burndown {
        /// Team key, name, or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
        /// Chart width in characters (default: 60)
        #[arg(long, default_value = "60")]
        width: usize,
        /// Chart height in lines (default: 15)
        #[arg(long, default_value = "15")]
        height: usize,
    },
    /// Show sprint velocity across recent cycles
    Velocity {
        /// Team key, name, or ID (falls back to default-team / LINEAR_CLI_TEAM)
        #[arg(short, long)]
        team: Option<String>,
        /// Number of past cycles to analyze (default: 6)
        #[arg(short = 'n', long, default_value = "6")]
        count: usize,
    },
}

pub async fn handle(cmd: SprintCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        SprintCommands::Status { team } => {
            let team = crate::config::require_team(team)?;
            sprint_status(&team, output).await
        }
        SprintCommands::Progress { team } => {
            let team = crate::config::require_team(team)?;
            sprint_progress(&team, output).await
        }
        SprintCommands::Plan { team } => {
            let team = crate::config::require_team(team)?;
            sprint_plan(&team, output).await
        }
        SprintCommands::CarryOver { team, force } => {
            let team = crate::config::require_team(team)?;
            sprint_carry_over(&team, force, output).await
        }
        SprintCommands::Burndown {
            team,
            width,
            height,
        } => {
            let team = crate::config::require_team(team)?;
            burndown(&team, width, height, output).await
        }
        SprintCommands::Velocity { team, count } => {
            let team = crate::config::require_team(team)?;
            velocity(&team, count, output).await
        }
    }
}

async fn sprint_status(team: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    let query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                name
                activeCycle {
                    id name number
                    startsAt endsAt
                    progress
                    scopeHistory
                    issues(first: 250) {
                        nodes {
                            id identifier title
                            state { name type }
                            priority
                            assignee { name }
                            estimate
                            createdAt
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

    let team_name = team_data["name"].as_str().unwrap_or(team);
    let cycle = &team_data["activeCycle"];

    if cycle.is_null() {
        if output.is_json() || output.has_template() {
            print_json_owned(json!({ "team": team_name, "activeCycle": null }), output)?;
        } else {
            println!("No active cycle for team '{}'.", team_name);
        }
        return Ok(());
    }

    if output.is_json() || output.has_template() {
        print_json(cycle, output)?;
        return Ok(());
    }

    let cycle_name = cycle["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("(unnamed)");
    let cycle_number = cycle["number"].as_u64().unwrap_or(0);
    let progress = cycle["progress"].as_f64().unwrap_or(0.0);
    let start_date = cycle["startsAt"]
        .as_str()
        .map(|s| s.get(..10).unwrap_or(s))
        .unwrap_or("-");
    let end_date = cycle["endsAt"]
        .as_str()
        .map(|s| s.get(..10).unwrap_or(s))
        .unwrap_or("-");

    let issues = cycle["issues"]["nodes"].as_array();

    let (total, completed, in_progress, scope_change) = if let Some(issues) = issues {
        let total = issues.len();
        let completed = issues
            .iter()
            .filter(|i| i["state"]["type"].as_str() == Some("completed"))
            .count();
        let in_progress = issues
            .iter()
            .filter(|i| i["state"]["type"].as_str() == Some("started"))
            .count();

        // Scope change: compare current total to first entry in scopeHistory
        let scope_change = cycle["scopeHistory"]
            .as_array()
            .and_then(|h| h.first())
            .and_then(|v| v.as_f64())
            .map(|initial| total as i64 - initial as i64)
            .unwrap_or(0);

        (total, completed, in_progress, scope_change)
    } else {
        (0, 0, 0, 0)
    };

    println!(
        "{}",
        format!("Sprint {} - {}", cycle_number, cycle_name).bold()
    );
    println!("{}", "-".repeat(40));
    println!("Team:        {}", team_name);
    println!("Dates:       {} to {}", start_date, end_date);
    println!("Progress:    {:.0}%", progress * 100.0);
    println!();
    println!("Issues:      {}", total);
    println!("  Completed: {}", completed.to_string().green());
    println!("  In Prog:   {}", in_progress.to_string().yellow());
    println!(
        "  Remaining: {}",
        (total - completed - in_progress).to_string().dimmed()
    );

    if scope_change != 0 {
        let sign = if scope_change > 0 { "+" } else { "" };
        println!(
            "  Scope:     {} issues",
            format!("{}{}", sign, scope_change).red()
        );
    }

    // Show estimate totals if any issues have estimates
    if let Some(issues) = issues {
        let total_estimate: f64 = issues.iter().filter_map(|i| i["estimate"].as_f64()).sum();
        let completed_estimate: f64 = issues
            .iter()
            .filter(|i| i["state"]["type"].as_str() == Some("completed"))
            .filter_map(|i| i["estimate"].as_f64())
            .sum();

        if total_estimate > 0.0 {
            println!();
            println!(
                "Estimates:   {:.0} / {:.0} points",
                completed_estimate, total_estimate
            );
        }
    }

    Ok(())
}

async fn sprint_progress(team: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    let query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                name
                activeCycle {
                    id name number progress
                    issues(first: 250) {
                        nodes {
                            id
                            state { type }
                            estimate
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

    let cycle = &team_data["activeCycle"];

    if cycle.is_null() {
        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({ "team": team_data["name"], "activeCycle": null }),
                output,
            )?;
        } else {
            println!(
                "No active cycle for team '{}'.",
                team_data["name"].as_str().unwrap_or(team)
            );
        }
        return Ok(());
    }

    let issues = cycle["issues"]["nodes"].as_array();
    let cycle_number = cycle["number"].as_u64().unwrap_or(0);
    let progress = cycle["progress"].as_f64().unwrap_or(0.0);

    let (total, completed, in_progress, todo) = if let Some(issues) = issues {
        let total = issues.len();
        let completed = issues
            .iter()
            .filter(|i| i["state"]["type"].as_str() == Some("completed"))
            .count();
        let in_progress = issues
            .iter()
            .filter(|i| i["state"]["type"].as_str() == Some("started"))
            .count();
        let todo = total - completed - in_progress;
        (total, completed, in_progress, todo)
    } else {
        (0, 0, 0, 0)
    };

    if output.is_json() || output.has_template() {
        let total_estimate: f64 = issues
            .map(|arr| arr.iter().filter_map(|i| i["estimate"].as_f64()).sum())
            .unwrap_or(0.0);
        let completed_estimate: f64 = issues
            .map(|arr| {
                arr.iter()
                    .filter(|i| i["state"]["type"].as_str() == Some("completed"))
                    .filter_map(|i| i["estimate"].as_f64())
                    .sum()
            })
            .unwrap_or(0.0);

        print_json_owned(
            json!({
                "cycle_number": cycle_number,
                "progress": progress,
                "total": total,
                "completed": completed,
                "in_progress": in_progress,
                "todo": todo,
                "total_estimate": total_estimate,
                "completed_estimate": completed_estimate,
            }),
            output,
        )?;
        return Ok(());
    }

    // Visual progress bar
    let bar_width: usize = 20;
    let filled = (progress * bar_width as f64).round() as usize;
    let empty = bar_width.saturating_sub(filled);
    let bar = format!(
        "[{}{}]",
        "\u{2588}".repeat(filled).green(),
        "\u{2591}".repeat(empty).dimmed()
    );

    println!(
        "Sprint {}: {} {:.0}% ({}/{} issues)",
        cycle_number,
        bar,
        progress * 100.0,
        completed,
        total
    );
    println!(
        "  Completed: {}  In Progress: {}  Todo: {}",
        completed.to_string().green(),
        in_progress.to_string().yellow(),
        todo.to_string().dimmed()
    );

    // Estimate summary
    if let Some(issues) = issues {
        let total_estimate: f64 = issues.iter().filter_map(|i| i["estimate"].as_f64()).sum();
        let completed_estimate: f64 = issues
            .iter()
            .filter(|i| i["state"]["type"].as_str() == Some("completed"))
            .filter_map(|i| i["estimate"].as_f64())
            .sum();

        if total_estimate > 0.0 {
            println!(
                "  Estimate: {:.0} points completed / {:.0} total",
                completed_estimate, total_estimate
            );
        }
    }

    Ok(())
}

async fn sprint_plan(team: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    let query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                name
                upcomingCycles(first: 1) {
                    nodes {
                        id name number startsAt endsAt
                        issues(first: 250) {
                            nodes {
                                id identifier title priority
                                state { name }
                                assignee { name }
                                estimate
                            }
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

    let team_name = team_data["name"].as_str().unwrap_or(team);
    let cycles = team_data["upcomingCycles"]["nodes"].as_array();

    let next_cycle = cycles.and_then(|arr| arr.first());

    if next_cycle.is_none() {
        if output.is_json() || output.has_template() {
            print_json_owned(json!({ "team": team_name, "nextCycle": null }), output)?;
        } else {
            println!("No upcoming cycle for team '{}'.", team_name);
        }
        return Ok(());
    }

    let cycle = next_cycle.unwrap();

    if output.is_json() || output.has_template() {
        print_json(cycle, output)?;
        return Ok(());
    }

    let cycle_name = cycle["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("(unnamed)");
    let cycle_number = cycle["number"].as_u64().unwrap_or(0);
    let start_date = cycle["startsAt"]
        .as_str()
        .map(|s| s.get(..10).unwrap_or(s))
        .unwrap_or("-");
    let end_date = cycle["endsAt"]
        .as_str()
        .map(|s| s.get(..10).unwrap_or(s))
        .unwrap_or("-");

    println!(
        "{}",
        format!("Next Sprint {} - {}", cycle_number, cycle_name).bold()
    );
    println!("{}", "-".repeat(40));
    println!("Dates: {} to {}", start_date, end_date);

    let issues = cycle["issues"]["nodes"].as_array();

    if let Some(issues) = issues {
        if issues.is_empty() {
            println!("\nNo issues planned yet.");
        } else {
            let total_estimate: f64 = issues.iter().filter_map(|i| i["estimate"].as_f64()).sum();

            println!("\n{} ({} issues)", "Planned Issues:".bold(), issues.len());
            if total_estimate > 0.0 {
                println!("Total estimate: {:.0} points", total_estimate);
            }
            println!();

            for issue in issues {
                let identifier = issue["identifier"].as_str().unwrap_or("");
                let title = issue["title"].as_str().unwrap_or("");
                let state = issue["state"]["name"].as_str().unwrap_or("-");
                let assignee = issue["assignee"]["name"].as_str().unwrap_or("-");
                let estimate = issue["estimate"]
                    .as_f64()
                    .map(|e| format!(" [{:.0}p]", e))
                    .unwrap_or_default();

                println!(
                    "  {} {}{} [{}] ({})",
                    identifier.cyan(),
                    title,
                    estimate.dimmed(),
                    state,
                    assignee
                );
            }
        }
    }

    Ok(())
}

async fn sprint_carry_over(team: &str, force: bool, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    // Get current cycle's incomplete issues
    let current_query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                name
                activeCycle {
                    id name number
                    issues(first: 250) {
                        nodes {
                            id identifier title
                            state { name type }
                        }
                    }
                }
            }
        }
    "#;

    let result = client
        .query(current_query, Some(json!({ "teamId": team_id })))
        .await?;
    let team_data = &result["data"]["team"];

    if team_data.is_null() {
        anyhow::bail!("Team not found: {}", team);
    }

    let team_name = team_data["name"].as_str().unwrap_or(team);
    let current_cycle = &team_data["activeCycle"];

    if current_cycle.is_null() {
        anyhow::bail!("No active cycle for team '{}'.", team_name);
    }

    // Get next cycle
    let next_query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                upcomingCycles(first: 1) {
                    nodes { id name number }
                }
            }
        }
    "#;

    let next_result = client
        .query(next_query, Some(json!({ "teamId": team_id })))
        .await?;
    let next_cycles = next_result["data"]["team"]["upcomingCycles"]["nodes"].as_array();
    let next_cycle = next_cycles
        .and_then(|arr| arr.first())
        .ok_or_else(|| anyhow::anyhow!("No upcoming cycle to carry issues over to."))?;

    let next_cycle_id = next_cycle["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Could not get next cycle ID"))?;

    // Find incomplete issues (not completed, not canceled)
    let incomplete: Vec<&serde_json::Value> = current_cycle["issues"]["nodes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|i| {
                    let state_type = i["state"]["type"].as_str().unwrap_or("");
                    state_type != "completed" && state_type != "canceled"
                })
                .collect()
        })
        .unwrap_or_default();

    if incomplete.is_empty() {
        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({
                    "carried_over": 0,
                    "message": "No incomplete issues to carry over"
                }),
                output,
            )?;
        } else {
            println!("No incomplete issues in the current cycle.");
        }
        return Ok(());
    }

    // Confirmation
    if !force && !crate::is_yes() {
        println!(
            "Will move {} incomplete issues from current cycle to next cycle:",
            incomplete.len()
        );
        for issue in &incomplete {
            let identifier = issue["identifier"].as_str().unwrap_or("");
            let title = issue["title"].as_str().unwrap_or("");
            let state = issue["state"]["name"].as_str().unwrap_or("-");
            println!("  {} {} [{}]", identifier.cyan(), title, state);
        }
        println!();
        anyhow::bail!(
            "Use --force or --yes to confirm. {} issues would be moved.",
            incomplete.len()
        );
    }

    // Move issues in parallel
    let issue_ids: Vec<String> = incomplete
        .iter()
        .filter_map(|i| i["id"].as_str().map(|s| s.to_string()))
        .collect();

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue { id identifier }
            }
        }
    "#;

    let results: Vec<(String, bool)> = stream::iter(issue_ids.iter())
        .map(|issue_id| {
            let client = &client;
            let id = issue_id.clone();
            let cycle_id = next_cycle_id.to_string();
            async move {
                let result = client
                    .mutate(
                        mutation,
                        Some(json!({ "id": id, "input": { "cycleId": cycle_id } })),
                    )
                    .await;
                let success = result
                    .as_ref()
                    .map(|r| {
                        r["data"]["issueUpdate"]["success"]
                            .as_bool()
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                (id, success)
            }
        })
        .buffer_unordered(10)
        .collect()
        .await;

    let moved = results.iter().filter(|(_, s)| *s).count();
    let failed = results.iter().filter(|(_, s)| !*s).count();

    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "carried_over": moved,
                "failed": failed,
                "next_cycle": next_cycle["name"],
                "next_cycle_number": next_cycle["number"],
            }),
            output,
        )?;
    } else {
        println!(
            "{} Moved {} issues to next cycle ({})",
            "+".green(),
            moved,
            next_cycle["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("upcoming")
        );
        if failed > 0 {
            println!("{} {} issues failed to move", "!".red(), failed);
        }
    }

    Ok(())
}

async fn burndown(team: &str, width: usize, height: usize, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    let query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                name
                activeCycle {
                    id name number
                    startsAt endsAt
                    scopeHistory
                    completedScopeHistory
                    issueCountHistory
                    completedIssueCountHistory
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

    let team_name = team_data["name"].as_str().unwrap_or(team);
    let cycle = &team_data["activeCycle"];

    if cycle.is_null() {
        if output.is_json() || output.has_template() {
            print_json_owned(json!({ "team": team_name, "activeCycle": null }), output)?;
        } else {
            println!("No active cycle for team '{}'.", team_name);
        }
        return Ok(());
    }

    // Extract history arrays
    let scope_history: Vec<f64> = cycle["scopeHistory"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
        .unwrap_or_default();
    let completed_scope_history: Vec<f64> = cycle["completedScopeHistory"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
        .unwrap_or_default();
    let issue_count_history: Vec<f64> = cycle["issueCountHistory"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
        .unwrap_or_default();
    let completed_issue_count_history: Vec<f64> = cycle["completedIssueCountHistory"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
        .unwrap_or_default();

    let cycle_number = cycle["number"].as_u64().unwrap_or(0);
    let start_date = cycle["startsAt"]
        .as_str()
        .map(|s| s.get(..10).unwrap_or(s))
        .unwrap_or("-");
    let end_date = cycle["endsAt"]
        .as_str()
        .map(|s| s.get(..10).unwrap_or(s))
        .unwrap_or("-");

    // JSON output: return raw history data
    if output.is_json() || output.has_template() {
        print_json_owned(
            json!({
                "cycle_number": cycle_number,
                "startsAt": start_date,
                "endsAt": end_date,
                "scopeHistory": scope_history,
                "completedScopeHistory": completed_scope_history,
                "issueCountHistory": issue_count_history,
                "completedIssueCountHistory": completed_issue_count_history,
            }),
            output,
        )?;
        return Ok(());
    }

    // Prefer scope (points) history; fall back to issue count history
    let (scope, completed, label) =
        if !scope_history.is_empty() && !completed_scope_history.is_empty() {
            (&scope_history, &completed_scope_history, "points")
        } else if !issue_count_history.is_empty() && !completed_issue_count_history.is_empty() {
            (
                &issue_count_history,
                &completed_issue_count_history,
                "issues",
            )
        } else {
            println!("No burndown data available yet.");
            return Ok(());
        };

    // Use the shorter length if they differ
    let len = scope.len().min(completed.len());
    if len == 0 {
        println!("No burndown data available yet.");
        return Ok(());
    }

    // Compute remaining work per day
    let remaining: Vec<f64> = (0..len)
        .map(|i| (scope[i] - completed[i]).max(0.0))
        .collect();

    // Compute ideal burndown: linear from initial scope down to 0
    let initial_scope = scope[0];
    let total_days = len.max(1) as f64;
    let ideal: Vec<f64> = (0..len)
        .map(|i| {
            let progress = i as f64 / (total_days - 1.0).max(1.0);
            (initial_scope * (1.0 - progress)).max(0.0)
        })
        .collect();

    let initial_scope_display = initial_scope as u64;
    let title = format!(
        "Sprint {} Burndown ({} {} | {} to {})",
        cycle_number, initial_scope_display, label, start_date, end_date
    );

    let chart = render_burndown(&remaining, &ideal, width, height, &title);
    println!("{}", chart);

    Ok(())
}

fn render_burndown(
    remaining: &[f64],
    ideal: &[f64],
    width: usize,
    height: usize,
    title: &str,
) -> String {
    let height = height.max(3);
    let data_len = remaining.len();
    let chart_width = width.min(data_len);

    let max_val = remaining
        .iter()
        .chain(ideal.iter())
        .cloned()
        .fold(0.0f64, f64::max)
        .max(1.0);

    let mut lines = Vec::new();
    lines.push(format!("{}", title.bold()));
    lines.push(String::new());

    // Determine y-axis label width
    let y_label_width = format!("{:.0}", max_val).len().max(4);

    // For each row (top to bottom)
    for row in (0..height).rev() {
        let y_val = max_val * row as f64 / (height - 1) as f64;
        let label = format!("{:>width$.0}", y_val, width = y_label_width);
        let mut line = format!("{} \u{2502}", label);

        for col in 0..chart_width {
            // Map column to data index (scale if chart_width < data_len)
            let data_idx = if chart_width > 1 {
                col * (data_len - 1) / (chart_width - 1)
            } else {
                0
            };

            let remaining_y = remaining[data_idx];
            let ideal_y = ideal[data_idx];

            // Map values to row positions
            let remaining_row = if max_val > 0.0 {
                (remaining_y / max_val * (height - 1) as f64).round() as usize
            } else {
                0
            };
            let ideal_row = if max_val > 0.0 {
                (ideal_y / max_val * (height - 1) as f64).round() as usize
            } else {
                0
            };

            if remaining_row == row {
                // Color actual line: yellow if above ideal, green if at/below
                let ch = if remaining_y > ideal_y + 0.01 {
                    format!("{}", "\u{25cf}".yellow())
                } else {
                    format!("{}", "\u{25cf}".green())
                };
                line.push_str(&ch);
            } else if ideal_row == row {
                line.push_str(&format!("{}", "\u{2500}".dimmed()));
            } else {
                line.push(' ');
            }
        }

        lines.push(line);
    }

    // X-axis border
    let x_border = format!(
        "{} \u{2514}{}",
        " ".repeat(y_label_width),
        "\u{2500}".repeat(chart_width)
    );
    lines.push(x_border);

    // Day labels
    if chart_width >= 10 {
        let day_end = format!("Day {}", data_len);
        let padding = chart_width.saturating_sub(5 + day_end.len());
        let day_line = format!(
            "{} Day 1{}{}",
            " ".repeat(y_label_width + 1),
            " ".repeat(padding),
            day_end
        );
        lines.push(day_line);
    } else {
        let day_line = format!(
            "{} Day 1 .. Day {}",
            " ".repeat(y_label_width + 1),
            data_len
        );
        lines.push(day_line);
    }

    // Legend
    lines.push(String::new());
    lines.push(format!(
        "  {} Actual remaining   {} Ideal burndown",
        "\u{25cf}".yellow(),
        "\u{2500}".dimmed()
    ));

    lines.join("\n")
}

#[derive(Tabled)]
struct VelocityRow {
    #[tabled(rename = "Sprint")]
    sprint: String,
    #[tabled(rename = "Issues")]
    issues: String,
    #[tabled(rename = "Points")]
    points: String,
    #[tabled(rename = "Done %")]
    done_pct: String,
    #[tabled(rename = "Duration")]
    duration: String,
}

async fn fetch_past_cycles_for_velocity(
    client: &LinearClient,
    team_id: &str,
    team_label: &str,
) -> Result<(String, Vec<Value>)> {
    let query = r#"
        query($teamId: String!, $first: Int!, $after: String) {
            team(id: $teamId) {
                name
                cycles(
                    first: $first
                    after: $after
                    orderBy: createdAt
                    filter: { isPast: { eq: true } }
                ) {
                    nodes {
                        id name number
                        startsAt endsAt completedAt
                        issueCountHistory
                        completedIssueCountHistory
                        scopeHistory
                        completedScopeHistory
                        progress
                    }
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                }
            }
        }
    "#;

    let mut team_name: Option<String> = None;
    let mut cycles: Vec<Value> = Vec::new();
    let mut after: Option<String> = None;

    loop {
        let result = client
            .query(
                query,
                Some(json!({
                    "teamId": team_id,
                    "first": 50,
                    "after": after,
                })),
            )
            .await?;
        let team_data = &result["data"]["team"];

        if team_data.is_null() {
            anyhow::bail!("Team not found: {}", team_label);
        }

        if team_name.is_none() {
            team_name = Some(team_data["name"].as_str().unwrap_or(team_label).to_string());
        }

        cycles.extend(
            team_data["cycles"]["nodes"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );

        let page_info = &team_data["cycles"]["pageInfo"];
        if page_info["hasNextPage"].as_bool() != Some(true) {
            break;
        }

        after = page_info["endCursor"]
            .as_str()
            .map(|cursor| cursor.to_string());
        if after.is_none() {
            break;
        }
    }

    Ok((team_name.unwrap_or_else(|| team_label.to_string()), cycles))
}

fn select_recent_cycles_for_velocity(mut cycles: Vec<Value>, count: usize) -> Vec<Value> {
    if count == 0 {
        return Vec::new();
    }

    cycles.sort_by(|a, b| {
        let a_start = a["startsAt"].as_str().unwrap_or("");
        let b_start = b["startsAt"].as_str().unwrap_or("");
        a_start.cmp(b_start)
    });

    if cycles.len() > count {
        cycles = cycles.split_off(cycles.len() - count);
    }

    cycles
}

async fn velocity(team: &str, count: usize, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;
    let (team_name, cycles) = fetch_past_cycles_for_velocity(&client, &team_id, team).await?;
    let cycles = select_recent_cycles_for_velocity(cycles, count);

    if cycles.is_empty() {
        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({ "team": team_name, "cycles": [], "average_points": null, "trend": null }),
                output,
            )?;
        } else {
            println!("No past cycles for team '{}'.", team_name);
        }
        return Ok(());
    }

    // Extract per-cycle stats
    struct CycleStats {
        name: String,
        number: u64,
        issues_completed: u64,
        issues_total: u64,
        points_completed: u64,
        points_total: u64,
        progress: f64,
        duration_days: u64,
    }

    let stats: Vec<CycleStats> = cycles
        .iter()
        .map(|c| {
            let name = c["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("(unnamed)")
                .to_string();
            let number = c["number"].as_u64().unwrap_or(0);

            let issues_completed = c["completedIssueCountHistory"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u64;
            let issues_total = c["issueCountHistory"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u64;
            let points_completed = c["completedScopeHistory"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u64;
            let points_total = c["scopeHistory"]
                .as_array()
                .and_then(|a| a.last())
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u64;
            let progress = c["progress"].as_f64().unwrap_or(0.0);

            let duration_days = match (c["startsAt"].as_str(), c["endsAt"].as_str()) {
                (Some(start), Some(end)) => {
                    let start_date = start.get(..10).unwrap_or(start);
                    let end_date = end.get(..10).unwrap_or(end);
                    // Parse YYYY-MM-DD manually
                    parse_duration_days(start_date, end_date).unwrap_or(14)
                }
                _ => 14,
            };

            CycleStats {
                name,
                number,
                issues_completed,
                issues_total,
                points_completed,
                points_total,
                progress,
                duration_days,
            }
        })
        .collect();

    // JSON output
    if output.is_json() || output.has_template() {
        let points_vals: Vec<u64> = stats.iter().map(|s| s.points_completed).collect();
        let avg = if points_vals.is_empty() {
            0.0
        } else {
            points_vals.iter().sum::<u64>() as f64 / points_vals.len() as f64
        };
        let trend = compute_trend(&points_vals);

        let cycles_json: Vec<serde_json::Value> = stats
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "number": s.number,
                    "issues_completed": s.issues_completed,
                    "issues_total": s.issues_total,
                    "points_completed": s.points_completed,
                    "points_total": s.points_total,
                    "progress": s.progress,
                    "duration_days": s.duration_days,
                })
            })
            .collect();

        print_json_owned(
            json!({
                "team": team_name,
                "cycles": cycles_json,
                "average_points": avg,
                "trend": trend,
            }),
            output,
        )?;
        return Ok(());
    }

    // Table output
    println!("{}", format!("Sprint Velocity - {}", team_name).bold());
    println!("{}", "-".repeat(50));

    let rows: Vec<VelocityRow> = stats
        .iter()
        .map(|s| {
            let pct = s.progress * 100.0;
            let done_str = format!("{:.0}%", pct);
            let colored_done = if pct >= 100.0 {
                done_str.green().to_string()
            } else if pct >= 80.0 {
                done_str.cyan().to_string()
            } else if pct >= 50.0 {
                done_str.yellow().to_string()
            } else {
                done_str.red().to_string()
            };

            VelocityRow {
                sprint: format!("#{} {}", s.number, s.name),
                issues: format!("{}/{}", s.issues_completed, s.issues_total),
                points: format!("{}/{}", s.points_completed, s.points_total),
                done_pct: colored_done,
                duration: format!("{} days", s.duration_days),
            }
        })
        .collect();

    let table = Table::new(&rows).to_string();
    println!("{}", table);

    // ASCII bar chart
    let max_points = stats
        .iter()
        .map(|s| s.points_completed)
        .max()
        .unwrap_or(1)
        .max(1);

    println!();
    println!("{}", "Points Completed per Sprint".bold());
    for s in &stats {
        let bar_len = (s.points_completed as f64 / max_points as f64 * 30.0).round() as usize;
        let bar = "\u{2588}".repeat(bar_len);
        println!("  #{:<3} {} {}", s.number, bar.green(), s.points_completed);
    }

    // Summary stats
    let points_vals: Vec<u64> = stats.iter().map(|s| s.points_completed).collect();
    let avg = if points_vals.is_empty() {
        0.0
    } else {
        points_vals.iter().sum::<u64>() as f64 / points_vals.len() as f64
    };

    println!();
    println!("{}", "Summary".bold());
    println!("  Average: {:.1} points/sprint", avg);

    if points_vals.len() >= 2 {
        let trend = compute_trend(&points_vals);
        let trend_display = match trend {
            "improving" => format!("{} improving", "\u{2191}").green().to_string(),
            "declining" => format!("{} declining", "\u{2193}").red().to_string(),
            _ => format!("{} stable", "\u{2192}").yellow().to_string(),
        };
        println!("  Trend:   {}", trend_display);
    }

    Ok(())
}

fn compute_trend(values: &[u64]) -> &'static str {
    if values.len() < 2 {
        return "stable";
    }
    let mid = values.len() / 2;
    let first_half = &values[..mid];
    let second_half = &values[mid..];

    let first_avg = if first_half.is_empty() {
        0.0
    } else {
        first_half.iter().sum::<u64>() as f64 / first_half.len() as f64
    };
    let second_avg = if second_half.is_empty() {
        0.0
    } else {
        second_half.iter().sum::<u64>() as f64 / second_half.len() as f64
    };

    let threshold = first_avg * 0.1; // 10% change threshold
    if second_avg > first_avg + threshold {
        "improving"
    } else if second_avg < first_avg - threshold {
        "declining"
    } else {
        "stable"
    }
}

fn parse_duration_days(start: &str, end: &str) -> Option<u64> {
    // Parse YYYY-MM-DD
    let parse = |s: &str| -> Option<(i64, i64, i64)> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 {
            return None;
        }
        let y = parts[0].parse::<i64>().ok()?;
        let m = parts[1].parse::<i64>().ok()?;
        let d = parts[2].parse::<i64>().ok()?;
        Some((y, m, d))
    };

    let (sy, sm, sd) = parse(start)?;
    let (ey, em, ed) = parse(end)?;

    // Convert to approximate days using a simple calculation
    let start_days = sy * 365 + sm * 30 + sd;
    let end_days = ey * 365 + em * 30 + ed;
    Some((end_days - start_days).unsigned_abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_recent_cycles_for_velocity_sorts_and_takes_latest_count() {
        let cycles = vec![
            json!({ "number": 10, "startsAt": "2024-01-15T00:00:00.000Z" }),
            json!({ "number": 30, "startsAt": "2024-03-15T00:00:00.000Z" }),
            json!({ "number": 20, "startsAt": "2024-02-15T00:00:00.000Z" }),
        ];

        let selected = select_recent_cycles_for_velocity(cycles, 2);
        let numbers: Vec<u64> = selected
            .iter()
            .map(|cycle| cycle["number"].as_u64().unwrap())
            .collect();

        assert_eq!(numbers, vec![20, 30]);
    }

    #[test]
    fn test_select_recent_cycles_for_velocity_zero_count_returns_empty() {
        let cycles = vec![json!({ "number": 10, "startsAt": "2024-01-15T00:00:00.000Z" })];

        assert!(select_recent_cycles_for_velocity(cycles, 0).is_empty());
    }
}
