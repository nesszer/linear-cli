use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use futures::stream::{self, StreamExt};
use serde_json::json;
use tabled::{Table, Tabled};

use crate::api::LinearClient;
use crate::display_options;
use crate::error::{CliError, ErrorKind};
use crate::input::read_ids_from_stdin;
use crate::output::{
    ensure_non_empty, filter_values, print_json, print_json_owned, sort_values, OutputOptions,
};
use crate::pagination::paginate_nodes;
use crate::text::truncate;
use crate::types::Comment;

fn safe_terminal_value(value: &str) -> String {
    crate::text::sanitize_terminal_text(value)
}

#[derive(Subcommand)]
pub enum CommentCommands {
    /// List comments for an issue
    #[command(alias = "ls")]
    List {
        /// Issue ID(s). Use "-" to read from stdin.
        issue_ids: Vec<String>,
    },
    /// Create a new comment on an issue
    Create {
        /// Issue ID to comment on
        issue_id: String,
        /// Comment body (Markdown supported)
        #[arg(short, long)]
        body: String,
        /// Parent comment ID to reply to (optional)
        #[arg(short, long)]
        parent_id: Option<String>,
    },
    /// Update an existing comment
    Update {
        /// Comment ID
        id: String,
        /// New comment body (Markdown supported)
        #[arg(short, long)]
        body: String,
    },
    /// Delete a comment
    Delete {
        /// Comment ID
        id: String,
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
}

#[derive(Tabled)]
struct CommentRow {
    #[tabled(rename = "Author")]
    author: String,
    #[tabled(rename = "Created")]
    created_at: String,
    #[tabled(rename = "Body")]
    body: String,
    #[tabled(rename = "ID")]
    id: String,
}

pub async fn handle(cmd: CommentCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        CommentCommands::List { issue_ids } => list_comments(&issue_ids, output).await,
        CommentCommands::Create {
            issue_id,
            body,
            parent_id,
        } => create_comment(&issue_id, &body, parent_id).await,
        CommentCommands::Update { id, body } => update_comment(&id, &body, output).await,
        CommentCommands::Delete { id, force } => delete_comment(&id, force).await,
    }
}

