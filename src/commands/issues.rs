use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use serde_json::{json, Map, Value};
use std::io::{self, BufRead};
use tabled::{Table, Tabled};

use crate::api::{
    resolve_label_id, resolve_project_id, resolve_state_id, resolve_team_id, resolve_user_id,
    LinearClient,
};
use crate::cache::CacheOptions;
use crate::display_options;
use crate::error::{self, CliError};
use crate::input::read_ids_from_stdin;
use crate::output::{
    ensure_non_empty, filter_values, print_json, print_json_owned, sort_values, OutputOptions,
};
use crate::pagination::{paginate_nodes, stream_nodes};
use crate::priority::priority_to_string;
use crate::text::{is_uuid, truncate};
use crate::vcs::{generate_branch_name, git_branch_exists, run_git_command, validate_branch_name};
use crate::AgentOptions;

use super::templates;

fn safe_terminal_value(value: &str) -> String {
    crate::text::sanitize_terminal_text(value)
}

#[derive(Subcommand)]
pub enum IssueCommands {
    /// List issues
    #[command(alias = "ls")]
    #[command(after_help = r#"EXAMPLES:
    linear issues list                         # List all issues
    linear i list -t ENG                       # Filter by team
    linear i list -t ENG -s "In Progress"      # Filter by team and status
    linear i list --assignee me                # Show my assigned issues
    linear i list --project "My Project"       # Filter by project name
    linear i list --output json                # Output as JSON"#)]
    List {
        /// Filter by team name or ID
        #[arg(short, long)]
        team: Option<String>,
        /// Filter by state name or ID
        #[arg(short, long)]
        state: Option<String>,
        /// Filter by assignee (user ID, name, email, or "me")
        #[arg(short, long)]
        assignee: Option<String>,
        /// Show only my assigned issues (shortcut for --assignee me)
        #[arg(long)]
        mine: bool,
        /// Filter by project name
        #[arg(long)]
        project: Option<String>,
        /// Filter by label name
        #[arg(short, long)]
        label: Option<String>,
        /// Apply a saved custom view's filters
        #[arg(long)]
        view: Option<String>,
        /// Only show issues created after this date (today, -7d, 2024-01-15, etc.)
        #[arg(long, alias = "newer-than")]
        since: Option<String>,
        /// Include archived issues
        #[arg(long)]
        archived: bool,
        /// Group output by field (state, priority, assignee)
        #[arg(long)]
        group_by: Option<String>,
        /// Show only the count of matching issues
        #[arg(long)]
        count_only: bool,
    },
    /// Get issue details
    #[command(after_help = r#"EXAMPLES:
    linear issues get LIN-123                  # View issue by identifier
    linear i get abc123-uuid                   # View issue by ID
    linear i get LIN-1 LIN-2 LIN-3             # Get multiple issues
    linear i get LIN-123 --output json         # Output as JSON
    echo "LIN-123" | linear i get -            # Read ID from stdin (piping)"#)]
    Get {
        /// Issue ID(s) or identifier(s). Use "-" to read from stdin.
        ids: Vec<String>,

        /// Show recent activity history
        #[arg(long)]
        history: bool,

        /// Show comments
        #[arg(long)]
        comments: bool,
    },
    /// Open issue in browser
    Open {
        /// Issue ID or identifier
        id: String,
    },
    /// Create a new issue
    #[command(after_help = r#"EXAMPLES:
    linear issues create "Fix bug" -t ENG      # Create with title and team
    linear i create "Feature" -t ENG -p 2      # Create with high priority
    linear i create "Task" -t ENG -a me        # Assign to yourself
    linear i create "Task" -t ENG --due +3d    # Due in 3 days
    linear i create "Bug" -t ENG --dry-run     # Preview without creating"#)]
    Create {
        /// Issue title
        title: String,
        /// Team name or ID (can be provided via template)
        #[arg(short, long)]
        team: Option<String>,
        /// Issue description (markdown). Use "-" to read from stdin.
        #[arg(short, long)]
        description: Option<String>,
        /// JSON input for issue fields. Use "-" to read from stdin.
        #[arg(long)]
        data: Option<String>,
        /// Priority (0=none, 1=urgent, 2=high, 3=normal, 4=low)
        #[arg(short, long)]
        priority: Option<i32>,
        /// State name or ID
        #[arg(short, long)]
        state: Option<String>,
        /// Assignee (user ID, name, email, or "me")
        #[arg(short, long)]
        assignee: Option<String>,
        /// Labels to add (can be specified multiple times)
        #[arg(short, long)]
        labels: Vec<String>,
        /// Due date (today, tomorrow, +3d, +1w, or YYYY-MM-DD)
        #[arg(long)]
        due: Option<String>,
        /// Estimate in points (e.g., 1, 2, 3, 5, 8)
        #[arg(short, long)]
        estimate: Option<f64>,
        /// Template name to use for default values
        #[arg(long)]
        template: Option<String>,
        /// Preview without creating (dry run)
        #[arg(long)]
        dry_run: bool,
    },
    /// Update an existing issue
    #[command(after_help = r#"EXAMPLES:
    linear issues update LIN-123 -s Done       # Mark as done
    linear i update LIN-123 -T "New title"     # Change title
    linear i update LIN-123 -p 1               # Set to urgent priority
    linear i update LIN-123 --due tomorrow     # Due tomorrow
    linear i update LIN-123 -a me              # Assign to yourself
    linear i update LIN-123 -l bug -l urgent   # Add labels
    linear i update LIN-123 --project MyProj   # Move to project"#)]
    Update {
        /// Issue ID
        id: String,
        /// New title
        #[arg(short = 'T', long)]
        title: Option<String>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
        /// JSON input for issue fields. Use "-" to read from stdin.
        #[arg(long)]
        data: Option<String>,
        /// New priority (0=none, 1=urgent, 2=high, 3=normal, 4=low)
        #[arg(short, long)]
        priority: Option<i32>,
        /// New state name or ID
        #[arg(short, long)]
        state: Option<String>,
        /// New assignee (user ID, name, email, or "me")
        #[arg(short, long)]
        assignee: Option<String>,
        /// Labels to set (can be specified multiple times)
        #[arg(short, long)]
        labels: Vec<String>,
        /// Due date (today, tomorrow, +3d, +1w, YYYY-MM-DD, or "none" to clear)
        #[arg(long)]
        due: Option<String>,
        /// Estimate in points (e.g., 1, 2, 3, 5, 8, or 0 to clear)
        #[arg(short, long)]
        estimate: Option<f64>,
        /// Project name or ID (or "none" to remove from project)
        #[arg(long)]
        project: Option<String>,
        /// Preview without updating (dry run)
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete an issue
    #[command(after_help = r#"EXAMPLES:
    linear issues delete LIN-123               # Delete with confirmation
    linear i delete LIN-123 --force            # Delete without confirmation"#)]
    Delete {
        /// Issue ID
        id: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Start working on an issue (set to In Progress and assign to me)
    #[command(after_help = r#"EXAMPLES:
    linear issues start LIN-123                # Start working on issue
    linear i start LIN-123 --checkout          # Start and checkout git branch
    linear i start LIN-123 -c -b feature/fix   # Start with custom branch"#)]
    Start {
        /// Issue ID or identifier (e.g., "LIN-123")
        id: String,
        /// Checkout a git branch for the issue
        #[arg(short, long)]
        checkout: bool,
        /// Custom branch name (optional, uses issue's branch name by default)
        #[arg(short, long)]
        branch: Option<String>,
    },
    /// Stop working on an issue (return to backlog state)
    #[command(after_help = r#"EXAMPLES:
    linear issues stop LIN-123                 # Stop working on issue
    linear i stop LIN-123 --unassign           # Stop and unassign"#)]
    Stop {
        /// Issue ID or identifier (e.g., "LIN-123")
        id: String,
        /// Unassign the issue
        #[arg(short, long)]
        unassign: bool,
    },
    /// Close an issue (mark as Done)
    #[command(alias = "done")]
    Close {
        /// Issue ID or identifier
        id: String,
    },
    /// Archive an issue
    Archive {
        /// Issue ID or identifier
        id: String,
    },
    /// Unarchive an issue
    Unarchive {
        /// Issue ID or identifier
        id: String,
    },
    /// Add a comment to an issue
    Comment {
        /// Issue ID or identifier
        id: String,
        /// Comment body (markdown). Use "-" to read from stdin.
        #[arg(short, long)]
        body: String,
    },
    /// Print the issue URL
    Link {
        /// Issue ID or identifier
        id: String,
    },
    /// Assign an issue to a user (shortcut for update --assignee)
    Assign {
        /// Issue ID or identifier
        id: String,
        /// User to assign (name, email, or "me"). Omit to unassign.
        user: Option<String>,
    },
    /// Move an issue to a different project
    #[command(alias = "mv")]
    Move {
        /// Issue ID or identifier
        id: String,
        /// Target project name or ID
        project: String,
    },
    /// Transfer an issue to a different team
    Transfer {
        /// Issue ID or identifier
        id: String,
        /// Target team key or ID (e.g., "ENG")
        team: String,
    },
}

#[derive(Tabled)]
struct IssueRow {
    #[tabled(rename = "ID")]
    identifier: String,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "State")]
    state: String,
    #[tabled(rename = "Priority")]
    priority: String,
    #[tabled(rename = "Assignee")]
    assignee: String,
}

