use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use dialoguer::{Input, Select};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tabled::{Table, Tabled};

use crate::api::{resolve_team_id, LinearClient};
use crate::display_options;
use crate::output::{
    ensure_non_empty, filter_values, print_json, print_json_owned, sort_values, OutputOptions,
};
use crate::priority::priority_to_string;
use crate::text::truncate;

fn safe_terminal_value(value: &str) -> String {
    crate::text::sanitize_terminal_text(value)
}
/// Issue template structure for creating issues with predefined values
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IssueTemplate {
    /// Template name (used as identifier)
    pub name: String,
    /// Optional prefix to add to issue titles
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_prefix: Option<String>,
    /// Default description for the issue
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Default priority (0=none, 1=urgent, 2=high, 3=normal, 4=low)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_priority: Option<i32>,
    /// Default labels to apply
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_labels: Vec<String>,
    /// Default team name or ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
}

/// Storage for all templates
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TemplateStore {
    pub templates: HashMap<String, IssueTemplate>,
}

#[derive(Subcommand)]
pub enum TemplateCommands {
    /// List available templates
    #[command(alias = "ls")]
    List,
    /// Create a new local template
    #[command(after_help = r#"EXAMPLES:
    linear templates create bug --team ENG --priority 2 --label bug --title-prefix "[Bug]"
    linear tpl create chore --description "Checklist..." --label ops,internal
    linear --dry-run --output json tpl create bug --team ENG --priority 2"#)]
    Create {
        /// Template name
        name: String,
        /// Prefix to add to issue titles
        #[arg(long)]
        title_prefix: Option<String>,
        /// Default issue description
        #[arg(short, long)]
        description: Option<String>,
        /// Default priority (0=none, 1=urgent, 2=high, 3=normal, 4=low)
        #[arg(short = 'p', long = "priority", value_name = "0-4")]
        default_priority: Option<i32>,
        /// Default label to apply. Repeat or comma-separate for multiple labels.
        #[arg(short = 'l', long = "label", value_delimiter = ',')]
        labels: Vec<String>,
        /// Default team name, key, or ID
        #[arg(short, long)]
        team: Option<String>,
    },
    /// Show template details
    #[command(alias = "get")]
    Show {
        /// Template name
        name: String,
    },
    /// Delete a template
    #[command(alias = "rm")]
    Delete {
        /// Template name
        name: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// List Linear workspace templates
    #[command(alias = "remote-ls")]
    RemoteList {
        /// Filter by type: issue, project, document
        #[arg(short = 'T', long = "type")]
        template_type: Option<String>,
    },
    /// Get a Linear workspace template
    RemoteGet {
        /// Template ID
        id: String,
    },
    /// Create a Linear workspace template
    RemoteCreate {
        /// Template name
        #[arg(short, long)]
        name: String,
        /// Template type: issue, project, document
        #[arg(short = 'T', long = "type")]
        template_type: String,
        /// Team key/name/ID (for issue templates)
        #[arg(short, long)]
        team: Option<String>,
        /// Template description
        #[arg(short, long)]
        description: Option<String>,
        /// Template data as JSON string
        #[arg(long)]
        data: Option<String>,
    },
    /// Update a Linear workspace template
    RemoteUpdate {
        /// Template ID
        id: String,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
        /// New template data as JSON
        #[arg(long)]
        data: Option<String>,
    },
    /// Delete a Linear workspace template
    RemoteDelete {
        /// Template ID
        id: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Tabled)]
struct TemplateRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Title Prefix")]
    title_prefix: String,
    #[tabled(rename = "Team")]
    team: String,
    #[tabled(rename = "Priority")]
    priority: String,
    #[tabled(rename = "Labels")]
    labels: String,
}

fn templates_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join("linear-cli");

    fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("templates.json"))
}

pub fn load_templates() -> Result<TemplateStore> {
    let path = templates_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let store: TemplateStore = serde_json::from_str(&content)?;
        Ok(store)
    } else {
        Ok(TemplateStore::default())
    }
}