async fn list_comments(issue_ids: &[String], output: &OutputOptions) -> Result<()> {
    let final_ids = read_ids_from_stdin(issue_ids.to_vec());

    if final_ids.is_empty() {
        anyhow::bail!(
            "No issue IDs provided. Provide IDs as arguments or pipe them via stdin.\nExamples:\n  linear comments list LIN-123\n  printf '%s\\n' LIN-123 LIN-456 | linear comments list -"
        );
    }

    let client = LinearClient::new()?;
    let pagination = output.pagination.with_default_limit(100);
    let fetched: Vec<_> = stream::iter(final_ids.iter())
        .map(|id| {
            let client = &client;
            let pagination = &pagination;
            let id = id.clone();
            async move {
                let issue = fetch_issue_meta(client, &id).await;
                match issue {
                    Ok(mut issue_val) if !issue_val.is_null() => {
                        match fetch_issue_comments(client, &id, pagination).await {
                            Ok(comments) => {
                                issue_val["comments"] = json!({ "nodes": comments });
                                Ok((id, issue_val))
                            }
                            Err(e) => Err((id, e)),
                        }
                    }
                    Ok(_) => Err((
                        id.clone(),
                        CliError::not_found(format!("Issue not found: {id}")).into(),
                    )),
                    Err(e) => Err((id, e)),
                }
            }
        })
        .buffer_unordered(10)
        .collect()
        .await;

    let mut issues = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    // Real fetch errors keep their kind; missing issues map to NotFound.
    let mut first_error: Option<anyhow::Error> = None;
    for result in fetched {
        match result {
            Ok((_id, issue)) => issues.push(issue),
            Err((id, e)) => {
                let is_missing = e
                    .downcast_ref::<CliError>()
                    .map(|c| c.kind == ErrorKind::NotFound)
                    .unwrap_or(false);
                if !output.is_json() && !output.has_template() {
                    if is_missing {
                        eprintln!("{} Issue not found: {}", "!".yellow(), id);
                    } else {
                        eprintln!("{} Error fetching {}: {}", "!".red(), id, e);
                    }
                }
                if is_missing {
                    missing.push(id);
                } else if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
    }

    // JSON output - return raw data for LLM consumption
    if output.is_json() || output.has_template() {
        if issues.len() == 1 {
            print_json(&issues[0], output)?;
        } else {
            print_json_owned(serde_json::json!(issues), output)?;
        }
        return comments_status(first_error, &missing);
    }

    for (idx, issue) in issues.iter().enumerate() {
        if idx > 0 {
            println!();
        }
        let identifier = safe_terminal_value(issue["identifier"].as_str().unwrap_or(""));
        let title = safe_terminal_value(issue["title"].as_str().unwrap_or(""));

        println!("{} {}", identifier.bold(), title);
        println!("{}", "-".repeat(50));

        let mut comments = issue["comments"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        filter_values(&mut comments, &output.filters);

        if let Some(sort_key) = output.json.sort.as_deref() {
            sort_values(&mut comments, sort_key, output.json.order);
        }

        ensure_non_empty(&comments, output)?;
        if comments.is_empty() {
            println!("No comments found for this issue.");
            continue;
        }

        let width = display_options().max_width(60);
        let rows: Vec<CommentRow> = comments
            .iter()
            .filter_map(|v| serde_json::from_value::<Comment>(v.clone()).ok())
            .map(|c| {
                let body_text = c.body.as_deref().unwrap_or("");
                let truncated_body = truncate(body_text, width);

                let created_at = c
                    .created_at
                    .as_deref()
                    .unwrap_or("")
                    .split('T')
                    .next()
                    .unwrap_or("-")
                    .to_string();

                CommentRow {
                    author: c
                        .user
                        .as_ref()
                        .map(|u| safe_terminal_value(&u.name))
                        .unwrap_or_else(|| "Unknown".to_string()),
                    created_at,
                    body: truncated_body.replace('\n', " "),
                    id: c.id,
                }
            })
            .collect();

        let table = Table::new(rows).to_string();
        println!("{}", table);
        println!("\n{} comments", comments.len());
    }

    comments_status(first_error, &missing)
}

/// Aggregate exit status: a real fetch error (passed through unchanged) wins so
/// its kind survives; otherwise missing issues map to NotFound.
fn comments_status(first_error: Option<anyhow::Error>, missing: &[String]) -> Result<()> {
    if let Some(e) = first_error {
        return Err(e);
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CliError::not_found(format!("Issue(s) not found: {}", missing.join(", "))).into())
    }
}

async fn fetch_issue_meta(client: &LinearClient, issue_id: &str) -> Result<serde_json::Value> {
    let query = r#"
        query($issueId: String!) {
            issue(id: $issueId) {
                id
                identifier
                title
            }
        }
    "#;

    let result = client
        .query(query, Some(json!({ "issueId": issue_id })))
        .await?;
    Ok(result["data"]["issue"].clone())
}

async fn fetch_issue_comments(
    client: &LinearClient,
    issue_id: &str,
    pagination: &crate::pagination::PaginationOptions,
) -> Result<Vec<serde_json::Value>> {
    let query = r#"
        query($issueId: String!, $first: Int, $after: String, $last: Int, $before: String) {
            issue(id: $issueId) {
                comments(first: $first, after: $after, last: $last, before: $before) {
                    nodes {
                        id
                        body
                        createdAt
                        user { name email }
                        parent { id }
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
    vars.insert("issueId".to_string(), json!(issue_id));

    paginate_nodes(
        client,
        query,
        vars,
        &["data", "issue", "comments", "nodes"],
        &["data", "issue", "comments", "pageInfo"],
        pagination,
        100,
    )
    .await
}

async fn create_comment(issue_id: &str, body: &str, parent_id: Option<String>) -> Result<()> {
    let client = LinearClient::new()?;

    let mut input = json!({
        "issueId": issue_id,
        "body": body
    });

    if let Some(pid) = parent_id {
        input["parentId"] = json!(pid);
    }

    let mutation = r#"
        mutation($input: CommentCreateInput!) {
            commentCreate(input: $input) {
                success
                comment {
                    id
                    body
                    createdAt
                    user { name }
                    issue { identifier title }
                }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "input": input })))
        .await?;

    if result["data"]["commentCreate"]["success"].as_bool() == Some(true) {
        let comment = &result["data"]["commentCreate"]["comment"];
        let issue_identifier =
            safe_terminal_value(comment["issue"]["identifier"].as_str().unwrap_or(""));
        let issue_title = safe_terminal_value(comment["issue"]["title"].as_str().unwrap_or(""));

        println!(
            "{} Comment added to {} {}",
            "+".green(),
            issue_identifier,
            issue_title
        );
        println!("  ID: {}", comment["id"].as_str().unwrap_or(""));
        println!(
            "  Author: {}",
            safe_terminal_value(comment["user"]["name"].as_str().unwrap_or(""))
        );

        let body_preview = truncate(comment["body"].as_str().unwrap_or(""), Some(80));
        if !body_preview.is_empty() {
            println!("  Body: {}", body_preview.dimmed());
        }
    } else {
        anyhow::bail!("Failed to create comment");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_terminal_value_removes_escape_sequences() {
        assert_eq!(
            safe_terminal_value("bad\u{1b}]52;c;ZXZpbA==\u{7}title"),
            "badtitle"
        );
    }

    #[test]
    fn comments_status_ok_when_nothing_failed() {
        assert!(comments_status(None, &[]).is_ok());
    }

    #[test]
    fn comments_status_notfound_when_only_missing() {
        let err = comments_status(None, &["LIN-9".to_string()]).unwrap_err();
        assert_eq!(err.downcast_ref::<CliError>().expect("CliError").code(), 2);
    }

    #[test]
    fn comments_status_preserves_real_error_over_missing() {
        // A real fetch error keeps its kind (rate-limited => 4), not NotFound(2).
        let err = comments_status(
            Some(
                CliError::rate_limited("429")
                    .with_retry_after(Some(5))
                    .into(),
            ),
            &["LIN-9".to_string()],
        )
        .unwrap_err();
        let cli = err.downcast_ref::<CliError>().expect("CliError");
        assert_eq!(cli.code(), 4);
        assert_eq!(cli.retry_after, Some(5));
    }
}

async fn update_comment(id: &str, body: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    let input = json!({ "body": body });

    let mutation = r#"
        mutation($id: String!, $input: CommentUpdateInput!) {
            commentUpdate(id: $id, input: $input) {
                success
                comment {
                    id
                    body
                    updatedAt
                    user { name }
                }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if result["data"]["commentUpdate"]["success"].as_bool() == Some(true) {
        let comment = &result["data"]["commentUpdate"]["comment"];

        if output.is_json() || output.has_template() {
            print_json(comment, output)?;
            return Ok(());
        }

        println!("{} Comment updated", "+".green());
        println!("  ID: {}", comment["id"].as_str().unwrap_or(""));
    } else {
        anyhow::bail!("Failed to update comment");
    }

    Ok(())
}

async fn delete_comment(id: &str, force: bool) -> Result<()> {
    if !force && !crate::is_yes() {
        anyhow::bail!(
            "Delete requires --force flag. Use: linear comments delete {} --force",
            id
        );
    }

    let client = LinearClient::new()?;

    let mutation = r#"
        mutation($id: String!) {
            commentDelete(id: $id) {
                success
            }
        }
    "#;

    let result = client.mutate(mutation, Some(json!({ "id": id }))).await?;

    if result["data"]["commentDelete"]["success"].as_bool() == Some(true) {
        println!("{} Comment deleted", "+".green());
    } else {
        anyhow::bail!("Failed to delete comment");
    }

    Ok(())
}
