use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use colored::Colorize;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

use crate::api::LinearClient;
use crate::display_options;
use crate::output::{print_json, OutputOptions};
use crate::text::truncate;
use crate::vcs::{generate_branch_name, git_branch_exists, run_git_command, validate_branch_name};

/// Version control system type
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Vcs {
    /// Git version control
    Git,
    /// Jujutsu (jj) version control
    Jj,
}

impl std::fmt::Display for Vcs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Vcs::Git => write!(f, "git"),
            Vcs::Jj => write!(f, "jj"),
        }
    }
}

#[derive(Subcommand)]
pub enum GitCommands {
    /// Checkout a branch for an issue (creates if doesn't exist)
    #[command(after_help = r#"EXAMPLES:
    linear git checkout LIN-123                # Checkout issue branch
    linear g checkout LIN-123 -b feature/fix   # Use custom branch name
    linear g checkout LIN-123 --vcs jj         # Use Jujutsu VCS"#)]
    Checkout {
        /// Issue identifier (e.g., "LIN-123") or ID
        issue: String,
        /// Custom branch name (optional, uses issue's branch name by default)
        #[arg(short, long)]
        branch: Option<String>,
        /// Version control system to use (auto-detected by default)
        #[arg(long, value_enum)]
        vcs: Option<Vcs>,
    },
    /// Show the branch name for an issue
    #[command(after_help = r#"EXAMPLES:
    linear git branch LIN-123                  # Show branch name
    linear g branch LIN-123 --vcs git          # Show git branch name"#)]
    Branch {
        /// Issue identifier (e.g., "LIN-123") or ID
        issue: String,
        /// Version control system to use (auto-detected by default)
        #[arg(long, value_enum)]
        vcs: Option<Vcs>,
    },
    /// Create a branch for an issue without checking out
    #[command(after_help = r#"EXAMPLES:
    linear git create LIN-123                  # Create branch
    linear g create LIN-123 -b custom-branch   # Create with custom name"#)]
    Create {
        /// Issue identifier (e.g., "LIN-123") or ID
        issue: String,
        /// Custom branch name (optional)
        #[arg(short, long)]
        branch: Option<String>,
        /// Version control system to use (auto-detected by default)
        #[arg(long, value_enum)]
        vcs: Option<Vcs>,
    },
    /// Show commits with Linear issue trailers (jj only)
    #[command(after_help = r#"EXAMPLES:
    linear git commits                         # Show last 10 commits
    linear g commits -l 20                     # Show last 20 commits"#)]
    Commits {
        /// Number of commits to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Version control system to use (auto-detected by default)
        #[arg(long, value_enum)]
        vcs: Option<Vcs>,
    },
    /// Show the Linear review URL for an issue's pull request(s)
    #[command(after_help = r#"EXAMPLES:
    linear git review-url LIN-123               # Print the review URL(s)
    linear g review-url LIN-123 -o json         # Include PR number, state, GitHub URL

NOTE: Linear only exposes a pull request's review slug for PRs it has linked to
an agent session, so a PR opened outside that flow has no review URL to resolve."#)]
    ReviewUrl {
        /// Issue identifier (e.g., "LIN-123") or ID
        issue: String,
    },
    /// Create a GitHub PR from a Linear issue
    #[command(after_help = r#"EXAMPLES:
    linear git pr LIN-123                      # Create PR for issue
    linear g pr LIN-123 --draft                # Create draft PR
    linear g pr LIN-123 -B develop             # Merge into develop
    linear g pr LIN-123 --web                  # Open PR in browser"#)]
    Pr {
        /// Issue identifier (e.g., "LIN-123") or ID
        issue: String,
        /// Base branch to merge into (default: main)
        #[arg(short = 'B', long, default_value = "main")]
        base: String,
        /// Create a draft PR
        #[arg(short, long)]
        draft: bool,
        /// Open the PR in the browser after creation
        #[arg(short, long)]
        web: bool,
    },
}