fn save_templates(store: &TemplateStore) -> Result<()> {
    let path = templates_path()?;
    let content = serde_json::to_string_pretty(store)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn get_template(name: &str) -> Result<Option<IssueTemplate>> {
    let store = load_templates()?;
    Ok(store.templates.get(name).cloned())
}

pub async fn handle(cmd: TemplateCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        TemplateCommands::List => list_templates(output),
        TemplateCommands::Create {
            name,
            title_prefix,
            description,
            default_priority,
            labels,
            team,
        } => create_template(
            &name,
            title_prefix,
            description,
            default_priority,
            labels,
            team,
            output,
        ),
        TemplateCommands::Show { name } => show_template(&name, output),
        TemplateCommands::Delete { name, force } => delete_template(&name, force, output),
        TemplateCommands::RemoteList { template_type } => {
            remote_list_templates(template_type.as_deref(), output).await
        }
        TemplateCommands::RemoteGet { id } => remote_get_template(&id, output).await,
        TemplateCommands::RemoteCreate {
            name,
            template_type,
            team,
            description,
            data,
        } => remote_create_template(&name, &template_type, team, description, data, output).await,
        TemplateCommands::RemoteUpdate {
            id,
            name,
            description,
            data,
        } => remote_update_template(&id, name, description, data, output).await,
        TemplateCommands::RemoteDelete { id, force } => {
            remote_delete_template(&id, force, output).await
        }
    }
}

fn list_templates(output: &OutputOptions) -> Result<()> {
    let store = load_templates()?;

    if store.templates.is_empty() {
        ensure_non_empty(&[], output)?;
        println!("No templates found.");
        println!("\nCreate one with: linear-cli templates create <name>");
        return Ok(());
    }

    let mut templates: Vec<serde_json::Value> =
        store.templates.values().map(|t| json!(t)).collect();

    filter_values(&mut templates, &output.filters);

    if let Some(sort_key) = output.json.sort.as_deref() {
        sort_values(&mut templates, sort_key, output.json.order);
    } else {
        templates.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
    }

    if output.is_json() || output.has_template() {
        print_json_owned(serde_json::json!(templates), output)?;
        return Ok(());
    }

    ensure_non_empty(&templates, output)?;
    if templates.is_empty() {
        println!("No templates found.");
        return Ok(());
    }

    let width = display_options().max_width(30);
    let rows: Vec<TemplateRow> = templates
        .iter()
        .map(|t| TemplateRow {
            name: truncate(
                safe_terminal_value(t["name"].as_str().unwrap_or("")).as_str(),
                width,
            ),
            title_prefix: truncate(
                safe_terminal_value(t["title_prefix"].as_str().unwrap_or("-")).as_str(),
                width,
            ),
            team: truncate(
                safe_terminal_value(t["team"].as_str().unwrap_or("-")).as_str(),
                width,
            ),
            priority: priority_to_string(t["default_priority"].as_i64()),
            labels: {
                let labels = t["default_labels"].as_array().cloned().unwrap_or_default();
                if labels.is_empty() {
                    "-".to_string()
                } else {
                    let joined = labels
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    truncate(
                        safe_terminal_value(&joined).as_str(),
                        display_options().max_width(40),
                    )
                }
            },
        })
        .collect();

    let table = Table::new(rows).to_string();
    println!("{}", table);
    println!("\n{} templates", store.templates.len());

    Ok(())
}

