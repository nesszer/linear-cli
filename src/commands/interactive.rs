use anyhow::Result;
use colored::Colorize;
use dialoguer::{console::Term, Confirm, Input, Select};
use serde_json::json;
use tabled::{Table, Tabled};

use crate::api::LinearClient;
use crate::display_options;
use crate::priority::priority_to_string;
use crate::text::truncate;

fn safe_terminal_value(value: &str) -> String {
    crate::text::sanitize_terminal_text(value)
}

#[derive(Debug, Clone)]
struct Team {
    id: String,
    name: String,
    key: String,
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
}

enum MenuAction {
    CreateIssue,
    ListIssues,
    ViewIssue,
    UpdateIssue,
    SwitchTeam,
    Exit,
}

impl std::fmt::Display for MenuAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MenuAction::CreateIssue => write!(f, "Create Issue"),
            MenuAction::ListIssues => write!(f, "List Issues"),
            MenuAction::ViewIssue => write!(f, "View Issue"),
            MenuAction::UpdateIssue => write!(f, "Update Issue"),
            MenuAction::SwitchTeam => write!(f, "Switch Team"),
            MenuAction::Exit => write!(f, "Exit"),
        }
    }
}

pub async fn run(default_team: Option<String>) -> Result<()> {
    let term = Term::stdout();
    let client = LinearClient::new()?;

    println!("{}", "Linear CLI - Interactive Mode".cyan().bold());
    println!("{}", "=".repeat(40));

    // Fetch available teams
    let teams = fetch_teams(&client).await?;
    if teams.is_empty() {
        println!("No teams found. Please check your API key.");
        return Ok(());
    }

    // Select initial team
    let mut current_team = match default_team {
        Some(team) => select_team_by_name_or_key(&teams, &team)?,
        None => select_team(&teams)?,
    };
    println!(
        "\n{} Selected team: {} ({})",
        "+".green(),
        current_team.name,
        current_team.key
    );

    loop {
        println!("\n{}", "-".repeat(40));
        println!(
            "Current team: {} {}",
            current_team.name.cyan(),
            format!("({})", current_team.key).dimmed()
        );

        let actions = [
            MenuAction::CreateIssue,
            MenuAction::ListIssues,
            MenuAction::ViewIssue,
            MenuAction::UpdateIssue,
            MenuAction::SwitchTeam,
            MenuAction::Exit,
        ];

        let action_strings: Vec<String> = actions.iter().map(|a| a.to_string()).collect();

        let selection = Select::new()
            .with_prompt("Select action")
            .items(&action_strings)
            .default(0)
            .interact_on(&term)?;

        let Some(action) = actions.get(selection) else {
            anyhow::bail!("Invalid menu selection: {}", selection);
        };
        match action {
            MenuAction::CreateIssue => {
                if let Err(e) = create_issue_interactive(&client, &current_team).await {
                    println!("{} Error: {}", "!".red(), e);
                }
            }
            MenuAction::ListIssues => {
                if let Err(e) = list_issues_interactive(&client, &current_team).await {
                    println!("{} Error: {}", "!".red(), e);
                }
            }
            MenuAction::ViewIssue => {
                if let Err(e) = view_issue_interactive(&client).await {
                    println!("{} Error: {}", "!".red(), e);
                }
            }
            MenuAction::UpdateIssue => {
                if let Err(e) = update_issue_interactive(&client).await {
                    println!("{} Error: {}", "!".red(), e);
                }
            }
            MenuAction::SwitchTeam => {
                current_team = select_team(&teams)?;
                println!(
                    "{} Switched to team: {} ({})",
                    "+".green(),
                    current_team.name,
                    current_team.key
                );
            }
            MenuAction::Exit => {
                println!("Goodbye!");
                break;
            }
        }
    }

    Ok(())
}