/// Detect which VCS is being used in the current directory
fn detect_vcs() -> Result<Vcs> {
    // First check for .jj directory
    if Path::new(".jj").exists() {
        return Ok(Vcs::Jj);
    }

    // Try running jj status to see if we're in a jj repo
    if let Ok(output) = Command::new("jj").args(["status"]).output() {
        if output.status.success() {
            return Ok(Vcs::Jj);
        }
    }

    // Check for .git directory
    if Path::new(".git").exists() {
        return Ok(Vcs::Git);
    }

    // Try running git status
    if let Ok(output) = Command::new("git").args(["status"]).output() {
        if output.status.success() {
            return Ok(Vcs::Git);
        }
    }

    anyhow::bail!("Not in a git or jj repository")
}

/// Get the VCS to use, either from the flag or auto-detected
fn get_vcs(vcs_flag: Option<Vcs>) -> Result<Vcs> {
    match vcs_flag {
        Some(vcs) => Ok(vcs),
        None => detect_vcs(),
    }
}

pub async fn handle(cmd: GitCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        GitCommands::ReviewUrl { issue } => show_review_url(&issue, output).await,
        GitCommands::Checkout { issue, branch, vcs } => {
            let vcs = get_vcs(vcs)?;
            checkout_issue(&issue, branch, vcs).await
        }
        GitCommands::Branch { issue, vcs } => {
            let vcs = get_vcs(vcs)?;
            show_branch(&issue, vcs).await
        }
        GitCommands::Create { issue, branch, vcs } => {
            let vcs = get_vcs(vcs)?;
            create_branch(&issue, branch, vcs).await
        }
        GitCommands::Commits { limit, vcs } => {
            let vcs = get_vcs(vcs)?;
            show_commits(limit, vcs).await
        }
        GitCommands::Pr {
            issue,
            base,
            draft,
            web,
        } => create_pr(&issue, &base, draft, web).await,
    }
}

/// Build the review entries for an issue from a `review-url` query response.
///
/// `PullRequest.slugId` is the only public field carrying the slug in a review
/// URL, and it is reachable only through the agent sessions attached to an issue,
/// so an issue can legitimately resolve to zero entries. One pull request can be
/// linked by more than one session, hence the de-duplication by slug.
fn review_entries(url_key: &str, issue: &Value) -> Vec<Value> {
    let mut seen: Vec<String> = Vec::new();
    let mut entries = Vec::new();

    let sessions = issue["agentSessions"]["nodes"].as_array();
    for session in sessions.into_iter().flatten() {
        let links = session["pullRequests"]["nodes"].as_array();
        for link in links.into_iter().flatten() {
            let pr = &link["pullRequest"];
            let Some(slug) = pr["slugId"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            if seen.iter().any(|s| s == slug) {
                continue;
            }
            seen.push(slug.to_string());
            entries.push(json!({
                "reviewUrl": format!("https://linear.app/{}/review/{}", url_key, slug),
                "number": pr["number"],
                "status": pr["status"],
                "url": pr["url"],
                "title": pr["title"],
            }));
        }
    }

    entries
}

async fn show_review_url(issue_id: &str, output: &OutputOptions) -> Result<()> {
    let client = LinearClient::new()?;

    let query = r#"
        query($id: String!) {
            organization { urlKey }
            issue(id: $id) {
                identifier
                agentSessions {
                    nodes {
                        pullRequests {
                            nodes {
                                pullRequest { slugId url number status title }
                            }
                        }
                    }
                }
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": issue_id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", issue_id);
    }

    let url_key = result["data"]["organization"]["urlKey"]
        .as_str()
        .unwrap_or_default();
    let entries = review_entries(url_key, issue);

    if entries.is_empty() {
        anyhow::bail!(
            "No review URL for {}: Linear exposes a pull request's review slug only \
             for PRs linked to an agent session, and this issue has none. Use the \
             GitHub PR URL instead.",
            issue["identifier"].as_str().unwrap_or(issue_id)
        );
    }

    if output.is_json() || output.has_template() {
        return print_json(&json!(entries), output);
    }

    for entry in &entries {
        println!("{}", entry["reviewUrl"].as_str().unwrap_or_default());
    }

    Ok(())
}

async fn get_issue_info(issue_id: &str) -> Result<(String, String, String, String)> {
    let client = LinearClient::new()?;

    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                title
                branchName
                url
            }
        }
    "#;

    let result = client.query(query, Some(json!({ "id": issue_id }))).await?;
    let issue = &result["data"]["issue"];

    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", issue_id);
    }

    let identifier = issue["identifier"].as_str().unwrap_or("").to_string();
    let title = issue["title"].as_str().unwrap_or("").to_string();
    let branch_name = issue["branchName"].as_str().unwrap_or("").to_string();
    let url = issue["url"].as_str().unwrap_or("").to_string();

    Ok((identifier, title, branch_name, url))
}