fn create_template(
    name: &str,
    title_prefix: Option<String>,
    description: Option<String>,
    default_priority: Option<i32>,
    labels: Vec<String>,
    team: Option<String>,
    output: &OutputOptions,
) -> Result<()> {
    let mut store = load_templates()?;

    if store.templates.contains_key(name) {
        anyhow::bail!("Template already exists. Delete it first or choose a different name.");
    }

    if let Some(priority) = default_priority {
        if !(0..=4).contains(&priority) {
            anyhow::bail!(
                "Invalid priority: {}. Use 0=none, 1=urgent, 2=high, 3=normal, or 4=low.",
                priority
            );
        }
    }

    let has_flag_values = title_prefix.is_some()
        || description.is_some()
        || default_priority.is_some()
        || !labels.is_empty()
        || team.is_some();

    if has_flag_values || output.is_json() || output.has_template() || output.dry_run {
        let default_priority = default_priority.filter(|&p| p != 0);
        let default_labels: Vec<String> = labels
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let template = IssueTemplate {
            name: name.to_string(),
            title_prefix,
            description,
            default_priority,
            default_labels,
            team,
        };

        if output.dry_run {
            let payload = json!({
                "dry_run": true,
                "would_create": template,
            });
            if output.is_json() || output.has_template() {
                print_json_owned(payload, output)?;
            } else {
                println!(
                    "Dry run: would create template '{}'.",
                    safe_terminal_value(name)
                );
            }
            return Ok(());
        }

        store.templates.insert(name.to_string(), template);
        save_templates(&store)?;

        if output.is_json() || output.has_template() {
            print_json_owned(json!(store.templates.get(name)), output)?;
            return Ok(());
        }

        println!(
            "{} Template created: {}",
            "+".green(),
            safe_terminal_value(name).cyan()
        );
        return Ok(());
    }

    println!("{} Creating template: {}", "+".green(), name.cyan());
    println!("Press Enter to skip optional fields.\n");

    let title_prefix: String = Input::new()
        .with_prompt("Title prefix (e.g., [Bug], [Feature])")
        .allow_empty(true)
        .interact_text()?;

    let title_prefix = if title_prefix.is_empty() {
        None
    } else {
        Some(title_prefix)
    };

    let description: String = Input::new()
        .with_prompt("Default description")
        .allow_empty(true)
        .interact_text()?;

    let description = if description.is_empty() {
        None
    } else {
        Some(description)
    };

    let priority_options = vec!["None", "Urgent (1)", "High (2)", "Normal (3)", "Low (4)"];
    let priority_selection = Select::new()
        .with_prompt("Default priority")
        .items(&priority_options)
        .default(0)
        .interact()?;

    let default_priority = match priority_selection {
        0 => None,
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(4),
        _ => None,
    };

    let team: String = Input::new()
        .with_prompt("Default team (name or key)")
        .allow_empty(true)
        .interact_text()?;

    let team = if team.is_empty() { None } else { Some(team) };

    let labels_input: String = Input::new()
        .with_prompt("Default labels (comma-separated)")
        .allow_empty(true)
        .interact_text()?;

    let default_labels: Vec<String> = if labels_input.is_empty() {
        vec![]
    } else {
        labels_input
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let template = IssueTemplate {
        name: name.to_string(),
        title_prefix,
        description,
        default_priority,
        default_labels,
        team,
    };

    store.templates.insert(name.to_string(), template);
    save_templates(&store)?;

    if output.is_json() || output.has_template() {
        print_json_owned(json!(store.templates.get(name)), output)?;
        return Ok(());
    }

    println!("\n{} Template created successfully!", "+".green());

    Ok(())
}

fn show_template(name: &str, output: &OutputOptions) -> Result<()> {
    let store = load_templates()?;

    let template = store
        .templates
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Template not found"))?;

    if output.is_json() || output.has_template() {
        print_json_owned(json!(template), output)?;
        return Ok(());
    }

    println!(
        "{} {}",
        "Template:".bold(),
        safe_terminal_value(&template.name).cyan().bold()
    );
    println!("{}", "-".repeat(40));

    println!(
        "Title Prefix: {}",
        template
            .title_prefix
            .as_deref()
            .map(safe_terminal_value)
            .unwrap_or_else(|| "-".to_string())
    );

    if let Some(desc) = &template.description {
        println!("Description:  {}", safe_terminal_value(desc));
    } else {
        println!("Description:  -");
    }

    println!(
        "Priority:     {}",
        priority_to_string(template.default_priority.map(|p| p as i64))
    );

    println!(
        "Team:         {}",
        template
            .team
            .as_deref()
            .map(safe_terminal_value)
            .unwrap_or_else(|| "-".to_string())
    );

    if template.default_labels.is_empty() {
        println!("Labels:       -");
    } else {
        println!(
            "Labels:       {}",
            safe_terminal_value(&template.default_labels.join(", "))
        );
    }

    Ok(())
}

fn delete_template(name: &str, force: bool, output: &OutputOptions) -> Result<()> {
    let mut store = load_templates()?;

    if !store.templates.contains_key(name) {
        anyhow::bail!("Template not found");
    }

    if !force && !crate::is_yes() {
        anyhow::bail!(
            "Delete requires --force flag. Use: linear templates delete {} --force",
            name
        );
    }

    store.templates.remove(name);
    save_templates(&store)?;

    if output.is_json() || output.has_template() {
        print_json_owned(json!({ "deleted": name }), output)?;
        return Ok(());
    }

    println!("{} Template deleted", "+".green());

    Ok(())
}

// --- Remote (Linear workspace) templates ---

#[derive(Tabled)]
struct RemoteTemplateRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    template_type: String,
    #[tabled(rename = "Team")]
    team: String,
    #[tabled(rename = "Created")]
    created: String,
    #[tabled(rename = "ID")]
    id: String,
}