pub async fn handle(
    cmd: IssueCommands,
    output: &OutputOptions,
    agent_opts: AgentOptions,
) -> Result<()> {
    match cmd {
        IssueCommands::List {
            team,
            state,
            assignee,
            mine,
            project,
            label,
            view,
            since,
            archived,
            group_by,
            count_only,
        } => {
            let assignee = if mine {
                Some("me".to_string())
            } else {
                assignee
            };
            list_issues(
                team, state, assignee, project, label, view, since, archived, group_by, count_only,
                output, agent_opts,
            )
            .await
        }
        IssueCommands::Get {
            ids,
            history,
            comments,
        } => {
            // Support reading from stdin if no IDs provided or if "-" is passed
            let final_ids = read_ids_from_stdin(ids);
            if final_ids.is_empty() {
                anyhow::bail!(
                    "No issue IDs provided. Provide IDs as arguments or pipe them via stdin.\nExamples:\n  linear issues get LIN-123\n  printf '%s\\n' LIN-123 LIN-456 | linear issues get -"
                );
            }
            get_issues(&final_ids, output, history, comments).await
        }
        IssueCommands::Open { id } => open_issue(&id).await,
        IssueCommands::Create {
            title,
            team,
            description,
            data,
            priority,
            state,
            assignee,
            labels,
            due,
            estimate,
            template,
            dry_run,
        } => {
            let dry_run = dry_run || output.dry_run || agent_opts.dry_run;
            // Load template if specified
            let tpl = if let Some(ref tpl_name) = template {
                templates::get_template(tpl_name)?
                    .ok_or_else(|| anyhow::anyhow!("Template not found: {}", tpl_name))?
            } else {
                templates::IssueTemplate {
                    name: String::new(),
                    title_prefix: None,
                    description: None,
                    default_priority: None,
                    default_labels: vec![],
                    team: None,
                }
            };

            // Team from CLI arg takes precedence, then template, then error
            let data_json = read_json_data(data.as_deref())?;
            let data_team = data_json.as_ref().and_then(|v| {
                v.get("team")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            });
            let data_team_id = data_json.as_ref().and_then(|v| {
                v.get("teamId")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            });
            let final_team = team
                .or(tpl.team.clone())
                .or(data_team)
                .or(data_team_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("--team is required (or use a template with a default team)")
                })?;

            // Build title with optional prefix from template
            let final_title = if let Some(ref prefix) = tpl.title_prefix {
                format!("{} {}", prefix, title)
            } else {
                title
            };

            // Merge template defaults with CLI args (CLI takes precedence)
            // Support reading description from stdin if "-" is passed
            if data.as_deref() == Some("-") && description.as_deref() == Some("-") {
                anyhow::bail!("--data - and --description - cannot both read from stdin");
            }

            let final_description = match description.as_deref() {
                Some("-") => {
                    let stdin = io::stdin();
                    let lines: Vec<String> = stdin.lock().lines().map_while(Result::ok).collect();
                    Some(lines.join("\n"))
                }
                Some(d) => Some(d.to_string()),
                None => tpl.description.clone(),
            };
            let final_priority = priority.or(tpl.default_priority);

            // Merge labels: template labels + CLI labels
            let mut final_labels = tpl.default_labels.clone();
            final_labels.extend(labels);

            create_issue(
                &final_title,
                &final_team,
                data_json,
                final_description,
                final_priority,
                state,
                assignee,
                final_labels,
                due,
                estimate,
                output,
                agent_opts,
                dry_run,
            )
            .await
        }
        IssueCommands::Update {
            id,
            title,
            description,
            data,
            priority,
            state,
            assignee,
            labels,
            due,
            estimate,
            project,
            dry_run,
        } => {
            let dry_run = dry_run || output.dry_run || agent_opts.dry_run;
            if data.as_deref() == Some("-") && description.as_deref() == Some("-") {
                anyhow::bail!("--data - and --description - cannot both read from stdin");
            }

            let data_json = read_json_data(data.as_deref())?;
            // Support reading description from stdin if "-" is passed
            let final_description = match description.as_deref() {
                Some("-") => {
                    let stdin = io::stdin();
                    let lines: Vec<String> = stdin.lock().lines().map_while(Result::ok).collect();
                    Some(lines.join("\n"))
                }
                Some(d) => Some(d.to_string()),
                None => None,
            };
            update_issue(
                &id,
                title,
                final_description,
                data_json,
                priority,
                state,
                assignee,
                labels,
                due,
                estimate,
                project,
                dry_run,
                output,
                agent_opts,
            )
            .await
        }
        IssueCommands::Delete { id, force } => delete_issue(&id, force, agent_opts).await,
        IssueCommands::Start {
            id,
            checkout,
            branch,
        } => start_issue(&id, checkout, branch, agent_opts).await,
        IssueCommands::Stop { id, unassign } => stop_issue(&id, unassign, agent_opts).await,
        IssueCommands::Close { id } => close_issue(&id).await,
        IssueCommands::Archive { id } => archive_issue(&id, true).await,
        IssueCommands::Unarchive { id } => archive_issue(&id, false).await,
        IssueCommands::Comment { id, body } => comment_issue(&id, &body).await,
        IssueCommands::Link { id } => link_issue(&id).await,
        IssueCommands::Assign { id, user } => assign_issue(&id, user).await,
        IssueCommands::Move { id, project } => move_issue(&id, &project).await,
        IssueCommands::Transfer { id, team } => transfer_issue(&id, &team).await,
    }
}

fn build_issue_assignee_filter(assignee: &str) -> Value {
    if assignee.eq_ignore_ascii_case("me") {
        json!({ "isMe": { "eq": true } })
    } else if is_uuid(assignee) {
        json!({ "id": { "eq": assignee } })
    } else if assignee.contains('@') {
        json!({ "email": { "eqIgnoreCase": assignee } })
    } else {
        json!({ "name": { "eqIgnoreCase": assignee } })
    }
}