/// Extract Linear issue ID from commit message
fn extract_linear_issue(message: &str) -> Option<String> {
    // Try Linear-Issue: trailer first
    if let Some(line) = message.lines().find(|l| l.starts_with("Linear-Issue:")) {
        return line
            .strip_prefix("Linear-Issue:")
            .map(|s| s.trim().to_string());
    }

    // Try [XXX-123] pattern in subject
    let re_bracket = regex::Regex::new(r"\[([A-Z]+-\d+)\]").ok()?;
    if let Some(caps) = re_bracket.captures(message) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    // Try linear.app URL
    if let Some(pos) = message.find("linear.app/") {
        // Extract issue ID from URL like linear.app/team/issue/XXX-123
        let after = &message[pos..];
        let re_url = regex::Regex::new(r"([A-Z]+-\d+)").ok()?;
        if let Some(caps) = re_url.captures(after) {
            return caps.get(1).map(|m| m.as_str().to_string());
        }
    }

    None
}

fn run_jj_command(args: &[&str]) -> Result<String> {
    let output = Command::new("jj").args(args).output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Jujutsu command failed: {}", stderr.trim());
    }
}

fn branch_exists(branch: &str, vcs: Vcs) -> bool {
    match vcs {
        Vcs::Git => git_branch_exists(branch),
        Vcs::Jj => {
            if validate_branch_name(branch).is_err() {
                return false;
            }
            // In jj, check if bookmark exists
            run_jj_command(&["bookmark", "list", branch])
                .is_ok_and(|output| output.lines().any(|line| line.starts_with(branch)))
        }
    }
}

fn get_current_branch(vcs: Vcs) -> Result<String> {
    match vcs {
        Vcs::Git => run_git_command(&["rev-parse", "--abbrev-ref", "HEAD"]),
        Vcs::Jj => {
            // Get the current change description or ID
            run_jj_command(&["log", "-r", "@", "--no-graph", "-T", "change_id.short()"])
        }
    }
}

/// Generate the commit description with Linear issue trailer
fn generate_jj_description(identifier: &str, title: &str, url: &str) -> String {
    format!(
        "{}: {}\n\nLinear-Issue: {}\nLinear-URL: {}",
        identifier, title, identifier, url
    )
}

async fn checkout_issue(issue_id: &str, custom_branch: Option<String>, vcs: Vcs) -> Result<()> {
    let (identifier, title, linear_branch, url) = get_issue_info(issue_id).await?;
    let title_width = display_options().max_width(50);
    let branch_name = if let Some(custom_branch) = custom_branch {
        validate_branch_name(&custom_branch)?;
        custom_branch
    } else if !linear_branch.is_empty() && validate_branch_name(&linear_branch).is_ok() {
        linear_branch
    } else {
        generate_branch_name(&identifier, &title)
    };

    println!(
        "{} {} {}",
        identifier.cyan(),
        truncate(&title, title_width).dimmed(),
        format!("({})", vcs).dimmed()
    );

    match vcs {
        Vcs::Git => {
            if branch_exists(&branch_name, vcs) {
                // Branch exists, just checkout
                println!("Checking out existing branch: {}", branch_name.green());
                run_git_command(&["checkout", &branch_name])?;
            } else {
                // Create and checkout new branch
                println!("Creating and checking out branch: {}", branch_name.green());
                run_git_command(&["checkout", "-b", &branch_name])?;
            }

            let current = get_current_branch(vcs)?;
            println!("{} Now on branch: {}", "+".green(), current);
        }
        Vcs::Jj => {
            // For jj, we create a new change with the issue info in the description
            let description = generate_jj_description(&identifier, &title, &url);

            if branch_exists(&branch_name, vcs) {
                // Bookmark exists, switch to it
                println!("Switching to existing bookmark: {}", branch_name.green());
                run_jj_command(&["edit", &branch_name])?;
            } else {
                // Create a new change with description
                println!("Creating new change for issue: {}", identifier.green());
                run_jj_command(&["new", "-m", &description])?;

                // Create a bookmark for the branch name
                println!("Creating bookmark: {}", branch_name.green());
                run_jj_command(&["bookmark", "create", &branch_name])?;
            }

            let current = get_current_branch(vcs)?;
            println!("{} Now on change: {}", "+".green(), current);
        }
    }

    Ok(())
}

