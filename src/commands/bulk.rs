use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use futures::stream::{self, StreamExt};
use serde_json::json;
use tokio::sync::Mutex;

use crate::api::{resolve_label_id, resolve_state_id, resolve_user_id, LinearClient};
use crate::display_options;
use crate::error::{self, CliError};
use crate::output::{print_json_owned, OutputOptions};
use crate::text::truncate;

#[derive(Subcommand)]
pub enum BulkCommands {
    /// Update the state of multiple issues
    #[command(alias = "state")]
    #[command(after_help = r#"EXAMPLES:
    linear bulk update-state Done -i LIN-1,LIN-2,LIN-3
    linear b state "In Progress" -i LIN-1,LIN-2"#)]
    UpdateState {
        /// The new state name or ID
        state: String,
        /// Comma-separated list of issue IDs (e.g., "LIN-1,LIN-2,LIN-3")
        #[arg(short, long, value_delimiter = ',')]
        issues: Vec<String>,
    },
    /// Assign multiple issues to a user
    #[command(after_help = r#"EXAMPLES:
    linear bulk assign me -i LIN-1,LIN-2,LIN-3
    linear b assign john@example.com -i LIN-1,LIN-2"#)]
    Assign {
        /// The user to assign (user ID, name, email, or "me")
        user: String,
        /// Comma-separated list of issue IDs (e.g., "LIN-1,LIN-2,LIN-3")
        #[arg(short, long, value_delimiter = ',')]
        issues: Vec<String>,
    },
    /// Add a label to multiple issues
    #[command(after_help = r#"EXAMPLES:
    linear bulk label "Bug" -i LIN-1,LIN-2,LIN-3
    linear b label LABEL_ID -i LIN-1,LIN-2"#)]
    Label {
        /// The label name or ID to add
        label: String,
        /// Comma-separated list of issue IDs (e.g., "LIN-1,LIN-2,LIN-3")
        #[arg(short, long, value_delimiter = ',')]
        issues: Vec<String>,
    },
    /// Unassign multiple issues
    #[command(after_help = r#"EXAMPLES:
    linear bulk unassign -i LIN-1,LIN-2,LIN-3"#)]
    Unassign {
        /// Comma-separated list of issue IDs (e.g., "LIN-1,LIN-2,LIN-3")
        #[arg(short, long, value_delimiter = ',')]
        issues: Vec<String>,
    },
}

/// Result of a single bulk operation
#[derive(Debug)]
struct BulkResult {
    issue_id: String,
    success: bool,
    identifier: Option<String>,
    error: Option<String>,
}

/// Get issue details including UUID and team ID from identifier (e.g., "LIN-123")
async fn get_issue_info(
    client: &LinearClient,
    issue_id: &str,
) -> Result<(String, String, Option<String>)> {
    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                team {
                    id
                }
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": issue_id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", issue_id);
    }

    let uuid = issue["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to get issue ID"))?
        .to_string();

    let team_id = issue["team"]["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to get team ID"))?
        .to_string();

    let identifier = issue["identifier"].as_str().map(|s| s.to_string());

    Ok((uuid, team_id, identifier))
}

pub async fn handle(cmd: BulkCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        BulkCommands::UpdateState { state, issues } => {
            bulk_update_state(&state, issues, output).await
        }
        BulkCommands::Assign { user, issues } => bulk_assign(&user, issues, output).await,
        BulkCommands::Label { label, issues } => bulk_label(&label, issues, output).await,
        BulkCommands::Unassign { issues } => bulk_unassign(issues, output).await,
    }
}

async fn bulk_update_state(state: &str, issues: Vec<String>, output: &OutputOptions) -> Result<()> {
    if issues.is_empty() {
        return missing_bulk_issues("linear bulk update-state Done -i LIN-1,LIN-2");
    }

    if !output.is_json() && !output.has_template() {
        println!(
            "{} Updating state to '{}' for {} issues...",
            ">>".cyan(),
            state,
            issues.len()
        );
    }

    let client = LinearClient::new()?;
    let state_owned = state.to_string();
    let state_cache: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

    let results: Vec<_> = stream::iter(issues.iter())
        .map(|issue_id| {
            let client = &client;
            let state = &state_owned;
            let cache = Arc::clone(&state_cache);
            let id = issue_id.clone();
            async move { update_issue_state(client, &id, state, &cache).await }
        })
        .buffer_unordered(10)
        .collect()
        .await;
    print_summary(&results, "state updated", output);
    bulk_exit_status(&results)
}