#[allow(clippy::too_many_arguments)]
async fn list_issues(
    team: Option<String>,
    state: Option<String>,
    assignee: Option<String>,
    project: Option<String>,
    label: Option<String>,
    view: Option<String>,
    since: Option<String>,
    include_archived: bool,
    group_by: Option<String>,
    count_only: bool,
    output: &OutputOptions,
    _agent_opts: AgentOptions,
) -> Result<()> {
    let client = LinearClient::new()?;

    // Resolve team key/name to ID upfront (supports both key like "SCW" and full name)
    let team_id = if let Some(ref t) = team {
        Some(resolve_team_id(&client, t, &output.cache).await?)
    } else {
        None
    };

    // Parse --since date
    let since_date = if let Some(ref since_str) = since {
        let date = crate::dates::parse_due_date(since_str).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid --since date: '{}'. Use today, -7d, 2024-01-15, etc.",
                since_str
            )
        })?;
        Some(format!("{}T00:00:00.000Z", date))
    } else {
        None
    };

    // If --view is specified, fetch the view's filterData and use it
    let filter_data = if let Some(ref view_name) = view {
        Some(super::views::fetch_view_filter(&client, view_name, &output.cache).await?)
    } else {
        None
    };

    let query = r#"
        query($filter: IssueFilter, $includeArchived: Boolean, $first: Int, $after: String, $last: Int, $before: String) {
            issues(
                first: $first,
                after: $after,
                last: $last,
                before: $before,
                includeArchived: $includeArchived,
                filter: $filter
            ) {
                nodes {
                    id
                    identifier
                    title
                    priority
                    state { name }
                    assignee { name }
                    project { name }
                }
                pageInfo {
                    hasNextPage
                    endCursor
                    hasPreviousPage
                    startCursor
                }
            }
        }
    "#;

    let mut variables = Map::new();
    variables.insert("includeArchived".to_string(), json!(include_archived));

    // Build filter dynamically — start from view filter if present, otherwise empty
    let mut filter = match filter_data {
        Some(fd) => fd,
        None => json!({}),
    };

    if let Some(ref since_ts) = since_date {
        filter["createdAt"] = json!({ "gte": since_ts });
    }
    if let Some(ref t) = team_id {
        filter["team"] = json!({ "id": { "eq": t } });
    }
    if let Some(s) = state {
        filter["state"] = json!({ "name": { "eqIgnoreCase": s } });
    }
    if let Some(a) = assignee {
        filter["assignee"] = build_issue_assignee_filter(&a);
    }
    if let Some(p) = project {
        filter["project"] = json!({ "name": { "eqIgnoreCase": p } });
    }
    if let Some(ref l) = label {
        filter["labels"] = json!({ "name": { "eqIgnoreCase": l } });
    }
    // Only include filter if non-empty
    if filter.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
        variables.insert("filter".to_string(), filter);
    }

    let pagination = output.pagination.with_default_limit(50);

    // For NDJSON, use streaming to avoid buffering all results
    if output.is_ndjson() {
        let mut count = 0;
        stream_nodes(
            &client,
            query,
            variables,
            &["data", "issues", "nodes"],
            &["data", "issues", "pageInfo"],
            &pagination,
            50,
            |batch| {
                count += batch.len();
                async move {
                    for issue in batch {
                        println!("{}", serde_json::to_string(&issue)?);
                    }
                    Ok(())
                }
            },
        )
        .await?;

        return Ok(());
    }

    // For other formats, use paginate_nodes (need all results for sorting/filtering/tables)
    let issues = paginate_nodes(
        &client,
        query,
        variables,
        &["data", "issues", "nodes"],
        &["data", "issues", "pageInfo"],
        &pagination,
        50,
    )
    .await?;

    if count_only {
        if output.is_json() || output.has_template() {
            print_json_owned(serde_json::json!({"count": issues.len()}), output)?;
        } else {
            println!("{}", issues.len());
        }
        return Ok(());
    }

    if output.is_json() || output.has_template() {
        print_json_owned(serde_json::json!(issues), output)?;
        return Ok(());
    }

    let mut issues = issues;
    filter_values(&mut issues, &output.filters);

    if let Some(sort_key) = output.json.sort.as_deref() {
        sort_values(&mut issues, sort_key, output.json.order);
    }

    ensure_non_empty(&issues, output)?;
    if issues.is_empty() {
        println!("No issues found.");
        return Ok(());
    }

    let width = display_options().max_width(50);

    // Grouped output
    if let Some(ref group_field) = group_by {
        let key_fn: Box<dyn Fn(&serde_json::Value) -> String> = match group_field.as_str() {
            "state" | "status" => Box::new(|issue: &serde_json::Value| {
                safe_terminal_value(issue["state"]["name"].as_str().unwrap_or("Unknown"))
            }),
            "priority" => {
                Box::new(|issue: &serde_json::Value| priority_to_string(issue["priority"].as_i64()))
            }
            "assignee" => Box::new(|issue: &serde_json::Value| {
                safe_terminal_value(issue["assignee"]["name"].as_str().unwrap_or("Unassigned"))
            }),
            "project" => Box::new(|issue: &serde_json::Value| {
                safe_terminal_value(issue["project"]["name"].as_str().unwrap_or("No Project"))
            }),
            other => anyhow::bail!(
                "Unknown --group-by field: '{}'. Use state, priority, assignee, or project.",
                other
            ),
        };

        // Build groups preserving insertion order
        let mut groups: Vec<(String, Vec<&serde_json::Value>)> = Vec::new();
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for issue in &issues {
            let key = key_fn(issue);
            if let Some(&idx) = seen.get(&key) {
                groups[idx].1.push(issue);
            } else {
                seen.insert(key.clone(), groups.len());
                groups.push((key, vec![issue]));
            }
        }

        for (group_name, group_issues) in &groups {
            println!("\n{} ({})", group_name.cyan().bold(), group_issues.len());
            println!("{}", "-".repeat(50));
            for issue in group_issues {
                let id = issue["identifier"].as_str().unwrap_or("");
                let title = truncate(issue["title"].as_str().unwrap_or(""), width);
                println!("  {} {}", id.cyan(), title);
            }
        }
        println!("\n{} issues in {} groups", issues.len(), groups.len());
        return Ok(());
    }

    let rows: Vec<IssueRow> = issues
        .iter()
        .map(|issue| IssueRow {
            identifier: issue["identifier"].as_str().unwrap_or("").to_string(),
            title: truncate(issue["title"].as_str().unwrap_or(""), width),
            state: safe_terminal_value(issue["state"]["name"].as_str().unwrap_or("-")),
            priority: priority_to_string(issue["priority"].as_i64()),
            assignee: issue["assignee"]["name"]
                .as_str()
                .map(safe_terminal_value)
                .unwrap_or_else(|| "-".to_string()),
        })
        .collect();

    let table = Table::new(rows).to_string();
    println!("{}", table);
    println!("\n{} issues", issues.len());

    Ok(())
}