async fn show_branch(issue_id: &str, vcs: Vcs) -> Result<()> {
    let (identifier, title, linear_branch, url) = get_issue_info(issue_id).await?;
    let title_width = display_options().max_width(50);

    println!(
        "{} {} {}",
        identifier.cyan().bold(),
        truncate(&title, title_width),
        format!("({})", vcs).dimmed()
    );
    println!("{}", "-".repeat(50));

    if !linear_branch.is_empty() {
        println!("Linear branch: {}", linear_branch.green());
    }

    let generated = generate_branch_name(&identifier, &title);
    println!("Generated:     {}", generated.yellow());
    println!("Issue URL:     {}", url.blue());

    match vcs {
        Vcs::Git => {
            // Check if either branch exists locally
            if branch_exists(&linear_branch, vcs) {
                println!("\n{} Linear branch exists locally", "+".green());
            } else if branch_exists(&generated, vcs) {
                println!("\n{} Generated branch exists locally", "+".green());
            } else {
                println!("\n{} No local branch found for this issue", "!".yellow());
            }
        }
        Vcs::Jj => {
            // Check if bookmark exists
            if branch_exists(&linear_branch, vcs) {
                println!("\n{} Linear bookmark exists", "+".green());
            } else if branch_exists(&generated, vcs) {
                println!("\n{} Generated bookmark exists", "+".green());
            } else {
                println!("\n{} No bookmark found for this issue", "!".yellow());
            }
        }
    }

    Ok(())
}

async fn create_branch(issue_id: &str, custom_branch: Option<String>, vcs: Vcs) -> Result<()> {
    let (identifier, title, linear_branch, url) = get_issue_info(issue_id).await?;
    let title_width = display_options().max_width(50);

    let branch_name = if let Some(custom_branch) = custom_branch {
        validate_branch_name(&custom_branch)?;
        custom_branch
    } else if !linear_branch.is_empty() && validate_branch_name(&linear_branch).is_ok() {
        linear_branch
    } else {
        generate_branch_name(&identifier, &title)
    };

    println!(
        "{} {} {}",
        identifier.cyan(),
        truncate(&title, title_width).dimmed(),
        format!("({})", vcs).dimmed()
    );

    match vcs {
        Vcs::Git => {
            if branch_exists(&branch_name, vcs) {
                println!("{} Branch already exists: {}", "!".yellow(), branch_name);
                return Ok(());
            }

            // Create branch without checking out
            run_git_command(&["branch", &branch_name])?;
            println!("{} Created branch: {}", "+".green(), branch_name);
        }
        Vcs::Jj => {
            if branch_exists(&branch_name, vcs) {
                println!("{} Bookmark already exists: {}", "!".yellow(), branch_name);
                return Ok(());
            }

            // Create a new change with description and bookmark
            let description = generate_jj_description(&identifier, &title, &url);
            run_jj_command(&["new", "-m", &description])?;
            run_jj_command(&["bookmark", "create", &branch_name])?;

            // Go back to original change
            run_jj_command(&["prev"])?;

            println!("{} Created bookmark: {}", "+".green(), branch_name);
        }
    }

    Ok(())
}

