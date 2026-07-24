use anyhow::{Context, Result};
use clap::{Subcommand, ValueHint};
use colored::Colorize;
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};

use crate::api::{
    resolve_label_id, resolve_state_id, resolve_team_id, resolve_user_id, LinearClient,
};
use crate::output::OutputOptions;

#[derive(Subcommand, Debug)]
pub enum ImportCommands {
    /// Import issues from a CSV file
    Csv {
        /// CSV file path
        #[arg(value_hint = ValueHint::FilePath)]
        file: String,
        /// Team key or name (required for new issues)
        #[arg(short, long)]
        team: String,
        /// Preview without creating (dry run)
        #[arg(long)]
        dry_run: bool,
    },
    /// Import issues from a JSON file
    Json {
        /// JSON file path (array of issue objects)
        #[arg(value_hint = ValueHint::FilePath)]
        file: String,
        /// Team key or name
        #[arg(short, long)]
        team: String,
        /// Preview without creating (dry run)
        #[arg(long)]
        dry_run: bool,
    },
}

pub async fn handle(cmd: ImportCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        ImportCommands::Csv {
            file,
            team,
            dry_run,
        } => import_csv(&file, &team, dry_run, output).await,
        ImportCommands::Json {
            file,
            team,
            dry_run,
        } => import_json(&file, &team, dry_run, output).await,
    }
}

/// Represents a parsed row ready for issue creation
struct ImportRow {
    title: String,
    description: Option<String>,
    priority: Option<i64>,
    status: Option<String>,
    assignee: Option<String>,
    labels: Vec<String>,
    estimate: Option<f64>,
    due_date: Option<String>,
}

fn parse_csv_rows(file: &str) -> Result<Vec<ImportRow>> {
    let mut reader = csv::Reader::from_path(file)
        .with_context(|| format!("Failed to open CSV file: {}", file))?;

    let headers = reader.headers()?.clone();

    // Validate that 'title' column exists
    if !headers.iter().any(|h| h.eq_ignore_ascii_case("title")) {
        anyhow::bail!(
            "CSV file must have a 'title' column. Found columns: {}",
            headers.iter().collect::<Vec<_>>().join(", ")
        );
    }

    let mut rows = Vec::new();
    for (i, result) in reader.records().enumerate() {
        let record = result.with_context(|| format!("Failed to parse CSV row {}", i + 1))?;

        let get_field = |name: &str| -> Option<String> {
            headers
                .iter()
                .position(|h| h.eq_ignore_ascii_case(name))
                .and_then(|idx| record.get(idx))
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string())
        };

        let title = match get_field("title") {
            Some(t) => t,
            None => {
                eprintln!(
                    "{}",
                    format!("Warning: Skipping row {} - missing title", i + 1).yellow()
                );
                continue;
            }
        };

        let priority = get_field("priority").and_then(|p| p.parse::<i64>().ok());
        let estimate = get_field("estimate").and_then(|e| e.parse::<f64>().ok());
        let labels = get_field("labels")
            .map(|l| {
                l.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        rows.push(ImportRow {
            title,
            description: get_field("description"),
            priority,
            status: get_field("status"),
            assignee: get_field("assignee"),
            labels,
            estimate,
            due_date: get_field("dueDate").or_else(|| get_field("due_date")),
        });
    }

    Ok(rows)
}

fn parse_json_rows(file: &str) -> Result<Vec<ImportRow>> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("Failed to read JSON file: {}", file))?;
    let data: Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON file: {}", file))?;

    let items = match &data {
        Value::Array(arr) => arr.clone(),
        Value::Object(_) => {
            // Allow a single object wrapped in an array
            vec![data]
        }
        _ => anyhow::bail!("JSON file must contain an array of issue objects or a single object"),
    };

    let mut rows = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let title = match item["title"].as_str() {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => {
                eprintln!(
                    "{}",
                    format!("Warning: Skipping item {} - missing title", i + 1).yellow()
                );
                continue;
            }
        };

        let priority = item["priority"].as_i64();
        let estimate = item["estimate"].as_f64();
        let labels: Vec<String> = if let Some(arr) = item["labels"].as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        } else if let Some(s) = item["labels"].as_str() {
            s.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            Vec::new()
        };

        rows.push(ImportRow {
            title,
            description: item["description"].as_str().map(|s| s.to_string()),
            priority,
            status: item["status"].as_str().map(|s| s.to_string()),
            assignee: item["assignee"].as_str().map(|s| s.to_string()),
            labels,
            estimate,
            due_date: item["dueDate"]
                .as_str()
                .or_else(|| item["due_date"].as_str())
                .map(|s| s.to_string()),
        });
    }

    Ok(rows)
}