async fn bulk_assign(user: &str, issues: Vec<String>, output: &OutputOptions) -> Result<()> {
    if issues.is_empty() {
        return missing_bulk_issues("linear bulk assign me -i LIN-1,LIN-2");
    }

    if !output.is_json() && !output.has_template() {
        println!(
            "{} Assigning {} issues to '{}'...",
            ">>".cyan(),
            issues.len(),
            user
        );
    }

    let client = LinearClient::new()?;

    // Resolution failure is a hard error: return Err, preserving the kind.
    let user_id = resolve_user_id(&client, user, &output.cache)
        .await
        .map_err(|e| {
            error::rewrap_with_message(&e, format!("Failed to resolve user '{user}': {e}"))
        })?;

    let results: Vec<_> = stream::iter(issues.iter())
        .map(|issue_id| {
            let client = &client;
            let user_id = &user_id;
            let id = issue_id.clone();
            async move { update_issue_assignee(client, &id, Some(user_id)).await }
        })
        .buffer_unordered(10)
        .collect()
        .await;
    print_summary(&results, "assigned", output);
    bulk_exit_status(&results)
}

async fn bulk_label(label: &str, issues: Vec<String>, output: &OutputOptions) -> Result<()> {
    if issues.is_empty() {
        return missing_bulk_issues("linear bulk label bug -i LIN-1,LIN-2");
    }

    if !output.is_json() && !output.has_template() {
        println!(
            "{} Adding label '{}' to {} issues...",
            ">>".cyan(),
            label,
            issues.len()
        );
    }

    let client = LinearClient::new()?;

    // Resolution failure is a hard error: return Err, preserving the kind.
    let label_id = resolve_label_id(&client, label, &output.cache)
        .await
        .map_err(|e| {
            error::rewrap_with_message(&e, format!("Failed to resolve label '{label}': {e}"))
        })?;

    let results: Vec<_> = stream::iter(issues.iter())
        .map(|issue_id| {
            let client = &client;
            let label_id = &label_id;
            let id = issue_id.clone();
            async move { add_label_to_issue(client, &id, label_id).await }
        })
        .buffer_unordered(10)
        .collect()
        .await;
    print_summary(&results, "labeled", output);
    bulk_exit_status(&results)
}

async fn bulk_unassign(issues: Vec<String>, output: &OutputOptions) -> Result<()> {
    if issues.is_empty() {
        return missing_bulk_issues("linear bulk unassign -i LIN-1,LIN-2");
    }

    if !output.is_json() && !output.has_template() {
        println!("{} Unassigning {} issues...", ">>".cyan(), issues.len());
    }

    let client = LinearClient::new()?;

    let results: Vec<_> = stream::iter(issues.iter())
        .map(|issue_id| {
            let client = &client;
            let id = issue_id.clone();
            async move { update_issue_assignee(client, &id, None).await }
        })
        .buffer_unordered(10)
        .collect()
        .await;
    print_summary(&results, "unassigned", output);
    bulk_exit_status(&results)
}

fn missing_bulk_issues(example: &str) -> Result<()> {
    anyhow::bail!(
        "No issues specified. Provide issue IDs with -i/--issues.\nExample: {}",
        example
    );
}