async fn show_commits(limit: usize, vcs: Vcs) -> Result<()> {
    match vcs {
        Vcs::Git => {
            let subj_width = display_options().max_width(60);
            println!("{}", "Commits with Linear references:".cyan().bold());
            println!("{}", "-".repeat(50));

            // Get recent commits with their full messages
            let limit_str = limit.to_string();
            let output =
                run_git_command(&["log", &format!("-{}", limit_str), "--format=%H|%s|%b%x00"])?;

            for entry in output.split('\0') {
                if entry.trim().is_empty() {
                    continue;
                }

                let parts: Vec<&str> = entry.splitn(3, '|').collect();
                if parts.len() < 2 {
                    continue;
                }

                let hash = &parts[0][..8]; // Short hash
                let subject = parts[1];
                let body = parts.get(2).unwrap_or(&"");

                // Check for Linear references in subject or body
                let full_message = format!("{} {}", subject, body);
                let has_linear_ref = full_message.contains("Linear-Issue:")
                    || full_message.contains("Linear-URL:")
                    || full_message.contains("linear.app/")
                    || subject.contains('[') && subject.contains('-'); // [LIN-123] pattern

                if has_linear_ref {
                    // Try to extract issue ID
                    let issue_id = extract_linear_issue(&full_message);
                    if let Some(id) = issue_id {
                        let subject = truncate(subject, subj_width);
                        println!(
                            "{} {} {}",
                            hash.yellow(),
                            subject,
                            format!("[{}]", id).cyan()
                        );
                    } else {
                        let subject = truncate(subject, subj_width);
                        println!("{} {}", hash.yellow(), subject);
                    }
                } else {
                    let subject = truncate(subject, subj_width);
                    println!("{} {}", hash.dimmed(), subject);
                }
            }

            Ok(())
        }
        Vcs::Jj => {
            let desc_width = display_options().max_width(60);
            println!("{}", "Commits with Linear issue trailers:".cyan().bold());
            println!("{}", "-".repeat(50));

            // Get commits with their descriptions
            let limit_str = limit.to_string();
            let output = run_jj_command(&[
                "log",
                "-r",
                &format!("ancestors(@, {})", limit_str),
                "--no-graph",
                "-T",
                r#"change_id.short() ++ " " ++ description.first_line() ++ "\n""#,
            ])?;

            // Parse and display commits, highlighting those with Linear trailers
            for line in output.lines() {
                if line.trim().is_empty() {
                    continue;
                }

                let parts: Vec<&str> = line.splitn(2, ' ').collect();
                let (change_id, description) = if parts.len() == 2 {
                    (parts[0], parts[1])
                } else {
                    (parts[0], "")
                };

                // Check if this commit has Linear trailers
                let full_desc =
                    run_jj_command(&["log", "-r", change_id, "--no-graph", "-T", "description"])?;

                let has_linear_trailer =
                    full_desc.contains("Linear-Issue:") || full_desc.contains("Linear-URL:");

                if has_linear_trailer {
                    // Extract the Linear issue ID
                    let issue_id = full_desc
                        .lines()
                        .find(|l| l.starts_with("Linear-Issue:"))
                        .and_then(|l| l.strip_prefix("Linear-Issue:"))
                        .map(|s| s.trim())
                        .unwrap_or("");

                    println!(
                        "{} {} {}",
                        change_id.yellow(),
                        truncate(description, desc_width),
                        format!("[{}]", issue_id).cyan()
                    );
                } else {
                    println!(
                        "{} {}",
                        change_id.dimmed(),
                        truncate(description, desc_width)
                    );
                }
            }

            Ok(())
        }
    }
}

fn run_gh_command(args: &[&str]) -> Result<String> {
    let output = Command::new("gh").args(args).output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh command failed: {}", stderr.trim());
    }
}

