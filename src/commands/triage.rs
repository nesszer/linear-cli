use anyhow::Result;
use clap::Subcommand;
use serde_json::json;
use tabled::{Table, Tabled};

use crate::api::LinearClient;
use crate::output::{print_json, OutputOptions};
use crate::text::truncate;
use crate::DISPLAY_OPTIONS;

#[derive(Subcommand, Debug)]
pub enum TriageCommands {
    /// List triage issues (unassigned, no project)
    List {
        /// Team key or ID
        #[arg(short, long)]
        team: Option<String>,
    },
    /// Assign issue to self and move to backlog
    Claim {
        /// Issue identifier (e.g., LIN-123)
        id: String,
    },
    /// Snooze issue for later
    Snooze {
        /// Issue identifier
        id: String,
        /// Snooze duration (e.g., 1d, 1w)
        #[arg(short, long, default_value = "1d")]
        duration: String,
    },
}

#[derive(Tabled)]
struct TriageRow {
    #[tabled(rename = "ID")]
    identifier: String,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Created")]
    created: String,
    #[tabled(rename = "Team")]
    team: String,
}

pub async fn handle(cmd: TriageCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        TriageCommands::List { team } => {
            list_triage(crate::config::resolve_team_arg(team), output).await
        }
        TriageCommands::Claim { id } => claim_issue(&id, output).await,
        TriageCommands::Snooze { id, duration } => snooze_issue(&id, &duration, output).await,
    }
}

async fn list_triage(team: Option<String>, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    let query = r#"
        query($filter: IssueFilter) {
            issues(first: 50, filter: $filter) {
                nodes {
                    id
                    identifier
                    title
                    createdAt
                    team {
                        key
                        name
                    }
                    state {
                        name
                        type
                    }
                }
            }
        }
    "#;

    // Filter for triage: no assignee, state is "triage" type or backlog
    let mut filter = json!({
        "assignee": { "null": true },
        "state": { "type": { "in": ["triage", "backlog"] } }
    });

    if let Some(ref t) = team {
        filter["team"] = json!({ "key": { "eq": t } });
    }

    let result = client
        .query(query, Some(json!({ "filter": filter })))
        .await?;
    let issues = &result["data"]["issues"]["nodes"];

    if output.is_json() {
        print_json(issues, output)?;
    } else {
        let display = DISPLAY_OPTIONS.get().cloned().unwrap_or_default();
        let max_width = display.max_width(50);

        let rows: Vec<TriageRow> = issues
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|i| TriageRow {
                identifier: i["identifier"].as_str().unwrap_or("-").to_string(),
                title: truncate(i["title"].as_str().unwrap_or("-"), max_width),
                created: i["createdAt"]
                    .as_str()
                    .unwrap_or("-")
                    .chars()
                    .take(10)
                    .collect(),
                team: i["team"]["key"].as_str().unwrap_or("-").to_string(),
            })
            .collect();

        if rows.is_empty() {
            println!("No triage issues found - inbox zero!");
        } else {
            println!("{}", Table::new(rows));
        }
    }

    Ok(())
}

async fn claim_issue(id: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    // Get current user and issue details (including team states to find backlog)
    let me_query = r#"query { viewer { id } }"#;
    let me_result = client.query(me_query, None).await?;
    let my_id = me_result["data"]["viewer"]["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Could not get current user"))?;

    // Fetch the issue's team states to find a backlog state
    let issue_query = r#"
        query($id: String!) {
            issue(id: $id) {
                team {
                    states {
                        nodes {
                            id
                            type
                        }
                    }
                }
            }
        }
    "#;

    let issue_result = client.query(issue_query, Some(json!({ "id": id }))).await?;
    let issue_data = &issue_result["data"]["issue"];

    if issue_data.is_null() {
        anyhow::bail!("Issue not found: {}", id);
    }

    // Find a backlog state, fall back to unstarted
    let empty = vec![];
    let states = issue_data["team"]["states"]["nodes"]
        .as_array()
        .unwrap_or(&empty);

    let backlog_state = states
        .iter()
        .find(|s| s["type"].as_str() == Some("backlog"))
        .or_else(|| {
            states
                .iter()
                .find(|s| s["type"].as_str() == Some("unstarted"))
        });

    // Build mutation input with assignee and optional state change
    let mut input = json!({ "assigneeId": my_id });
    if let Some(state) = backlog_state {
        if let Some(state_id) = state["id"].as_str() {
            input["stateId"] = json!(state_id);
        }
    }

    // Update issue
    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue {
                    id
                    identifier
                    title
                    assignee { name }
                    state { name }
                }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if output.is_json() {
        print_json(&result["data"]["issueUpdate"], output)?;
    } else {
        let issue = &result["data"]["issueUpdate"]["issue"];
        let state_name = issue["state"]["name"].as_str().unwrap_or("backlog");
        println!(
            "Claimed {} - {} (moved to {})",
            issue["identifier"].as_str().unwrap_or(id),
            issue["title"].as_str().unwrap_or(""),
            state_name
        );
    }

    Ok(())
}

async fn snooze_issue(id: &str, duration: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    // Parse duration to calculate snooze until date
    let days = match duration {
        "1d" => 1,
        "2d" => 2,
        "3d" => 3,
        "1w" => 7,
        "2w" => 14,
        _ => duration.trim_end_matches('d').parse::<i64>().unwrap_or(1),
    };

    let snooze_until = chrono::Utc::now() + chrono::Duration::days(days);

    let mutation = r#"
        mutation($id: String!, $snoozedUntilAt: DateTime!) {
            issueUpdate(id: $id, input: { snoozedUntilAt: $snoozedUntilAt }) {
                success
                issue {
                    id
                    identifier
                    snoozedUntilAt
                }
            }
        }
    "#;

    let result = client
        .mutate(
            mutation,
            Some(json!({ "id": id, "snoozedUntilAt": snooze_until.to_rfc3339() })),
        )
        .await?;

    if output.is_json() {
        print_json(&result["data"]["issueUpdate"], output)?;
    } else {
        println!("Snoozed {} for {}", id, duration);
    }

    Ok(())
}