async fn create_issues(
    rows: Vec<ImportRow>,
    team: &str,
    dry_run: bool,
    output: &OutputOptions,
) -> Result<()> {
    if rows.is_empty() {
        eprintln!("{}", "No issues to import.".yellow());
        return Ok(());
    }

    let client = LinearClient::new()?;
    let team_id = resolve_team_id(&client, team, &output.cache).await?;

    let total = rows.len();
    eprintln!(
        "{}",
        format!(
            "Importing {} issue{} into team {}...",
            total,
            if total == 1 { "" } else { "s" },
            team
        )
        .bold()
    );

    if dry_run {
        eprintln!(
            "{}",
            "[DRY RUN] Preview of issues to create:".yellow().bold()
        );
        for (i, row) in rows.iter().enumerate() {
            eprintln!();
            eprintln!(
                "  {} {}",
                format!("[{}/{}]", i + 1, total).dimmed(),
                row.title.bold()
            );
            if let Some(ref desc) = row.description {
                let preview = if desc.len() > 80 {
                    format!("{}...", desc.chars().take(80).collect::<String>())
                } else {
                    desc.clone()
                };
                eprintln!("    Description: {}", preview);
            }
            if let Some(p) = row.priority {
                eprintln!("    Priority:    {}", p);
            }
            if let Some(ref s) = row.status {
                eprintln!("    Status:      {}", s);
            }
            if let Some(ref a) = row.assignee {
                eprintln!("    Assignee:    {}", a);
            }
            if !row.labels.is_empty() {
                eprintln!("    Labels:      {}", row.labels.join(", "));
            }
            if let Some(e) = row.estimate {
                eprintln!("    Estimate:    {}", e);
            }
            if let Some(ref d) = row.due_date {
                eprintln!("    Due Date:    {}", d);
            }
        }
        eprintln!();
        eprintln!(
            "{}",
            format!("[DRY RUN] Would create {} issues", total)
                .yellow()
                .bold()
        );
        return Ok(());
    }

    let mutation = r#"
        mutation($input: IssueCreateInput!) {
            issueCreate(input: $input) {
                success
                issue {
                    id
                    identifier
                    title
                    url
                }
            }
        }
    "#;

    // Build all inputs first, resolving names to IDs
    let mut inputs: Vec<(usize, Value, String)> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let mut input = json!({
            "title": row.title,
            "teamId": team_id,
        });

        if let Some(ref desc) = row.description {
            input["description"] = json!(desc);
        }
        if let Some(p) = row.priority {
            input["priority"] = json!(p);
        }
        if let Some(ref s) = row.status {
            match resolve_state_id(&client, &team_id, s).await {
                Ok(state_id) => {
                    input["stateId"] = json!(state_id);
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        format!(
                            "[{}/{}] Warning: Could not resolve status '{}': {}",
                            i + 1,
                            total,
                            s,
                            e
                        )
                        .yellow()
                    );
                }
            }
        }
        if let Some(ref a) = row.assignee {
            match resolve_user_id(&client, a, &output.cache).await {
                Ok(user_id) => {
                    input["assigneeId"] = json!(user_id);
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        format!(
                            "[{}/{}] Warning: Could not resolve assignee '{}': {}",
                            i + 1,
                            total,
                            a,
                            e
                        )
                        .yellow()
                    );
                }
            }
        }
        if !row.labels.is_empty() {
            let mut label_ids = Vec::new();
            for label in &row.labels {
                match resolve_label_id(&client, label, &output.cache).await {
                    Ok(label_id) => {
                        label_ids.push(label_id);
                    }
                    Err(e) => {
                        eprintln!(
                            "{}",
                            format!(
                                "[{}/{}] Warning: Could not resolve label '{}': {}",
                                i + 1,
                                total,
                                label,
                                e
                            )
                            .yellow()
                        );
                    }
                }
            }
            if !label_ids.is_empty() {
                input["labelIds"] = json!(label_ids);
            }
        }
        if let Some(e) = row.estimate {
            input["estimate"] = json!(e);
        }
        if let Some(ref d) = row.due_date {
            if let Some(parsed) = crate::dates::parse_due_date(d) {
                input["dueDate"] = json!(parsed);
            } else {
                input["dueDate"] = json!(d);
            }
        }

        inputs.push((i, input, row.title.clone()));
    }

    // Create issues with bounded concurrency
    type CreateResult = (usize, String, Result<(String, String), String>);
    let client_ref = &client;
    let results: Vec<CreateResult> = stream::iter(inputs)
        .map(|(i, input, title)| async move {
            let result = client_ref
                .mutate(mutation, Some(json!({ "input": input })))
                .await;

            match result {
                Ok(resp) => {
                    if resp["data"]["issueCreate"]["success"].as_bool() == Some(true) {
                        let issue = &resp["data"]["issueCreate"]["issue"];
                        let identifier = issue["identifier"].as_str().unwrap_or("???").to_string();
                        let url = issue["url"].as_str().unwrap_or("").to_string();
                        (i, title, Ok((identifier, url)))
                    } else {
                        let msg = resp["data"]["issueCreate"]
                            .as_object()
                            .and_then(|obj| {
                                obj.get("error")
                                    .or_else(|| obj.get("errors"))
                                    .map(|v| v.to_string())
                            })
                            .unwrap_or_else(|| "Unknown error".to_string());
                        (i, title, Err(msg))
                    }
                }
                Err(e) => (i, title, Err(e.to_string())),
            }
        })
        .buffer_unordered(5)
        .collect()
        .await;

    // Sort by original index for ordered output
    let mut sorted_results = results;
    sorted_results.sort_by_key(|(i, _, _)| *i);

    let mut created = 0usize;
    let mut failed = 0usize;

    for (i, title, result) in &sorted_results {
        match result {
            Ok((identifier, _url)) => {
                created += 1;
                eprintln!(
                    "  {} Created {}: \"{}\"",
                    format!("[{}/{}]", i + 1, total).dimmed(),
                    identifier.green(),
                    title
                );
            }
            Err(err) => {
                failed += 1;
                eprintln!(
                    "  {} {} \"{}\" - {}",
                    format!("[{}/{}]", i + 1, total).dimmed(),
                    "Failed".red(),
                    title,
                    err
                );
            }
        }
    }

    eprintln!();
    if failed == 0 {
        eprintln!(
            "{}",
            format!(
                "Created {} issue{}",
                created,
                if created == 1 { "" } else { "s" }
            )
            .green()
            .bold()
        );
    } else {
        eprintln!(
            "{}",
            format!(
                "Created {} issue{} ({} failed)",
                created,
                if created == 1 { "" } else { "s" },
                failed
            )
            .yellow()
            .bold()
        );
    }

    Ok(())
}

async fn import_csv(file: &str, team: &str, dry_run: bool, output: &OutputOptions) -> Result<()> {
    let rows = parse_csv_rows(file)?;
    create_issues(rows, team, dry_run, output).await
}

async fn import_json(file: &str, team: &str, dry_run: bool, output: &OutputOptions) -> Result<()> {
    let rows = parse_json_rows(file)?;
    create_issues(rows, team, dry_run, output).await
}