async fn create_pr(issue_id: &str, base: &str, draft: bool, web: bool) -> Result<()> {
    let (identifier, title, _branch_name, url) = get_issue_info(issue_id).await?;
    let title_width = display_options().max_width(60);

    let pr_title = format!("[{}] {}", identifier, title);
    let pr_body = format!("Linear: {}", url);

    println!(
        "{} {}",
        identifier.cyan(),
        truncate(&title, title_width).dimmed()
    );
    println!(
        "Creating PR with title: {}",
        truncate(&pr_title, title_width).green()
    );

    let mut args = vec![
        "pr", "create", "--title", &pr_title, "--body", &pr_body, "--base", base,
    ];

    if draft {
        args.push("--draft");
    }

    if web {
        args.push("--web");
    }

    let result = run_gh_command(&args)?;

    if !result.is_empty() {
        println!("{} PR created: {}", "+".green(), result);
    } else {
        println!("{} PR created successfully!", "+".green());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue_with_sessions(sessions: Value) -> Value {
        json!({ "identifier": "LIN-123", "agentSessions": { "nodes": sessions } })
    }

    fn pr_link(slug: &str, number: u64) -> Value {
        json!({ "pullRequest": {
            "slugId": slug,
            "number": number,
            "status": "open",
            "url": format!("https://github.com/acme/app/pull/{}", number),
            "title": "Fix the thing"
        }})
    }

    #[test]
    fn test_review_entries_builds_review_url_from_slug() {
        let issue = issue_with_sessions(json!([
            { "pullRequests": { "nodes": [pr_link("7ffd27854fd2", 183)] } }
        ]));

        let entries = review_entries("acme", &issue);

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]["reviewUrl"],
            "https://linear.app/acme/review/7ffd27854fd2"
        );
        assert_eq!(entries[0]["number"], 183);
        assert_eq!(entries[0]["url"], "https://github.com/acme/app/pull/183");
    }

    #[test]
    fn test_review_entries_dedupes_a_pr_linked_by_several_sessions() {
        let issue = issue_with_sessions(json!([
            { "pullRequests": { "nodes": [pr_link("aaa111", 7)] } },
            { "pullRequests": { "nodes": [pr_link("aaa111", 7), pr_link("bbb222", 8)] } }
        ]));

        let entries = review_entries("acme", &issue);

        assert_eq!(entries.len(), 2, "the repeated pull request is listed once");
        assert_eq!(
            entries[0]["reviewUrl"],
            "https://linear.app/acme/review/aaa111"
        );
        assert_eq!(
            entries[1]["reviewUrl"],
            "https://linear.app/acme/review/bbb222"
        );
    }

    #[test]
    fn test_review_entries_empty_without_sessions_or_slug() {
        assert!(review_entries("acme", &issue_with_sessions(json!([]))).is_empty());

        // A session with no linked pull request, and a link whose slug is missing or
        // blank: all unresolvable, and none of them may produce a bogus URL.
        let unresolvable = issue_with_sessions(json!([
            { "pullRequests": { "nodes": [] } },
            { "pullRequests": { "nodes": [{ "pullRequest": { "number": 1 } }] } },
            { "pullRequests": { "nodes": [{ "pullRequest": { "slugId": "", "number": 2 } }] } }
        ]));
        assert!(review_entries("acme", &unresolvable).is_empty());
    }

    #[test]
    fn test_generate_branch_name_simple() {
        assert_eq!(
            generate_branch_name("LIN-123", "Fix bug"),
            "lin-123/fix-bug"
        );
    }

    #[test]
    fn test_generate_branch_name_special_chars() {
        assert_eq!(
            generate_branch_name("LIN-456", "Add feature: user auth!"),
            "lin-456/add-feature-user-auth"
        );
    }

    #[test]
    fn test_generate_branch_name_long_title() {
        let long_title = "This is a very long title that should be truncated because it exceeds the maximum length";
        let result = generate_branch_name("LIN-789", long_title);
        // Format is identifier/slug, identifier is 7 chars + 1 slash = 8 chars
        // Slug should be max 50 chars
        assert!(result.len() <= 58, "Branch name too long: {}", result);
        assert!(result.starts_with("lin-789/"));
    }

    #[test]
    fn test_generate_branch_name_unicode() {
        // Unicode chars get removed (not alphanumeric)
        let result = generate_branch_name("LIN-100", "Fix emoji 🎉 handling");
        assert_eq!(result, "lin-100/fix-emoji-handling");
    }

    #[test]
    fn test_generate_branch_name_multiple_spaces() {
        assert_eq!(
            generate_branch_name("ENG-42", "Fix   multiple   spaces"),
            "eng-42/fix-multiple-spaces"
        );
    }

    #[test]
    fn test_generate_branch_name_leading_trailing_special() {
        assert_eq!(
            generate_branch_name("DEV-1", "  --Fix bug--  "),
            "dev-1/fix-bug"
        );
    }
}