async fn update_issue_state(
    client: &LinearClient,
    issue_id: &str,
    state: &str,
    state_cache: &Arc<Mutex<HashMap<String, String>>>,
) -> BulkResult {
    // First, get issue UUID and team ID
    let (uuid, team_id, identifier) = match get_issue_info(client, issue_id).await {
        Ok(info) => info,
        Err(e) => {
            return BulkResult {
                issue_id: issue_id.to_string(),
                success: false,
                identifier: None,
                error: Some(e.to_string()),
            };
        }
    };

    // Check cache for resolved state ID for this team
    let cache_key = format!("{}:{}", team_id, state);
    let cached = {
        let cache = state_cache.lock().await;
        cache.get(&cache_key).cloned()
    };

    let state_id = match cached {
        Some(id) => id,
        None => {
            // Resolve state name to UUID for this team
            match resolve_state_id(client, &team_id, state).await {
                Ok(id) => {
                    let mut cache = state_cache.lock().await;
                    cache.insert(cache_key, id.clone());
                    id
                }
                Err(e) => {
                    return BulkResult {
                        issue_id: issue_id.to_string(),
                        success: false,
                        identifier,
                        error: Some(e.to_string()),
                    };
                }
            }
        }
    };

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue {
                    identifier
                    title
                }
            }
        }
    "#;

    let input = json!({ "stateId": state_id });

    match client
        .mutate(mutation, Some(json!({ "id": uuid, "input": input })))
        .await
    {
        Ok(result) => {
            if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
                let identifier = result["data"]["issueUpdate"]["issue"]["identifier"]
                    .as_str()
                    .map(|s| s.to_string());
                BulkResult {
                    issue_id: issue_id.to_string(),
                    success: true,
                    identifier,
                    error: None,
                }
            } else {
                BulkResult {
                    issue_id: issue_id.to_string(),
                    success: false,
                    identifier: None,
                    error: Some("Update failed".to_string()),
                }
            }
        }
        Err(e) => BulkResult {
            issue_id: issue_id.to_string(),
            success: false,
            identifier: None,
            error: Some(e.to_string()),
        },
    }
}

async fn update_issue_assignee(
    client: &LinearClient,
    issue_id: &str,
    assignee_id: Option<&str>,
) -> BulkResult {
    // First, get issue UUID
    let (uuid, _team_id, identifier) = match get_issue_info(client, issue_id).await {
        Ok(info) => info,
        Err(e) => {
            return BulkResult {
                issue_id: issue_id.to_string(),
                success: false,
                identifier: None,
                error: Some(e.to_string()),
            };
        }
    };

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue {
                    identifier
                    title
                }
            }
        }
    "#;

    let input = match assignee_id {
        Some(id) => json!({ "assigneeId": id }),
        None => json!({ "assigneeId": null }),
    };

    match client
        .mutate(mutation, Some(json!({ "id": uuid, "input": input })))
        .await
    {
        Ok(result) => {
            if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
                let identifier = result["data"]["issueUpdate"]["issue"]["identifier"]
                    .as_str()
                    .map(|s| s.to_string())
                    .or(identifier);
                BulkResult {
                    issue_id: issue_id.to_string(),
                    success: true,
                    identifier,
                    error: None,
                }
            } else {
                BulkResult {
                    issue_id: issue_id.to_string(),
                    success: false,
                    identifier,
                    error: Some("Update failed".to_string()),
                }
            }
        }
        Err(e) => BulkResult {
            issue_id: issue_id.to_string(),
            success: false,
            identifier,
            error: Some(e.to_string()),
        },
    }
}