/// Get multiple issues (supports batch fetching with concurrency limit)
async fn get_issues(
    ids: &[String],
    output: &OutputOptions,
    history: bool,
    comments: bool,
) -> Result<()> {
    // Handle single ID (most common case)
    if ids.len() == 1 {
        return get_issue(&ids[0], output, history, comments).await;
    }

    let client = LinearClient::new()?;

    // Limit concurrent requests to avoid rate limiting and socket exhaustion
    use futures::stream::{self, StreamExt};
    const MAX_CONCURRENT: usize = 10;

    let results: Vec<_> = stream::iter(ids.iter().cloned())
        .map(|id| {
            let client = client.clone();
            async move {
                let query = r#"
                    query($id: String!) {
                        issue(id: $id) {
                            id
                            identifier
                            title
                            description
                            priority
                            url
                            createdAt
                            updatedAt
                            state { name }
                            team { name }
                            assignee { name email }
                            labels { nodes { name color } }
                            project { name }
                            parent { identifier title }
                            children { nodes { identifier title state { name } } }
                            dueDate
                            estimate
                        }
                    }
                "#;
                let result = client.query(query, Some(json!({ "id": id }))).await;
                (id, result)
            }
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;

    // Any missing/errored issue exits non-zero; the issues found are still
    // printed. Computed before printing so `results` can be consumed below.
    let status = multi_get_status(&results);

    // JSON output: array of issues
    if output.is_json() || output.has_template() {
        let issues: Vec<_> = results
            .iter()
            .filter_map(|(_, r)| {
                r.as_ref().ok().and_then(|data| {
                    let issue = &data["data"]["issue"];
                    if !issue.is_null() {
                        Some(issue.clone())
                    } else {
                        None
                    }
                })
            })
            .collect();
        print_json_owned(serde_json::json!(issues), output)?;
        return status;
    }

    // Table output
    for (id, result) in results {
        match result {
            Ok(data) => {
                let issue = &data["data"]["issue"];
                if issue.is_null() {
                    eprintln!("{} Issue not found: {}", "!".yellow(), id);
                } else {
                    let identifier =
                        safe_terminal_value(issue["identifier"].as_str().unwrap_or(""));
                    let title = safe_terminal_value(issue["title"].as_str().unwrap_or(""));
                    let state = safe_terminal_value(issue["state"]["name"].as_str().unwrap_or("-"));
                    let priority = priority_to_string(issue["priority"].as_i64());
                    println!("{} {} [{}] {}", identifier.cyan(), title, state, priority);
                }
            }
            Err(e) => {
                eprintln!("{} Error fetching {}: {}", "!".red(), id, e);
            }
        }
    }

    status
}

/// Aggregate exit status for a multi-issue fetch. A real fetch error takes
/// priority and keeps its kind (e.g. RateLimited stays retryable); only a `null`
/// issue node maps to NotFound.
fn multi_get_status(results: &[(String, Result<serde_json::Value>)]) -> Result<()> {
    if let Some((id, e)) = results.iter().find_map(|(id, r)| match r {
        Err(e) => Some((id, e)),
        Ok(_) => None,
    }) {
        return Err(error::rewrap_with_message(
            e,
            format!("Error fetching {id}: {e}"),
        ));
    }

    let missing: Vec<&str> = results
        .iter()
        .filter(|(_, r)| matches!(r, Ok(data) if data["data"]["issue"].is_null()))
        .map(|(id, _)| id.as_str())
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(CliError::not_found(format!("Issue(s) not found: {}", missing.join(", "))).into())
    }
}

async fn open_issue(id: &str) -> Result<()> {
    let client = LinearClient::new()?;
    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                identifier
                url
            }
        }
    "#;
    let result = client.query(query, Some(json!({ "id": id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", id);
    }

    let url = issue["url"].as_str().unwrap_or("");
    if url.is_empty() {
        anyhow::bail!("No URL for issue: {}", id);
    }

    let identifier = issue["identifier"].as_str().unwrap_or(id);
    println!("Opening {} in browser...", identifier);
    open::that(url)?;
    Ok(())
}

fn format_history_entry(entry: &serde_json::Value) -> String {
    let mut parts = Vec::new();

    // State change
    if let (Some(from), Some(to)) = (
        entry["fromState"]["name"].as_str(),
        entry["toState"]["name"].as_str(),
    ) {
        parts.push(format!("Status: {} → {}", from, to));
    }

    // Assignee change
    match (
        entry["fromAssignee"]["name"].as_str(),
        entry["toAssignee"]["name"].as_str(),
    ) {
        (Some(from), Some(to)) => parts.push(format!("Assignee: {} → {}", from, to)),
        (None, Some(to)) => parts.push(format!("Assigned to {}", to)),
        (Some(from), None) => parts.push(format!("Unassigned from {}", from)),
        _ => {}
    }

    // Priority change
    if let (Some(from), Some(to)) = (entry["fromPriority"].as_f64(), entry["toPriority"].as_f64()) {
        let from_i = from as i64;
        let to_i = to as i64;
        if from_i != to_i {
            parts.push(format!(
                "Priority: {} → {}",
                priority_to_string(Some(from_i)),
                priority_to_string(Some(to_i))
            ));
        }
    }

    // Title change
    if entry["fromTitle"].is_string() && entry["toTitle"].is_string() {
        parts.push("Title updated".to_string());
    }

    // Description change
    if entry["updatedDescription"].as_bool() == Some(true) {
        parts.push("Description updated".to_string());
    }

    // Labels added/removed
    if let Some(added) = entry["addedLabels"].as_array() {
        if !added.is_empty() {
            let names: Vec<&str> = added.iter().filter_map(|l| l["name"].as_str()).collect();
            if !names.is_empty() {
                parts.push(format!("Added labels: {}", names.join(", ")));
            }
        }
    }
    if let Some(removed) = entry["removedLabels"].as_array() {
        if !removed.is_empty() {
            let names: Vec<&str> = removed.iter().filter_map(|l| l["name"].as_str()).collect();
            if !names.is_empty() {
                parts.push(format!("Removed labels: {}", names.join(", ")));
            }
        }
    }

    // Project change
    match (
        entry["fromProject"]["name"].as_str(),
        entry["toProject"]["name"].as_str(),
    ) {
        (Some(from), Some(to)) => parts.push(format!("Project: {} → {}", from, to)),
        (None, Some(to)) => parts.push(format!("Added to project {}", to)),
        (Some(from), None) => parts.push(format!("Removed from project {}", from)),
        _ => {}
    }

    // Archive/trash
    if entry["archived"].as_bool() == Some(true) {
        parts.push("Archived".to_string());
    }
    if entry["trashed"].as_bool() == Some(true) {
        parts.push("Trashed".to_string());
    }

    // SLA changes
    if entry["slaStartedAt"].is_string() {
        parts.push("SLA started".to_string());
    }
    if entry["slaBreachesAt"].is_string() {
        parts.push(format!(
            "SLA breaches at {}",
            entry["slaBreachesAt"].as_str().unwrap_or("")
        ));
    }

    crate::text::sanitize_terminal_text(&parts.join("; "))
}

async fn get_issue(id: &str, output: &OutputOptions, history: bool, comments: bool) -> Result<()> {
    let client = LinearClient::new()?;

    let history_fragment = if history {
        r#"
                history(first: 15) {
                    nodes {
                        createdAt
                        actor { name }
                        fromState { name }
                        toState { name }
                        fromAssignee { name }
                        toAssignee { name }
                        fromPriority
                        toPriority
                        fromTitle
                        toTitle
                        updatedDescription
                        addedLabels { name }
                        removedLabels { name }
                        fromProject { name }
                        toProject { name }
                        archived
                        trashed
                        slaBreachesAt
                        slaStartedAt
                    }
                }"#
    } else {
        ""
    };

    let comments_fragment = if comments {
        r#"
                comments(first: 20) {
                    nodes {
                        createdAt
                        body
                        user { name }
                    }
                }"#
    } else {
        ""
    };

    let query = format!(
        r#"
        query($id: String!) {{
            issue(id: $id) {{
                id
                identifier
                title
                description
                priority
                url
                createdAt
                updatedAt
                state {{ name }}
                team {{ name }}
                assignee {{ name email }}
                labels {{ nodes {{ name color }} }}
                project {{ name }}
                parent {{ identifier title }}
                children {{ nodes {{ identifier title state {{ name }} }} }}
                dueDate
                estimate
                slaStartedAt
                slaMediumRiskAt
                slaHighRiskAt
                slaBreachesAt
                slaType
                {}
                {}
            }}
        }}
        "#,
        history_fragment, comments_fragment
    );

    let result = client.query(&query, Some(json!({ "id": id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", id);
    }

    // Handle JSON output
    if output.is_json() || output.has_template() {
        print_json(issue, output)?;
        return Ok(());
    }

    let identifier = safe_terminal_value(issue["identifier"].as_str().unwrap_or(""));
    let title = safe_terminal_value(issue["title"].as_str().unwrap_or(""));
    println!("{} {}", identifier.cyan().bold(), title.bold());
    println!("{}", "-".repeat(60));

    if let Some(desc) = issue["description"].as_str() {
        if !desc.is_empty() {
            println!("\n{}", crate::text::strip_markdown(desc));
            println!();
        }
    }

    println!(
        "State:    {}",
        safe_terminal_value(issue["state"]["name"].as_str().unwrap_or("-"))
    );
    println!(
        "Priority: {}",
        priority_to_string(issue["priority"].as_i64())
    );
    println!(
        "Team:     {}",
        safe_terminal_value(issue["team"]["name"].as_str().unwrap_or("-"))
    );

    if let Some(assignee) = issue["assignee"]["name"].as_str() {
        let assignee = safe_terminal_value(assignee);
        let email = safe_terminal_value(issue["assignee"]["email"].as_str().unwrap_or(""));
        if !email.is_empty() {
            println!("Assignee: {} ({})", assignee, email.dimmed());
        } else {
            println!("Assignee: {}", assignee);
        }
    } else {
        println!("Assignee: -");
    }

    if let Some(project) = issue["project"]["name"].as_str() {
        println!("Project:  {}", safe_terminal_value(project));
    }

    if let Some(parent) = issue["parent"]["identifier"].as_str() {
        let parent = safe_terminal_value(parent);
        let parent_title = safe_terminal_value(issue["parent"]["title"].as_str().unwrap_or(""));
        println!("Parent:   {} {}", parent, parent_title.dimmed());
    }

    let labels = issue["labels"]["nodes"].as_array();
    if let Some(labels) = labels {
        if !labels.is_empty() {
            let label_names: Vec<String> = labels
                .iter()
                .filter_map(|l| l["name"].as_str())
                .map(safe_terminal_value)
                .collect();
            println!("Labels:   {}", label_names.join(", "));
        }
    }

    if let Some(due) = issue["dueDate"].as_str() {
        println!("Due:      {}", due);
    }
    if let Some(est) = issue["estimate"].as_f64() {
        println!("Estimate: {}", est);
    }

    // SLA information
    let has_sla = !issue["slaType"].is_null() || !issue["slaStartedAt"].is_null();
    if has_sla {
        println!();
        println!("{}", "SLA".bold());
        if let Some(sla_type) = issue["slaType"].as_str() {
            println!("  Type:       {}", safe_terminal_value(sla_type));
        }
        if let Some(started) = issue["slaStartedAt"].as_str() {
            println!("  Started:    {}", started);
        }
        if let Some(medium) = issue["slaMediumRiskAt"].as_str() {
            println!("  Medium risk: {}", medium.yellow());
        }
        if let Some(high) = issue["slaHighRiskAt"].as_str() {
            println!("  High risk:  {}", high.red());
        }
        if let Some(breaches) = issue["slaBreachesAt"].as_str() {
            println!("  Breaches at: {}", breaches.red().bold());
        }
    }

    // Sub-issues
    if let Some(children) = issue["children"]["nodes"].as_array() {
        if !children.is_empty() {
            println!("\n{} ({}):", "Sub-issues".bold(), children.len());
            for child in children {
                let cid = safe_terminal_value(child["identifier"].as_str().unwrap_or(""));
                let ctitle = safe_terminal_value(child["title"].as_str().unwrap_or(""));
                let cstate = safe_terminal_value(child["state"]["name"].as_str().unwrap_or("-"));
                println!("  {} {} [{}]", cid.cyan(), ctitle, cstate);
            }
        }
    }

    println!(
        "\nURL: {}",
        safe_terminal_value(issue["url"].as_str().unwrap_or("-"))
    );
    println!(
        "ID:  {}",
        safe_terminal_value(issue["id"].as_str().unwrap_or("-"))
    );

    // Display activity history if requested
    if history {
        if let Some(entries) = issue["history"]["nodes"].as_array() {
            if !entries.is_empty() {
                println!("\n{}", "Activity".bold());
                println!("{}", "-".repeat(60));
                for entry in entries {
                    let ts = entry["createdAt"].as_str().unwrap_or("");
                    let date = if ts.len() >= 10 { &ts[..10] } else { ts };
                    let actor =
                        safe_terminal_value(entry["actor"]["name"].as_str().unwrap_or("System"));
                    let desc = format_history_entry(entry);
                    if !desc.is_empty() {
                        println!("  {} {} — {}", date.dimmed(), actor, desc);
                    }
                }
            }
        }
    }

    // Display comments if requested
    if comments {
        if let Some(comment_nodes) = issue["comments"]["nodes"].as_array() {
            if !comment_nodes.is_empty() {
                println!("\n{} ({}):", "Comments".bold(), comment_nodes.len());
                println!("{}", "-".repeat(60));
                for comment in comment_nodes {
                    let ts = comment["createdAt"].as_str().unwrap_or("");
                    let date = if ts.len() >= 10 { &ts[..10] } else { ts };
                    let author =
                        safe_terminal_value(comment["user"]["name"].as_str().unwrap_or("Unknown"));
                    let body = comment["body"].as_str().unwrap_or("");
                    println!("\n  {} {} {}:", date.dimmed(), "by".dimmed(), author.cyan());
                    for line in crate::text::strip_markdown(body).lines() {
                        println!("    {}", line);
                    }
                }
            } else {
                println!("\nNo comments.");
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create_issue(
    title: &str,
    team: &str,
    data_json: Option<Value>,
    description: Option<String>,
    priority: Option<i32>,
    state: Option<String>,
    assignee: Option<String>,
    labels: Vec<String>,
    due: Option<String>,
    estimate: Option<f64>,
    output: &OutputOptions,
    agent_opts: AgentOptions,
    dry_run: bool,
) -> Result<()> {
    let client = LinearClient::new()?;

    // Determine the final team (CLI arg takes precedence, then template, then error)
    let final_team = team;

    // Resolve team key/name to UUID
    let team_id = resolve_team_id(&client, final_team, &output.cache).await?;

    // Build the title with optional prefix from template
    let final_title = title.to_string();

    let mut input = match data_json {
        Some(Value::Object(map)) => Value::Object(map),
        Some(_) => anyhow::bail!("--data must be a JSON object"),
        None => json!({}),
    };

    input["title"] = json!(final_title);
    input["teamId"] = json!(team_id);

    // CLI args override template values
    if let Some(ref desc) = description {
        input["description"] = json!(desc);
    }
    if let Some(p) = priority {
        input["priority"] = json!(p);
    }
    if let Some(ref s) = state {
        if dry_run {
            input["stateId"] = json!(s);
        } else {
            let state_id = resolve_state_id(&client, &team_id, s).await?;
            input["stateId"] = json!(state_id);
        }
    }
    if let Some(ref a) = assignee {
        // Resolve user name/email to UUID (skip during dry-run to avoid API calls)
        if dry_run {
            input["assigneeId"] = json!(a);
        } else {
            let assignee_id = resolve_user_id(&client, a, &output.cache).await?;
            input["assigneeId"] = json!(assignee_id);
        }
    }
    if !labels.is_empty() {
        // Resolve label names to UUIDs (skip during dry-run to avoid API calls)
        if dry_run {
            let mut label_ids: Vec<String> = input["labelIds"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            label_ids.extend(labels.clone());
            input["labelIds"] = json!(label_ids);
        } else {
            let mut label_ids: Vec<String> = input["labelIds"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            for label in &labels {
                let label_id = resolve_label_id(&client, label, &output.cache).await?;
                label_ids.push(label_id);
            }
            input["labelIds"] = json!(label_ids);
        }
    }
    if let Some(ref d) = due {
        // Parse due date shorthand
        if let Some(parsed) = crate::dates::parse_due_date(d) {
            input["dueDate"] = json!(parsed);
        } else {
            // Assume it's already a valid date format
            input["dueDate"] = json!(d);
        }
    }
    if let Some(e) = estimate {
        input["estimate"] = json!(e);
    }

    // Dry run: show what would be created without actually creating
    if dry_run {
        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({
                    "dry_run": true,
                    "would_create": {
                        "title": final_title,
                        "team": final_team,
                        "teamId": team_id,
                        "description": description,
                        "priority": priority,
                        "state": state,
                        "assignee": assignee,
                        "labels": labels,
                        "dueDate": due,
                        "estimate": estimate,
                    }
                }),
                output,
            )?;
        } else {
            println!("{}", "[DRY RUN] Would create issue:".yellow().bold());
            println!("  Title:       {}", final_title);
            println!("  Team:        {} ({})", final_team, team_id);
            if let Some(ref desc) = description {
                let preview: String = desc.chars().take(50).collect();
                let preview = if preview.len() < desc.len() {
                    format!("{}...", preview)
                } else {
                    preview
                };
                println!("  Description: {}", preview);
            }
            if let Some(p) = priority {
                println!("  Priority:    {}", p);
            }
            if let Some(ref s) = state {
                println!("  State:       {}", s);
            }
            if let Some(ref a) = assignee {
                println!("  Assignee:    {}", a);
            }
            if !labels.is_empty() {
                println!("  Labels:      {}", labels.join(", "));
            }
            if let Some(ref d) = due {
                println!("  Due:         {}", d);
            }
            if let Some(e) = estimate {
                println!("  Estimate:    {}", e);
            }
        }
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

    let result = client
        .mutate(mutation, Some(json!({ "input": input })))
        .await?;

    if result["data"]["issueCreate"]["success"].as_bool() == Some(true) {
        let issue = &result["data"]["issueCreate"]["issue"];
        let identifier = issue["identifier"].as_str().unwrap_or("");

        // --id-only: Just output the identifier for chaining
        if agent_opts.id_only {
            println!("{}", identifier);
            return Ok(());
        }

        // Handle JSON output
        if output.is_json() || output.has_template() {
            print_json(issue, output)?;
            return Ok(());
        }

        // Quiet mode: minimal output
        if agent_opts.quiet {
            println!("{}", identifier);
            return Ok(());
        }

        let issue_title = issue["title"].as_str().unwrap_or("");
        println!(
            "{} Created issue: {} {}",
            "+".green(),
            identifier.cyan(),
            issue_title
        );
        println!(
            "  ID:  {}",
            safe_terminal_value(issue["id"].as_str().unwrap_or(""))
        );
        println!(
            "  URL: {}",
            safe_terminal_value(issue["url"].as_str().unwrap_or(""))
        );
    } else {
        anyhow::bail!("Failed to create issue");
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_issue(
    id: &str,
    title: Option<String>,
    description: Option<String>,
    data_json: Option<Value>,
    priority: Option<i32>,
    state: Option<String>,
    assignee: Option<String>,
    labels: Vec<String>,
    due: Option<String>,
    estimate: Option<f64>,
    project: Option<String>,
    dry_run: bool,
    output: &OutputOptions,
    agent_opts: AgentOptions,
) -> Result<()> {
    let client = LinearClient::new()?;

    let mut input = match data_json {
        Some(Value::Object(map)) => Value::Object(map),
        Some(_) => anyhow::bail!("--data must be a JSON object"),
        None => json!({}),
    };

    if let Some(t) = title {
        input["title"] = json!(t);
    }
    if let Some(d) = description {
        input["description"] = json!(d);
    }
    if let Some(p) = priority {
        input["priority"] = json!(p);
    }
    if let Some(s) = state {
        if dry_run {
            input["stateId"] = json!(s);
        } else {
            // Fetch the issue's team ID to resolve state name
            let team_query = r#"
                query($id: String!) {
                    issue(id: $id) {
                        team { id }
                    }
                }
            "#;
            let team_result = client.query(team_query, Some(json!({ "id": id }))).await?;
            let issue_team_id = team_result["data"]["issue"]["team"]["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Could not determine team for issue {}", id))?;
            let state_id = resolve_state_id(&client, issue_team_id, &s).await?;
            input["stateId"] = json!(state_id);
        }
    }
    if let Some(a) = assignee {
        // Resolve user name/email to UUID (skip during dry-run to avoid API calls)
        if dry_run {
            input["assigneeId"] = json!(a);
        } else {
            let assignee_id = resolve_user_id(&client, &a, &output.cache).await?;
            input["assigneeId"] = json!(assignee_id);
        }
    }
    if !labels.is_empty() {
        // Resolve label names to UUIDs (skip during dry-run to avoid API calls)
        if dry_run {
            input["labelIds"] = json!(labels);
        } else {
            let mut label_ids = Vec::new();
            for label in &labels {
                let label_id = resolve_label_id(&client, label, &output.cache).await?;
                label_ids.push(label_id);
            }
            input["labelIds"] = json!(label_ids);
        }
    }
    if let Some(ref d) = due {
        // Support clearing due date with "none"
        if d.eq_ignore_ascii_case("none") || d.eq_ignore_ascii_case("clear") {
            input["dueDate"] = json!(null);
        } else if let Some(parsed) = crate::dates::parse_due_date(d) {
            input["dueDate"] = json!(parsed);
        } else {
            input["dueDate"] = json!(d);
        }
    }
    if let Some(e) = estimate {
        // 0 clears the estimate
        if e == 0.0 {
            input["estimate"] = json!(null);
        } else {
            input["estimate"] = json!(e);
        }
    }
    if let Some(ref p) = project {
        if p.eq_ignore_ascii_case("none") || p.eq_ignore_ascii_case("clear") {
            input["projectId"] = json!(null);
        } else if dry_run {
            input["projectId"] = json!(p);
        } else {
            let project_id = resolve_project_id(&client, p, &output.cache).await?;
            input["projectId"] = json!(project_id);
        }
    }

    if input.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        if !agent_opts.quiet {
            println!("No updates specified.");
        }
        return Ok(());
    }

    if dry_run {
        if output.is_json() || output.has_template() {
            print_json_owned(
                json!({
                    "dry_run": true,
                    "would_update": {
                        "id": id,
                        "input": input,
                    }
                }),
                output,
            )?;
        } else {
            println!("{}", "[DRY RUN] Would update issue:".yellow().bold());
            println!("  ID: {}", id);
        }
        return Ok(());
    }

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

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
        let issue = &result["data"]["issueUpdate"]["issue"];
        let identifier = issue["identifier"].as_str().unwrap_or("");

        // --id-only: Just output the identifier
        if agent_opts.id_only {
            println!("{}", identifier);
            return Ok(());
        }

        // Handle JSON output
        if output.is_json() || output.has_template() {
            print_json(issue, output)?;
            return Ok(());
        }

        // Quiet mode
        if agent_opts.quiet {
            println!("{}", identifier);
            return Ok(());
        }

        println!(
            "{} Updated issue: {} {}",
            "+".green(),
            identifier,
            issue["title"].as_str().unwrap_or("")
        );
    } else {
        anyhow::bail!("Failed to update issue");
    }

    Ok(())
}

fn read_json_data(data: Option<&str>) -> Result<Option<Value>> {
    let Some(data) = data else { return Ok(None) };
    let raw = if data == "-" {
        let stdin = io::stdin();
        let lines: Vec<String> = stdin.lock().lines().map_while(Result::ok).collect();
        lines.join("\n")
    } else {
        data.to_string()
    };
    let value: Value = serde_json::from_str(&raw)?;
    Ok(Some(value))
}

async fn delete_issue(id: &str, force: bool, agent_opts: AgentOptions) -> Result<()> {
    let skip_confirm = force || crate::is_yes();
    if !skip_confirm && !agent_opts.quiet {
        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!("Delete issue {}? This cannot be undone", id))
            .default(false)
            .interact()?;

        if !confirm {
            println!("Cancelled.");
            return Ok(());
        }
    } else if !skip_confirm && agent_opts.quiet {
        // In quiet mode without force, require --force
        anyhow::bail!("Use --force to delete in quiet mode");
    }

    let client = LinearClient::new()?;

    let mutation = r#"
        mutation($id: String!) {
            issueDelete(id: $id) {
                success
            }
        }
    "#;

    let result = client.mutate(mutation, Some(json!({ "id": id }))).await?;

    if result["data"]["issueDelete"]["success"].as_bool() == Some(true) {
        if !agent_opts.quiet {
            println!("{} Issue deleted", "+".green());
        }
    } else {
        anyhow::bail!("Failed to delete issue");
    }

    Ok(())
}

// Git helper functions for start command

fn branch_exists(branch: &str) -> bool {
    git_branch_exists(branch)
}

async fn start_issue(
    id: &str,
    checkout: bool,
    custom_branch: Option<String>,
    agent_opts: AgentOptions,
) -> Result<()> {
    let client = LinearClient::new()?;

    // First, get the issue details including team info to find the "started" state
    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                title
                branchName
                team {
                    id
                    states {
                        nodes {
                            id
                            name
                            type
                        }
                    }
                }
            }
            viewer {
                id
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", id);
    }

    let identifier = issue["identifier"].as_str().unwrap_or("");
    let title = issue["title"].as_str().unwrap_or("");
    let linear_branch = issue["branchName"].as_str().unwrap_or("").to_string();

    // Get current user ID
    let viewer_id = result["data"]["viewer"]["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Could not fetch current user ID"))?;

    // Find a "started" type state (In Progress)
    let empty = vec![];
    let states = issue["team"]["states"]["nodes"]
        .as_array()
        .unwrap_or(&empty);

    let started_state = states
        .iter()
        .find(|s| s["type"].as_str() == Some("started"));

    let state_id = match started_state {
        Some(s) => s["id"].as_str().unwrap_or(""),
        None => anyhow::bail!("No 'started' state found for this team"),
    };

    let state_name = started_state
        .and_then(|s| s["name"].as_str())
        .unwrap_or("In Progress");

    // Update the issue: set state to "In Progress" and assign to current user
    let input = json!({
        "stateId": state_id,
        "assigneeId": viewer_id
    });

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue {
                    identifier
                    title
                    state { name }
                    assignee { name }
                }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
        let updated = &result["data"]["issueUpdate"]["issue"];
        let updated_id = updated["identifier"].as_str().unwrap_or("");

        if agent_opts.id_only {
            println!("{}", updated_id);
        } else if !agent_opts.quiet {
            println!(
                "{} Started issue: {} {}",
                "+".green(),
                updated_id.cyan(),
                updated["title"].as_str().unwrap_or("")
            );
            println!(
                "  State:    {}",
                updated["state"]["name"].as_str().unwrap_or(state_name)
            );
            println!(
                "  Assignee: {}",
                updated["assignee"]["name"].as_str().unwrap_or("me")
            );
        }
    } else {
        anyhow::bail!("Failed to start issue");
    }

    // Optionally checkout a git branch
    if checkout {
        let branch_name = if let Some(custom_branch) = custom_branch {
            validate_branch_name(&custom_branch)?;
            custom_branch
        } else if !linear_branch.is_empty() && validate_branch_name(&linear_branch).is_ok() {
            linear_branch
        } else {
            generate_branch_name(identifier, title)
        };

        if !agent_opts.quiet {
            println!();
        }
        if branch_exists(&branch_name) {
            if !agent_opts.quiet {
                println!("Checking out existing branch: {}", branch_name.green());
            }
            run_git_command(&["checkout", &branch_name])?;
        } else {
            if !agent_opts.quiet {
                println!("Creating and checking out branch: {}", branch_name.green());
            }
            run_git_command(&["checkout", "-b", &branch_name])?;
        }

        let current = run_git_command(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        if !agent_opts.quiet {
            println!("{} Now on branch: {}", "+".green(), current);
        }
    }

    Ok(())
}

async fn stop_issue(id: &str, unassign: bool, agent_opts: AgentOptions) -> Result<()> {
    let client = LinearClient::new()?;

    // First, get the issue details including team info to find the "backlog" or "unstarted" state
    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                title
                team {
                    id
                    states {
                        nodes {
                            id
                            name
                            type
                        }
                    }
                }
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", id);
    }

    // Find a "backlog" or "unstarted" type state
    let empty = vec![];
    let states = issue["team"]["states"]["nodes"]
        .as_array()
        .unwrap_or(&empty);

    // Prefer backlog, fall back to unstarted
    let stop_state = states
        .iter()
        .find(|s| s["type"].as_str() == Some("backlog"))
        .or_else(|| {
            states
                .iter()
                .find(|s| s["type"].as_str() == Some("unstarted"))
        });

    let state_id = match stop_state {
        Some(s) => s["id"].as_str().unwrap_or(""),
        None => anyhow::bail!("No 'backlog' or 'unstarted' state found for this team"),
    };

    let state_name = stop_state
        .and_then(|s| s["name"].as_str())
        .unwrap_or("Backlog");

    // Build the update input
    let mut input = json!({
        "stateId": state_id
    });

    // Optionally unassign
    if unassign {
        input["assigneeId"] = json!(null);
    }

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue {
                    identifier
                    title
                    state { name }
                    assignee { name }
                }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
        let updated = &result["data"]["issueUpdate"]["issue"];
        let updated_id = updated["identifier"].as_str().unwrap_or("");

        if agent_opts.id_only {
            println!("{}", updated_id);
        } else if !agent_opts.quiet {
            println!(
                "{} Stopped issue: {} {}",
                "+".green(),
                updated_id.cyan(),
                updated["title"].as_str().unwrap_or("")
            );
            println!(
                "  State:    {}",
                updated["state"]["name"].as_str().unwrap_or(state_name)
            );
            if unassign {
                println!("  Assignee: (unassigned)");
            } else if let Some(assignee) = updated["assignee"]["name"].as_str() {
                println!("  Assignee: {}", assignee);
            }
        }
    } else {
        anyhow::bail!("Failed to stop issue");
    }

    Ok(())
}

async fn close_issue(id: &str) -> Result<()> {
    let client = LinearClient::new()?;

    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                title
                team {
                    states {
                        nodes {
                            id
                            name
                            type
                        }
                    }
                }
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", id);
    }

    let empty = vec![];
    let states = issue["team"]["states"]["nodes"]
        .as_array()
        .unwrap_or(&empty);

    // Find a "completed" type state (e.g., "Done")
    let done_state = states
        .iter()
        .find(|s| s["type"].as_str() == Some("completed"));

    let state_id = match done_state {
        Some(s) => s["id"].as_str().unwrap_or(""),
        None => anyhow::bail!("No 'completed' state found for this team"),
    };

    let state_name = done_state
        .and_then(|s| s["name"].as_str())
        .unwrap_or("Done");

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue {
                    identifier
                    title
                    state { name }
                }
            }
        }
    "#;

    let issue_uuid = issue["id"].as_str().unwrap_or(id);
    let result = client
        .mutate(
            mutation,
            Some(json!({ "id": issue_uuid, "input": { "stateId": state_id } })),
        )
        .await?;

    if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
        let updated = &result["data"]["issueUpdate"]["issue"];
        println!(
            "{} Closed issue: {} {}",
            "+".green(),
            updated["identifier"].as_str().unwrap_or("").cyan(),
            updated["title"].as_str().unwrap_or("")
        );
        println!(
            "  State: {}",
            updated["state"]["name"].as_str().unwrap_or(state_name)
        );
    } else {
        anyhow::bail!("Failed to close issue");
    }

    Ok(())
}