async fn remote_list_templates(template_type: Option<&str>, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    let query = r#"
        query {
            templates {
                nodes {
                    id
                    name
                    type
                    description
                    team { name }
                    createdAt
                }
            }
        }
    "#;

    let result = client.query(query, None).await?;
    let nodes = result["data"]["templates"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Client-side filter by type
    let mut filtered: Vec<serde_json::Value> = if let Some(t) = template_type {
        let t_lower = t.to_lowercase();
        nodes
            .into_iter()
            .filter(|n| {
                n["type"]
                    .as_str()
                    .map(|v| v.to_lowercase() == t_lower)
                    .unwrap_or(false)
            })
            .collect()
    } else {
        nodes
    };

    if output.is_json() || output.has_template() {
        print_json_owned(json!(filtered), output)?;
        return Ok(());
    }

    filter_values(&mut filtered, &output.filters);

    if let Some(sort_key) = output.json.sort.as_deref() {
        sort_values(&mut filtered, sort_key, output.json.order);
    }

    ensure_non_empty(&filtered, output)?;
    if filtered.is_empty() {
        println!("No workspace templates found.");
        return Ok(());
    }

    let width = display_options().max_width(30);
    let rows: Vec<RemoteTemplateRow> = filtered
        .iter()
        .map(|t| RemoteTemplateRow {
            name: truncate(
                safe_terminal_value(t["name"].as_str().unwrap_or("")).as_str(),
                width,
            ),
            template_type: t["type"].as_str().unwrap_or("-").to_string(),
            team: truncate(
                safe_terminal_value(t["team"]["name"].as_str().unwrap_or("-")).as_str(),
                display_options().max_width(20),
            ),
            created: t["createdAt"]
                .as_str()
                .map(|s| s.get(..10).unwrap_or(s).to_string())
                .unwrap_or_else(|| "-".to_string()),
            id: t["id"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    let rows_len = rows.len();
    let table = Table::new(rows).to_string();
    println!("{}", table);
    println!("\n{} workspace templates", rows_len);

    Ok(())
}

async fn remote_get_template(id: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    let query = r#"
        query($id: String!) {
            template(id: $id) {
                id
                name
                type
                description
                templateData
                team { name key }
                creator { name }
                createdAt
                updatedAt
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": id }))).await?;
    let raw = &result["data"]["template"];

    if raw.is_null() {
        anyhow::bail!("Template not found: {}", id);
    }

    if output.is_json() || output.has_template() {
        print_json(raw, output)?;
        return Ok(());
    }

    println!(
        "{} {}",
        "Template:".bold(),
        safe_terminal_value(raw["name"].as_str().unwrap_or(""))
            .cyan()
            .bold()
    );
    println!("{}", "-".repeat(40));
    println!(
        "Type: {}",
        safe_terminal_value(raw["type"].as_str().unwrap_or("-"))
    );
    if let Some(desc) = raw["description"].as_str() {
        if !desc.is_empty() {
            println!("Description: {}", safe_terminal_value(desc));
        }
    }
    if let Some(team_name) = raw["team"]["name"].as_str() {
        let team_name = safe_terminal_value(team_name);
        let team_key = safe_terminal_value(raw["team"]["key"].as_str().unwrap_or(""));
        println!("Team: {} ({})", team_name, team_key);
    }
    if let Some(creator) = raw["creator"]["name"].as_str() {
        println!("Creator: {}", safe_terminal_value(creator));
    }
    println!(
        "Created: {}",
        raw["createdAt"]
            .as_str()
            .map(|s| s.get(..10).unwrap_or(s))
            .unwrap_or("-")
    );
    println!("ID: {}", id);

    if !raw["templateData"].is_null() {
        println!(
            "\nTemplate Data:\n{}",
            serde_json::to_string_pretty(&raw["templateData"]).unwrap_or_default()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_terminal_value_removes_escape_sequences() {
        assert_eq!(safe_terminal_value("bad\u{1b}[31mname\u{1b}[0m"), "badname");
    }
}

async fn remote_create_template(
    name: &str,
    template_type: &str,
    team: Option<String>,
    description: Option<String>,
    data: Option<String>,
    output: &OutputOptions,
) -> Result<()> {
    let client = LinearClient::new()?;

    let mut input = json!({
        "name": name,
        "type": template_type,
    });

    if let Some(t) = team {
        let team_id = resolve_team_id(&client, &t, &output.cache).await?;
        input["teamId"] = json!(team_id);
    }
    if let Some(d) = &description {
        input["description"] = json!(d);
    }
    if let Some(d) = &data {
        let parsed: serde_json::Value = serde_json::from_str(d)
            .context("Invalid JSON for --data. Provide valid JSON string.")?;
        input["templateData"] = parsed;
    }

    let mutation = r#"
        mutation($input: TemplateCreateInput!) {
            templateCreate(input: $input) {
                success
                template { id name }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "input": input })))
        .await?;

    if result["data"]["templateCreate"]["success"].as_bool() == Some(true) {
        let tmpl = &result["data"]["templateCreate"]["template"];
        if output.is_json() || output.has_template() {
            print_json(tmpl, output)?;
            return Ok(());
        }
        println!(
            "{} Template created: {}",
            "+".green(),
            safe_terminal_value(tmpl["name"].as_str().unwrap_or(""))
        );
        println!("  ID: {}", tmpl["id"].as_str().unwrap_or(""));
    } else {
        anyhow::bail!("Failed to create template");
    }

    Ok(())
}

async fn remote_update_template(
    id: &str,
    name: Option<String>,
    description: Option<String>,
    data: Option<String>,
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
    if let Some(d) = &data {
        let parsed: serde_json::Value = serde_json::from_str(d)
            .context("Invalid JSON for --data. Provide valid JSON string.")?;
        input["templateData"] = parsed;
    }

    if input.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        println!("No updates specified.");
        return Ok(());
    }

    let mutation = r#"
        mutation($id: String!, $input: TemplateUpdateInput!) {
            templateUpdate(id: $id, input: $input) {
                success
                template { id name }
            }
        }
    "#;

    let result = client
        .mutate(mutation, Some(json!({ "id": id, "input": input })))
        .await?;

    if result["data"]["templateUpdate"]["success"].as_bool() == Some(true) {
        let tmpl = &result["data"]["templateUpdate"]["template"];
        if output.is_json() || output.has_template() {
            print_json(tmpl, output)?;
            return Ok(());
        }
        println!("{} Template updated", "+".green());
    } else {
        anyhow::bail!("Failed to update template");
    }

    Ok(())
}

async fn remote_delete_template(id: &str, force: bool, output: &OutputOptions) -> Result<()> {
    if !force && !crate::is_yes() {
        anyhow::bail!(
            "Delete requires --force flag. Use: linear templates remote-delete {} --force",
            id
        );
    }

    let client = LinearClient::new()?;

    let mutation = r#"
        mutation($id: String!) {
            templateDelete(id: $id) {
                success
            }
        }
    "#;

    let result = client.mutate(mutation, Some(json!({ "id": id }))).await?;

    let success = result["data"]["templateDelete"]["success"]
        .as_bool()
        .unwrap_or(false);

    if success {
        if output.is_json() || output.has_template() {
            print_json_owned(json!({ "deleted": id }), output)?;
            return Ok(());
        }
        println!("{} Workspace template deleted", "+".green());
    } else {
        anyhow::bail!("Failed to delete template {}", id);
    }

    Ok(())
}