async fn add_label_to_issue(client: &LinearClient, issue_id: &str, label_id: &str) -> BulkResult {
    // First, get existing labels for the issue (using the issue identifier/UUID)
    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                labels {
                    nodes {
                        id
                    }
                }
            }
        }
    "#;

    let (uuid, identifier, existing_label_ids) =
        match client.query(query, Some(json!({ "id": issue_id }))).await {
            Ok(result) => {
                if result["data"]["issue"].is_null() {
                    return BulkResult {
                        issue_id: issue_id.to_string(),
                        success: false,
                        identifier: None,
                        error: Some("Issue not found".to_string()),
                    };
                }

                let uuid = result["data"]["issue"]["id"]
                    .as_str()
                    .unwrap_or(issue_id)
                    .to_string();

                let identifier = result["data"]["issue"]["identifier"]
                    .as_str()
                    .map(|s| s.to_string());

                let labels: Vec<String> = result["data"]["issue"]["labels"]["nodes"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|l| l["id"].as_str().map(|s| s.to_string()))
                    .collect();

                (uuid, identifier, labels)
            }
            Err(e) => {
                return BulkResult {
                    issue_id: issue_id.to_string(),
                    success: false,
                    identifier: None,
                    error: Some(e.to_string()),
                };
            }
        };

    let mut label_ids = existing_label_ids;

    // Add the new label if not already present
    if !label_ids.contains(&label_id.to_string()) {
        label_ids.push(label_id.to_string());
    }

    // Update the issue with the new label list
    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue {
                    identifier
                }
            }
        }
    "#;

    let input = json!({ "labelIds": label_ids });

    match client
        .mutate(mutation, Some(json!({ "id": uuid, "input": input })))
        .await
    {
        Ok(result) => {
            if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
                let identifier = result["data"]["issueUpdate"]["issue"]["identifier"]
                    .as_str()
                    .map(|s| s.to_string())
                    .or(identifier);
                BulkResult {
                    issue_id: issue_id.to_string(),
                    success: true,
                    identifier,
                    error: None,
                }
            } else {
                BulkResult {
                    issue_id: issue_id.to_string(),
                    success: false,
                    identifier,
                    error: Some("Update failed".to_string()),
                }
            }
        }
        Err(e) => BulkResult {
            issue_id: issue_id.to_string(),
            success: false,
            identifier,
            error: Some(e.to_string()),
        },
    }
}

fn print_summary(results: &[BulkResult], action: &str, output: &OutputOptions) {
    let success_count = results.iter().filter(|r| r.success).count();
    let failure_count = results.len() - success_count;
    let id_width = display_options().max_width(30);
    let err_width = display_options().max_width(60);

    if output.is_json() || output.has_template() {
        let json_results: Vec<_> = results
            .iter()
            .map(|r| {
                json!({
                    "issue_id": r.issue_id,
                    "identifier": r.identifier,
                    "success": r.success,
                    "error": r.error,
                })
            })
            .collect();

        let payload = json!({
            "action": action,
            "results": json_results,
            "summary": {
                "total": results.len(),
                "succeeded": success_count,
                "failed": failure_count,
            }
        });
        if let Err(err) = print_json_owned(payload, output) {
            eprintln!("Error: {}", err);
        }
        return;
    }

    println!();

    // Print individual results
    for result in results {
        if result.success {
            let display_id = result.identifier.as_deref().unwrap_or(&result.issue_id);
            let display_id = truncate(display_id, id_width);
            println!("  {} {} {}", "+".green(), display_id.cyan(), action);
        } else {
            let error_msg = result.error.as_deref().unwrap_or("Unknown error");
            let error_msg = truncate(error_msg, err_width);
            println!(
                "  {} {} failed: {}",
                "x".red(),
                result.issue_id.cyan(),
                error_msg.dimmed()
            );
        }
    }

    // Print summary
    println!();
    println!(
        "{} Summary: {} succeeded, {} failed",
        ">>".cyan(),
        success_count.to_string().green(),
        if failure_count > 0 {
            failure_count.to_string().red().to_string()
        } else {
            failure_count.to_string()
        }
    );
}

/// `Err` if any item failed, so partial failures still exit non-zero.
fn bulk_exit_status(results: &[BulkResult]) -> Result<()> {
    let failed = results.iter().filter(|r| !r.success).count();
    if failed > 0 {
        return Err(CliError::general(format!(
            "{} of {} operations failed",
            failed,
            results.len()
        ))
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(success: bool) -> BulkResult {
        BulkResult {
            issue_id: "LIN-1".to_string(),
            success,
            identifier: None,
            error: if success {
                None
            } else {
                Some("boom".to_string())
            },
        }
    }

    #[test]
    fn bulk_exit_status_ok_when_all_succeed_or_empty() {
        assert!(bulk_exit_status(&[]).is_ok());
        assert!(bulk_exit_status(&[result(true), result(true)]).is_ok());
    }

    #[test]
    fn bulk_exit_status_errs_on_any_failure() {
        let err = bulk_exit_status(&[result(true), result(false)]).unwrap_err();
        assert_eq!(err.downcast_ref::<CliError>().expect("CliError").code(), 1);
    }

    #[test]
    fn bulk_exit_status_errs_when_all_fail() {
        assert!(bulk_exit_status(&[result(false), result(false)]).is_err());
    }
}