async fn archive_issue(id: &str, archive: bool) -> Result<()> {
    let client = LinearClient::new()?;

    let mutation = if archive {
        r#"
        mutation($id: String!) {
            issueArchive(id: $id) {
                success
            }
        }
        "#
    } else {
        r#"
        mutation($id: String!) {
            issueUnarchive(id: $id) {
                success
            }
        }
        "#
    };

    let result = client.mutate(mutation, Some(json!({ "id": id }))).await?;
    let key = if archive {
        "issueArchive"
    } else {
        "issueUnarchive"
    };

    if result["data"][key]["success"].as_bool() == Some(true) {
        let action = if archive { "Archived" } else { "Unarchived" };
        println!("{} {} issue: {}", "+".green(), action, id.cyan());
    } else {
        let action = if archive { "archive" } else { "unarchive" };
        anyhow::bail!("Failed to {} issue: {}", action, id);
    }

    Ok(())
}

async fn comment_issue(id: &str, body: &str) -> Result<()> {
    let client = LinearClient::new()?;

    let actual_body = if body == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        body.to_string()
    };

    if actual_body.trim().is_empty() {
        anyhow::bail!("Comment body cannot be empty");
    }

    let mutation = r#"
        mutation($input: CommentCreateInput!) {
            commentCreate(input: $input) {
                success
                comment {
                    id
                    body
                    issue { identifier }
                }
            }
        }
    "#;

    let result = client
        .mutate(
            mutation,
            Some(json!({ "input": { "issueId": id, "body": actual_body } })),
        )
        .await?;

    if result["data"]["commentCreate"]["success"].as_bool() == Some(true) {
        let comment = &result["data"]["commentCreate"]["comment"];
        let issue_id = comment["issue"]["identifier"].as_str().unwrap_or(id);
        println!("{} Added comment to {}", "+".green(), issue_id.cyan());
    } else {
        anyhow::bail!("Failed to add comment to issue: {}", id);
    }

    Ok(())
}