async fn fetch_teams(client: &LinearClient) -> Result<Vec<Team>> {
    use crate::pagination::{paginate_nodes, PaginationOptions};

    let query = r#"
        query($first: Int, $after: String) {
            teams(first: $first, after: $after) {
                nodes {
                    id
                    name
                    key
                }
                pageInfo {
                    hasNextPage
                    endCursor
                }
            }
        }
    "#;

    let teams_json = paginate_nodes(
        client,
        query,
        serde_json::Map::new(),
        &["data", "teams", "nodes"],
        &["data", "teams", "pageInfo"],
        &PaginationOptions {
            all: true,
            page_size: Some(100),
            ..Default::default()
        },
        100,
    )
    .await?;

    let teams: Vec<Team> = teams_json
        .iter()
        .map(|t| Team {
            id: t["id"].as_str().unwrap_or("").to_string(),
            name: t["name"].as_str().unwrap_or("").to_string(),
            key: t["key"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    Ok(teams)
}

fn select_team(teams: &[Team]) -> Result<Team> {
    let team_names: Vec<String> = teams
        .iter()
        .map(|t| format!("{} ({})", t.name, t.key))
        .collect();

    let selection = Select::new()
        .with_prompt("Select a team")
        .items(&team_names)
        .default(0)
        .interact()?;

    Ok(teams[selection].clone())
}

async fn create_issue_interactive(client: &LinearClient, team: &Team) -> Result<()> {
    println!("\n{}", "Create New Issue".cyan().bold());

    let title: String = Input::new().with_prompt("Title").interact_text()?;

    if title.trim().is_empty() {
        println!("Title cannot be empty.");
        return Ok(());
    }

    let description: String = Input::new()
        .with_prompt("Description (optional)")
        .allow_empty(true)
        .interact_text()?;

    let priority_options = vec!["None", "Urgent", "High", "Normal", "Low"];
    let priority_selection = Select::new()
        .with_prompt("Priority")
        .items(&priority_options)
        .default(3)
        .interact()?;

    let priority = match priority_selection {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        _ => 0,
    };

    // Fetch team states and members for optional fields
    let meta_query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                states { nodes { id name type } }
                members { nodes { id name } }
                labels { nodes { id name } }
            }
        }
    "#;
    let meta_result = client
        .query(meta_query, Some(json!({ "teamId": team.id })))
        .await?;
    let team_data = &meta_result["data"]["team"];

    // Assignee selection
    let mut assignee_id: Option<String> = None;
    if let Some(members) = team_data["members"]["nodes"].as_array() {
        if !members.is_empty() {
            let mut names: Vec<String> = vec!["(Skip)".to_string()];
            names.extend(
                members
                    .iter()
                    .filter_map(|m| m["name"].as_str().map(|s| s.to_string())),
            );
            let sel = Select::new()
                .with_prompt("Assignee")
                .items(&names)
                .default(0)
                .interact()?;
            if sel > 0 {
                assignee_id = members[sel - 1]["id"].as_str().map(|s| s.to_string());
            }
        }
    }

    // Status selection
    let mut state_id: Option<String> = None;
    if let Some(states) = team_data["states"]["nodes"].as_array() {
        if !states.is_empty() {
            let mut names: Vec<String> = vec!["(Default)".to_string()];
            names.extend(
                states
                    .iter()
                    .filter_map(|s| s["name"].as_str().map(|n| n.to_string())),
            );
            let sel = Select::new()
                .with_prompt("Status")
                .items(&names)
                .default(0)
                .interact()?;
            if sel > 0 {
                state_id = states[sel - 1]["id"].as_str().map(|s| s.to_string());
            }
        }
    }

    // Label selection
    let mut label_ids: Vec<String> = Vec::new();
    if let Some(labels) = team_data["labels"]["nodes"].as_array() {
        if !labels.is_empty() {
            let mut names: Vec<String> = vec!["(Skip)".to_string()];
            names.extend(
                labels
                    .iter()
                    .filter_map(|l| l["name"].as_str().map(|n| n.to_string())),
            );
            let sel = Select::new()
                .with_prompt("Label")
                .items(&names)
                .default(0)
                .interact()?;
            if sel > 0 {
                if let Some(id) = labels[sel - 1]["id"].as_str() {
                    label_ids.push(id.to_string());
                }
            }
        }
    }

    // Due date
    let due_date: String = Input::new()
        .with_prompt("Due date (YYYY-MM-DD, +3d, tomorrow, or empty)")
        .allow_empty(true)
        .interact_text()?;

    let confirm = Confirm::new()
        .with_prompt("Create this issue?")
        .default(true)
        .interact()?;

    if !confirm {
        println!("Cancelled.");
        return Ok(());
    }

    let mut input = json!({
        "title": title,
        "teamId": team.id
    });

    if !description.is_empty() {
        input["description"] = json!(description);
    }
    if priority > 0 {
        input["priority"] = json!(priority);
    }
    if let Some(aid) = assignee_id {
        input["assigneeId"] = json!(aid);
    }
    if let Some(sid) = state_id {
        input["stateId"] = json!(sid);
    }
    if !label_ids.is_empty() {
        input["labelIds"] = json!(label_ids);
    }
    if !due_date.is_empty() {
        if let Some(parsed) = crate::dates::parse_due_date(&due_date) {
            input["dueDate"] = json!(parsed);
        }
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
        let identifier = safe_terminal_value(issue["identifier"].as_str().unwrap_or(""));
        let issue_title = safe_terminal_value(issue["title"].as_str().unwrap_or(""));
        println!(
            "\n{} Created issue: {} {}",
            "+".green(),
            identifier.cyan(),
            issue_title
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

async fn list_issues_interactive(client: &LinearClient, team: &Team) -> Result<()> {
    println!("\n{}", "Issues".cyan().bold());

    let query = r#"
        query($teamId: String!) {
            team(id: $teamId) {
                issues(first: 25) {
                    nodes {
                        id
                        identifier
                        title
                        priority
                        state { name }
                    }
                }
            }
        }
    "#;

    let result = client
        .query(query, Some(json!({ "teamId": team.id })))
        .await?;

    let empty = vec![];
    let issues = result["data"]["team"]["issues"]["nodes"]
        .as_array()
        .unwrap_or(&empty);

    if issues.is_empty() {
        println!("No issues found for team {}.", team.name);
        return Ok(());
    }

    let width = display_options().max_width(50);
    let rows: Vec<IssueRow> = issues
        .iter()
        .map(|issue| IssueRow {
            identifier: issue["identifier"].as_str().unwrap_or("").to_string(),
            title: truncate(issue["title"].as_str().unwrap_or(""), width),
            state: issue["state"]["name"].as_str().unwrap_or("-").to_string(),
            priority: priority_to_string(issue["priority"].as_i64()),
        })
        .collect();

    let table = Table::new(rows).to_string();
    println!("{}", table);
    println!("\n{} issues (showing up to 25)", issues.len());

    Ok(())
}

async fn view_issue_interactive(client: &LinearClient) -> Result<()> {
    println!("\n{}", "View Issue".cyan().bold());

    let issue_id: String = Input::new()
        .with_prompt("Issue ID (e.g., LIN-123)")
        .interact_text()?;

    if issue_id.trim().is_empty() {
        println!("Issue ID cannot be empty.");
        return Ok(());
    }

    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                title
                description
                priority
                url
                state { name }
                team { name }
                assignee { name email }
                labels { nodes { name } }
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": issue_id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        println!("Issue not found: {}", issue_id);
        return Ok(());
    }

    let identifier = safe_terminal_value(issue["identifier"].as_str().unwrap_or(""));
    let title = safe_terminal_value(issue["title"].as_str().unwrap_or(""));
    println!("\n{} {}", identifier.cyan().bold(), title.bold());
    println!("{}", "-".repeat(60));

    if let Some(desc) = issue["description"].as_str() {
        if !desc.is_empty() {
            let truncated = truncate(desc, display_options().max_width(200));
            println!("\n{}\n", truncated);
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
        println!("Assignee: {}", safe_terminal_value(assignee));
    } else {
        println!("Assignee: -");
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

    println!(
        "\nURL: {}",
        safe_terminal_value(issue["url"].as_str().unwrap_or("-"))
    );

    Ok(())
}

fn select_team_by_name_or_key(teams: &[Team], value: &str) -> Result<Team> {
    let needle = value.trim().to_lowercase();
    if needle.is_empty() {
        return select_team(teams);
    }
    if let Some(team) = teams.iter().find(|t| {
        t.id.eq_ignore_ascii_case(&needle)
            || t.key.eq_ignore_ascii_case(&needle)
            || t.name.eq_ignore_ascii_case(&needle)
    }) {
        return Ok(team.clone());
    }
    anyhow::bail!(
        "Team not found: '{}'. Use `linear teams list` to see available teams.",
        value
    )
}

async fn update_issue_interactive(client: &LinearClient) -> Result<()> {
    println!("\n{}", "Update Issue".cyan().bold());

    let issue_id: String = Input::new()
        .with_prompt("Issue ID (e.g., LIN-123)")
        .interact_text()?;

    if issue_id.trim().is_empty() {
        println!("Issue ID cannot be empty.");
        return Ok(());
    }

    // First fetch the issue to show current state and get the UUID
    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                title
                description
                priority
                state { id name }
                assignee { id name }
                team {
                    id
                    states { nodes { id name type } }
                    members { nodes { id name } }
                }
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": issue_id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        println!("Issue not found: {}", issue_id);
        return Ok(());
    }

    // Get the UUID for the update mutation
    let issue_uuid = issue["id"].as_str().unwrap_or(&issue_id);
    let current_title = issue["title"].as_str().unwrap_or("");
    let current_description = issue["description"].as_str().unwrap_or("");
    let identifier = issue["identifier"].as_str().unwrap_or("");
    let current_state = issue["state"]["name"].as_str().unwrap_or("Unknown");
    let current_assignee = issue["assignee"]["name"].as_str().unwrap_or("Unassigned");

    println!("\nCurrent: {} {}", identifier.cyan(), current_title);
    println!(
        "Status: {} | Assignee: {}",
        current_state.yellow(),
        current_assignee.dimmed()
    );

    let update_options = vec![
        "Title",
        "Priority",
        "Status",
        "Assignee",
        "Description",
        "Cancel",
    ];
    let selection = Select::new()
        .with_prompt("What to update?")
        .items(&update_options)
        .default(0)
        .interact()?;

    let mut input = json!({});

    match selection {
        0 => {
            // Update title
            let new_title: String = Input::new()
                .with_prompt("New title")
                .with_initial_text(current_title)
                .interact_text()?;

            if !new_title.is_empty() && new_title != current_title {
                input["title"] = json!(new_title);
            }
        }
        1 => {
            // Update priority
            let priority_options = vec!["None", "Urgent", "High", "Normal", "Low"];
            let current_priority = issue["priority"].as_i64().unwrap_or(0) as usize;
            let priority_selection = Select::new()
                .with_prompt("New priority")
                .items(&priority_options)
                .default(current_priority)
                .interact()?;

            input["priority"] = json!(priority_selection);
        }
        2 => {
            // Update status
            let states = issue["team"]["states"]["nodes"].as_array();
            if let Some(states) = states {
                let state_names: Vec<&str> =
                    states.iter().filter_map(|s| s["name"].as_str()).collect();

                if state_names.is_empty() {
                    println!("No states available for this team.");
                    return Ok(());
                }

                // Find current state index
                let current_state_id = issue["state"]["id"].as_str().unwrap_or("");
                let current_idx = states
                    .iter()
                    .position(|s| s["id"].as_str() == Some(current_state_id))
                    .unwrap_or(0);

                let state_selection = Select::new()
                    .with_prompt("New status")
                    .items(&state_names)
                    .default(current_idx)
                    .interact()?;

                if let Some(state_id) = states[state_selection]["id"].as_str() {
                    input["stateId"] = json!(state_id);
                }
            } else {
                println!("Could not fetch team states.");
                return Ok(());
            }
        }
        3 => {
            // Update assignee
            let members = issue["team"]["members"]["nodes"].as_array();
            if let Some(members) = members {
                let mut assignee_names: Vec<&str> = vec!["(Unassign)"];
                assignee_names.extend(members.iter().filter_map(|m| m["name"].as_str()));

                // Find current assignee index
                let current_assignee_id = issue["assignee"]["id"].as_str().unwrap_or("");
                let current_idx = if current_assignee_id.is_empty() {
                    0 // Unassigned
                } else {
                    members
                        .iter()
                        .position(|m| m["id"].as_str() == Some(current_assignee_id))
                        .map(|i| i + 1) // +1 because of "(Unassign)" at index 0
                        .unwrap_or(0)
                };

                let assignee_selection = Select::new()
                    .with_prompt("Assignee")
                    .items(&assignee_names)
                    .default(current_idx)
                    .interact()?;

                if assignee_selection == 0 {
                    input["assigneeId"] = json!(null);
                } else if let Some(member_id) = members[assignee_selection - 1]["id"].as_str() {
                    input["assigneeId"] = json!(member_id);
                }
            } else {
                println!("Could not fetch team members.");
                return Ok(());
            }
        }
        4 => {
            // Update description
            println!("Current description:");
            if current_description.is_empty() {
                println!("  (empty)");
            } else {
                for line in current_description.lines().take(5) {
                    println!("  {}", line.dimmed());
                }
                if current_description.lines().count() > 5 {
                    println!(
                        "  {} more lines...",
                        current_description.lines().count() - 5
                    );
                }
            }

            let new_description: String = Input::new()
                .with_prompt("New description (markdown, single line)")
                .with_initial_text(current_description)
                .allow_empty(true)
                .interact_text()?;

            if new_description != current_description {
                input["description"] = json!(new_description);
            }
        }
        _ => {
            println!("Cancelled.");
            return Ok(());
        }
    }

    if input.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        println!("No changes made.");
        return Ok(());
    }

    let confirm = Confirm::new()
        .with_prompt("Apply changes?")
        .default(true)
        .interact()?;

    if !confirm {
        println!("Cancelled.");
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

    // Use the UUID for the update mutation
    let result = client
        .mutate(mutation, Some(json!({ "id": issue_uuid, "input": input })))
        .await?;

    if result["data"]["issueUpdate"]["success"].as_bool() == Some(true) {
        let updated = &result["data"]["issueUpdate"]["issue"];
        println!(
            "\n{} Updated issue: {} {}",
            "+".green(),
            updated["identifier"].as_str().unwrap_or(""),
            updated["title"].as_str().unwrap_or("")
        );
    } else {
        anyhow::bail!("Failed to update issue");
    }

    Ok(())
}