async fn link_issue(id: &str) -> Result<()> {
    let client = LinearClient::new()?;
    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                url
            }
        }
    "#;
    let result = client.query(query, Some(json!({ "id": id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", id);
    }

    let url = issue["url"].as_str().unwrap_or("");
    if url.is_empty() {
        anyhow::bail!("No URL for issue: {}", id);
    }

    println!("{}", url);
    Ok(())
}

async fn assign_issue(id: &str, user: Option<String>) -> Result<()> {
    let client = LinearClient::new()?;

    let assignee_id = match &user {
        Some(u) => {
            let uid = crate::api::resolve_user_id(&client, u, &CacheOptions::default()).await?;
            Some(uid)
        }
        None => None,
    };

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue { identifier assignee { name } }
            }
        }
    "#;

    let input = if let Some(ref uid) = assignee_id {
        json!({ "assigneeId": uid })
    } else {
        json!({ "assigneeId": null })
    };

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
        let issue = &result["data"]["issueUpdate"]["issue"];
        let ident = issue["identifier"].as_str().unwrap_or(id);
        if let Some(name) = issue["assignee"]["name"].as_str() {
            println!("{} Assigned {} to {}", "+".green(), ident.cyan(), name);
        } else {
            println!("{} Unassigned {}", "+".green(), ident.cyan());
        }
    } else {
        anyhow::bail!("Failed to assign issue: {}", id);
    }

    Ok(())
}

async fn move_issue(id: &str, project: &str) -> Result<()> {
    let client = LinearClient::new()?;
    let project_id =
        crate::api::resolve_project_id(&client, project, &CacheOptions::default()).await?;

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue { identifier project { name } }
            }
        }
    "#;

    let result = client
        .mutate(
            mutation,
            Some(json!({ "id": id, "input": { "projectId": project_id } })),
        )
        .await?;

    if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
        let issue = &result["data"]["issueUpdate"]["issue"];
        let ident = issue["identifier"].as_str().unwrap_or(id);
        let proj = issue["project"]["name"].as_str().unwrap_or(project);
        println!("{} Moved {} to project {}", "+".green(), ident.cyan(), proj);
    } else {
        anyhow::bail!("Failed to move issue: {}", id);
    }

    Ok(())
}

async fn transfer_issue(id: &str, team: &str) -> Result<()> {
    let client = LinearClient::new()?;
    let team_id = crate::api::resolve_team_id(&client, team, &CacheOptions::default()).await?;

    let mutation = r#"
        mutation($id: String!, $input: IssueUpdateInput!) {
            issueUpdate(id: $id, input: $input) {
                success
                issue { identifier team { name key } }
            }
        }
    "#;

    let result = client
        .mutate(
            mutation,
            Some(json!({ "id": id, "input": { "teamId": team_id } })),
        )
        .await?;

    if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
        let issue = &result["data"]["issueUpdate"]["issue"];
        let ident = issue["identifier"].as_str().unwrap_or(id);
        let team_name = issue["team"]["name"].as_str().unwrap_or(team);
        println!(
            "{} Transferred {} to team {}",
            "+".green(),
            ident.cyan(),
            team_name
        );
    } else {
        anyhow::bail!("Failed to transfer issue: {}", id);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_get_status_ok_when_all_found() {
        let results = vec![(
            "LIN-1".to_string(),
            Ok(json!({ "data": { "issue": { "id": "x" } } })),
        )];
        assert!(multi_get_status(&results).is_ok());
        assert!(multi_get_status(&[]).is_ok());
    }

    #[test]
    fn multi_get_status_notfound_when_issue_null() {
        let results = vec![(
            "LIN-9".to_string(),
            Ok(json!({ "data": { "issue": null } })),
        )];
        let err = multi_get_status(&results).unwrap_err();
        assert_eq!(err.downcast_ref::<CliError>().expect("CliError").code(), 2);
    }

    #[test]
    fn multi_get_status_preserves_rate_limited_kind_over_notfound() {
        // A 429 must stay RateLimited(4), not collapse to NotFound(2).
        let results: Vec<(String, Result<Value>)> = vec![
            (
                "LIN-1".to_string(),
                Ok(json!({ "data": { "issue": null } })),
            ),
            (
                "LIN-2".to_string(),
                Err(CliError::rate_limited("429")
                    .with_retry_after(Some(3))
                    .into()),
            ),
        ];
        let err = multi_get_status(&results).unwrap_err();
        let cli = err.downcast_ref::<CliError>().expect("CliError");
        assert_eq!(cli.code(), 4);
        assert_eq!(cli.retry_after, Some(3));
    }

    #[test]
    fn test_format_history_state_change() {
        let entry = serde_json::json!({
            "fromState": { "name": "Todo" },
            "toState": { "name": "In Progress" }
        });
        assert_eq!(format_history_entry(&entry), "Status: Todo → In Progress");
    }

    #[test]
    fn test_format_history_assignee_set() {
        let entry = serde_json::json!({
            "toAssignee": { "name": "Alice" }
        });
        assert_eq!(format_history_entry(&entry), "Assigned to Alice");
    }

    #[test]
    fn test_format_history_assignee_changed() {
        let entry = serde_json::json!({
            "fromAssignee": { "name": "Alice" },
            "toAssignee": { "name": "Bob" }
        });
        assert_eq!(format_history_entry(&entry), "Assignee: Alice → Bob");
    }

    #[test]
    fn test_format_history_priority_change() {
        // `format_history_entry` calls `sanitize_terminal_text` on its
        // output (see end of the function), so it never returns ANSI
        // escapes — compare against a plain literal. (Coloring of the
        // priority label itself is covered by priority::tests.)
        let entry = serde_json::json!({
            "fromPriority": 3.0,
            "toPriority": 1.0
        });
        assert_eq!(format_history_entry(&entry), "Priority: Normal → Urgent");
    }

    #[test]
    fn test_format_history_description_updated() {
        let entry = serde_json::json!({
            "updatedDescription": true
        });
        assert_eq!(format_history_entry(&entry), "Description updated");
    }

    #[test]
    fn test_format_history_labels_added() {
        let entry = serde_json::json!({
            "addedLabels": [{ "name": "bug" }, { "name": "urgent" }]
        });
        assert_eq!(format_history_entry(&entry), "Added labels: bug, urgent");
    }

    #[test]
    fn test_format_history_project_added() {
        let entry = serde_json::json!({
            "toProject": { "name": "FreshTrack" }
        });
        assert_eq!(format_history_entry(&entry), "Added to project FreshTrack");
    }

    #[test]
    fn test_format_history_multiple_changes() {
        let entry = serde_json::json!({
            "fromState": { "name": "Todo" },
            "toState": { "name": "Done" },
            "updatedDescription": true
        });
        let result = format_history_entry(&entry);
        assert!(result.contains("Status: Todo → Done"));
        assert!(result.contains("Description updated"));
    }

    #[test]
    fn test_format_history_empty() {
        let entry = serde_json::json!({});
        assert_eq!(format_history_entry(&entry), "");
    }

    #[test]
    fn test_format_history_archived() {
        let entry = serde_json::json!({ "archived": true });
        assert_eq!(format_history_entry(&entry), "Archived");
    }

    #[test]
    fn test_build_issue_assignee_filter_for_me() {
        assert_eq!(
            build_issue_assignee_filter("me"),
            serde_json::json!({ "isMe": { "eq": true } })
        );
    }

    #[test]
    fn test_build_issue_assignee_filter_for_uuid() {
        let user_id = "123e4567-e89b-12d3-a456-426614174000";
        assert_eq!(
            build_issue_assignee_filter(user_id),
            serde_json::json!({ "id": { "eq": user_id } })
        );
    }

    #[test]
    fn test_build_issue_assignee_filter_for_email() {
        let email = "alice@example.com";
        assert_eq!(
            build_issue_assignee_filter(email),
            serde_json::json!({ "email": { "eqIgnoreCase": email } })
        );
    }

    #[test]
    fn test_build_issue_assignee_filter_for_name() {
        let name = "Alice";
        assert_eq!(
            build_issue_assignee_filter(name),
            serde_json::json!({ "name": { "eqIgnoreCase": name } })
        );
    }
}
