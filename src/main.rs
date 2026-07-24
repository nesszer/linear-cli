mod api;
mod cache;
mod commands;
mod config;
mod dates;
mod error;
mod input;
mod json_path;
#[cfg(feature = "secure-storage")]
mod keyring;
mod oauth;
mod output;
mod pagination;
mod priority;
mod retry;
mod text;
#[allow(dead_code)]
mod types;
mod vcs;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use commands::{
    attachments, auth, bulk, comments, cycles, doctor, documents, export, favorites, git, history,
    import, initiatives, interactive, issues, labels, metrics, notifications, project_updates,
    projects, relations, roadmaps, search, setup, sprint, statuses, sync, teams, templates, time,
    triage, update, uploads, users, views, watch, webhooks,
};
use error::CliError;
use output::print_json_owned;
use output::{parse_filters, JsonOutputOptions, OutputOptions, SortOrder};
use pagination::PaginationOptions;
use std::io::IsTerminal;
use std::sync::OnceLock;

/// Output format for command results
#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq)]
pub enum OutputFormat {
    /// Display results as formatted tables (default)
    #[default]
    Table,
    /// Display results as raw JSON
    Json,
    /// Display results as NDJSON (one JSON object per line)
    Ndjson,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

/// Global options for agentic/scripting use
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentOptions {
    /// Suppress decorative output (headers, separators, tips)
    pub quiet: bool,
    /// Only output IDs of created/updated resources
    pub id_only: bool,
    /// Preview without making changes (where supported)
    pub dry_run: bool,
    /// Auto-confirm all prompts (deletes, destructive operations)
    pub yes: bool,
}

static YES_MODE: OnceLock<bool> = OnceLock::new();

pub fn set_yes_mode(yes: bool) {
    let _ = YES_MODE.set(yes);
}

pub fn is_yes() -> bool {
    YES_MODE.get().copied().unwrap_or(false)
}

#[derive(Parser)]
#[command(name = "linear-cli")]
#[command(
    about = "A powerful CLI for Linear.app - manage issues, projects, and more from your terminal"
)]
#[command(version)]
#[command(after_help = r#"QUICK START:
    1. Get your API key from https://linear.app/settings/api
    2. Configure the CLI:
       printf '%s\n' "$LINEAR_API_KEY" | linear config set-key
    3. List your issues:
       linear issues list
    4. Create an issue:
       linear issues create "Fix bug" --team ENG --priority 2

COMMON FLAGS:
    --output table|json|ndjson    Output format (default: table)
    --color-mode auto|always|never   Color output control
    --no-color                    Disable color output
    --width N                     Max table column width
    --no-truncate                 Disable table truncation
    --quiet                       Reduce decorative output
    --format TEMPLATE             Template output (e.g. '{{identifier}} {{title}}')
    --filter field=value          Filter results (=, !=, ~= operators; dot paths; case-insensitive)
    --limit N                     Limit list/search results
    --page-size N                 Page size for list/search
    --after CURSOR                Pagination cursor (after)
    --before CURSOR               Pagination cursor (before)
    --all                         Fetch all pages
    --profile NAME                Use named profile
    --schema                      Print JSON schema version and exit
    --cache-ttl N                 Cache TTL in seconds
    --no-cache                    Disable cache usage
    --yes                         Auto-confirm all prompts

For more info on a command, run: linear <command> --help"#)]
struct Cli {
    /// Output format (table or json)
    #[arg(
        short,
        long,
        global = true,
        env = "LINEAR_CLI_OUTPUT",
        default_value = "table"
    )]
    output: OutputFormat,

    /// Suppress decorative output (headers, separators, tips) - for scripting
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Only output IDs of created/updated resources - for chaining commands
    #[arg(long, global = true)]
    id_only: bool,

    /// Color output: auto, always, or never
    #[arg(
        long = "color-mode",
        global = true,
        value_enum,
        default_value = "auto",
        conflicts_with = "no_color"
    )]
    color_mode: ColorChoice,

    /// Disable color output
    #[arg(long, global = true)]
    no_color: bool,

    /// Max column width for table output (default: 50)
    #[arg(long, global = true)]
    width: Option<usize>,

    /// Disable truncation for table output
    #[arg(long, global = true)]
    no_truncate: bool,

    /// Emit compact JSON without pretty formatting
    #[arg(long, global = true)]
    compact: bool,

    /// Limit JSON output to specific fields (comma-separated, supports dot paths)
    #[arg(long, global = true, value_delimiter = ',')]
    fields: Vec<String>,

    /// Sort JSON array output by a field (default: identifier/id when available)
    #[arg(long, global = true)]
    sort: Option<String>,

    /// Sort order for JSON array output
    #[arg(long, global = true, value_enum, default_value = "asc")]
    order: SortOrder,

    /// Override workspace profile for this invocation
    #[arg(long, global = true, env = "LINEAR_CLI_PROFILE")]
    profile: Option<String>,

    /// Output using a template (e.g. '{{identifier}} {{title}}')
    #[arg(long, global = true)]
    format: Option<String>,

    /// Filter results (field=value, field!=value, field~=value).
    /// Supports dot-notation for nested fields (e.g. state.name=Done).
    /// ~= is a case-insensitive "contains" match. All comparisons are case-insensitive.
    /// Multiple --filter flags are combined with AND logic.
    #[arg(long, global = true)]
    filter: Vec<String>,

    /// Exit with non-zero status when a list is empty
    #[arg(long, global = true)]
    fail_on_empty: bool,

    /// Max results to return for list/search commands
    #[arg(long, global = true)]
    limit: Option<usize>,

    /// Pagination cursor to start after
    #[arg(long, global = true)]
    after: Option<String>,

    /// Pagination cursor to end before
    #[arg(long, global = true)]
    before: Option<String>,

    /// Page size per request for list/search commands
    #[arg(long, global = true)]
    page_size: Option<usize>,

    /// Fetch all pages for list/search commands
    #[arg(long, global = true)]
    all: bool,

    /// Override cache TTL in seconds
    #[arg(long, global = true, env = "LINEAR_CLI_CACHE_TTL")]
    cache_ttl: Option<u64>,

    /// Disable cache usage for this invocation
    #[arg(long, global = true, env = "LINEAR_CLI_NO_CACHE")]
    no_cache: bool,

    /// Preview without making changes where supported
    #[arg(long, global = true)]
    dry_run: bool,

    /// Auto-confirm all prompts (deletes, destructive operations)
    #[arg(long, global = true, env = "LINEAR_CLI_YES")]
    yes: bool,

    /// Number of retries for failed API requests (with exponential backoff)
    #[arg(long, global = true, default_value = "0")]
    retry: u32,

    /// Print JSON schema version info and exit
    #[arg(long, global = true)]
    schema: bool,

    /// Disable pager for output (default: auto-detect from terminal)
    #[arg(long, global = true, env = "LINEAR_CLI_NO_PAGER")]
    no_pager: bool,

    /// Show common tasks and examples
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayOptions {
    pub width: Option<usize>,
    pub no_truncate: bool,
}

impl DisplayOptions {
    pub fn max_width(&self, default: usize) -> Option<usize> {
        if self.no_truncate {
            None
        } else {
            Some(self.width.unwrap_or(default))
        }
    }
}

static DISPLAY_OPTIONS: OnceLock<DisplayOptions> = OnceLock::new();

fn set_cli_state(display: DisplayOptions) {
    let _ = DISPLAY_OPTIONS.set(display);
}

pub fn display_options() -> DisplayOptions {
    DISPLAY_OPTIONS.get().copied().unwrap_or_default()
}

#[derive(Subcommand)]
enum Commands {
    /// Show common tasks and examples
    #[command(alias = "tasks")]
    Common,
    /// Show agent-focused capabilities and examples
    Agent,
    /// Check for and install the latest released version of linear-cli
    #[command(after_help = r#"EXAMPLES:
    linear-cli update
    linear-cli update --check"#)]
    Update {
        /// Check whether a newer release exists without installing it
        #[arg(long)]
        check: bool,
    },
    /// Manage issue attachments - list, create, update, delete, link URLs
    #[command(alias = "att")]
    #[command(after_help = r#"EXAMPLES:
    linear attachments list SCW-123          # List attachments on issue
    linear att get ATTACHMENT_ID             # View attachment details
    linear att create SCW-123 -T "Doc" -u https://example.com
    linear att link-url SCW-123 https://example.com
    linear att delete ATTACHMENT_ID --force  # Delete attachment"#)]
    Attachments {
        #[command(subcommand)]
        action: attachments::AttachmentCommands,
    },
    /// Authenticate and manage API keys
    #[command(after_help = r#"EXAMPLES:
    linear auth login                        # Store API key
    linear auth status                       # Show auth status
    linear auth logout                       # Remove current profile
    linear auth oauth                        # Authenticate via OAuth 2.0
    linear auth oauth --client-id MY_ID      # Use custom OAuth app
    linear auth revoke                       # Revoke OAuth tokens"#)]
    Auth {
        #[command(subcommand)]
        action: auth::AuthCommands,
    },
    /// Diagnose configuration and connectivity
    #[command(after_help = r#"EXAMPLES:
    linear doctor                            # Check config and auth
    linear doctor --check-api                # Validate API access
    linear doctor --fix                      # Auto-fix common issues"#)]
    Doctor {
        /// Validate API connectivity and auth
        #[arg(long)]
        check_api: bool,
        /// Auto-fix common issues (stale cache, missing config, invalid API key)
        #[arg(long)]
        fix: bool,
    },
    /// Execute raw GraphQL queries and mutations against the Linear API
    #[command(after_help = r#"EXAMPLES:
    linear api query '{ viewer { id name } }'
    linear api query -v teamId=abc '...'     # With variables
    linear api mutate -v title=Bug '...'     # Run mutations"#)]
    Api {
        #[command(subcommand)]
        action: commands::api::ApiCommands,
    },
    /// Manage projects - list, create, update, delete projects
    #[command(alias = "p")]
    #[command(after_help = r#"EXAMPLES:
    linear projects list                    # List all projects
    linear p list --archived                # Include archived projects
    linear p get PROJECT_ID                 # View project details
    linear p create "Q1 Roadmap" -t ENG     # Create a project"#)]
    Projects {
        #[command(subcommand)]
        action: projects::ProjectCommands,
    },
    /// Manage project status updates - list, create, update, archive
    #[command(alias = "pu")]
    #[command(after_help = r#"EXAMPLES:
    linear project-updates list "My Project"   # List updates
    linear pu get UPDATE_ID                    # View update details
    linear pu create "My Project" -b "On track" # Create update
    linear pu archive UPDATE_ID                # Archive update"#)]
    ProjectUpdates {
        #[command(subcommand)]
        action: project_updates::ProjectUpdateCommands,
    },
    /// Manage issues - list, create, update, assign, track issues
    #[command(alias = "i")]
    #[command(after_help = r#"EXAMPLES:
    linear issues list                      # List all issues
    linear i list -t ENG -s "In Progress"   # Filter by team and status
    linear i get LIN-123                    # View issue details
    linear i create "Bug fix" -t ENG -p 2   # Create high priority issue
    linear i update LIN-123 -s Done         # Update issue status"#)]
    Issues {
        #[command(subcommand)]
        action: issues::IssueCommands,
    },
    /// Manage labels - create and organize project/issue labels
    #[command(alias = "l")]
    #[command(after_help = r##"EXAMPLES:
    linear labels list                      # List project labels
    linear l list --type issue              # List issue labels
    linear l create "Feature" --color "#10B981"
    linear l delete LABEL_ID --force"##)]
    Labels {
        #[command(subcommand)]
        action: labels::LabelCommands,
    },
    /// Manage teams - list and view team details
    #[command(alias = "t")]
    #[command(after_help = r#"EXAMPLES:
    linear teams list                       # List all teams
    linear t get ENG                        # View team details"#)]
    Teams {
        #[command(subcommand)]
        action: teams::TeamCommands,
    },
    /// Manage users - list workspace users and view profiles
    #[command(alias = "u")]
    #[command(after_help = r#"EXAMPLES:
    linear users list                       # List all users
    linear u list --team ENG                # List team members
    linear u me                             # View your profile"#)]
    Users {
        #[command(subcommand)]
        action: users::UserCommands,
    },
    /// Manage cycles - view sprint cycles and current cycle
    #[command(alias = "c")]
    #[command(after_help = r#"EXAMPLES:
    linear cycles list -t ENG               # List team cycles
    linear c current -t ENG                 # Show current cycle
    linear c create -t ENG --name "Sprint 5" # Create a cycle
    linear c update ID --name "Sprint 5b"   # Update cycle name"#)]
    Cycles {
        #[command(subcommand)]
        action: cycles::CycleCommands,
    },
    /// Manage comments - add and view issue comments
    #[command(alias = "cm")]
    #[command(after_help = r#"EXAMPLES:
    linear comments list ISSUE_ID           # List comments on issue
    linear cm create ISSUE_ID -b "LGTM!"    # Add a comment"#)]
    Comments {
        #[command(subcommand)]
        action: comments::CommentCommands,
    },
    /// Manage documents - create, update, delete documentation
    #[command(alias = "d")]
    #[command(after_help = r#"EXAMPLES:
    linear documents list                   # List all documents
    linear d get DOC_ID                     # View document
    linear d create "Design Doc" -p PROJ_ID # Create document
    linear d delete DOC_ID --force          # Delete document"#)]
    Documents {
        #[command(subcommand)]
        action: documents::DocumentCommands,
    },
    /// Search across Linear - find issues and projects
    #[command(alias = "s")]
    #[command(after_help = r#"EXAMPLES:
    linear search issues "auth bug"         # Search issues
    linear s projects "backend"             # Search projects"#)]
    Search {
        #[command(subcommand)]
        action: search::SearchCommands,
    },
    /// Sync operations - compare local folders with Linear
    #[command(alias = "sy")]
    #[command(after_help = r#"EXAMPLES:
    linear sync status                      # Compare local vs Linear
    linear sy push -t ENG                   # Create projects for folders
    linear sy push -t ENG --dry-run         # Preview without creating"#)]
    Sync {
        #[command(subcommand)]
        action: sync::SyncCommands,
    },
    /// Manage issue statuses - view workflow states
    #[command(alias = "st")]
    #[command(after_help = r#"EXAMPLES:
    linear statuses list -t ENG             # List team statuses
    linear st get "In Progress" -t ENG      # View status details"#)]
    Statuses {
        #[command(subcommand)]
        action: statuses::StatusCommands,
    },
    /// Git branch operations - checkout branches, create PRs
    #[command(alias = "g")]
    #[command(after_help = r#"EXAMPLES:
    linear git checkout LIN-123             # Checkout issue branch
    linear g branch LIN-123                 # Show branch name
    linear g pr LIN-123                     # Create GitHub PR
    linear g pr LIN-123 --draft             # Create draft PR"#)]
    Git {
        #[command(subcommand)]
        action: git::GitCommands,
    },
    /// Bulk operations - update multiple issues at once
    #[command(alias = "b")]
    #[command(after_help = r#"EXAMPLES:
    linear bulk update-state Done -i LIN-1,LIN-2  # Update multiple issues
    linear b assign me -i LIN-1,LIN-2             # Assign multiple issues
    linear b label bug -i LIN-1,LIN-2             # Add label to issues"#)]
    Bulk {
        #[command(subcommand)]
        action: bulk::BulkCommands,
    },
    /// Manage cache - clear cached data or view status
    #[command(alias = "ca")]
    #[command(after_help = r#"EXAMPLES:
    linear cache status                     # Show cache status
    linear ca clear                         # Clear all cache
    linear ca clear --type teams            # Clear only teams cache"#)]
    Cache {
        #[command(subcommand)]
        action: commands::cache::CacheCommands,
    },
    /// Manage notifications - view and mark as read
    #[command(alias = "n")]
    #[command(after_help = r#"EXAMPLES:
    linear notifications list               # List unread notifications
    linear n count                          # Show unread count
    linear n read-all                       # Mark all as read
    linear n archive NOTIF_ID              # Archive a notification
    linear n archive-all                   # Archive all notifications"#)]
    Notifications {
        #[command(subcommand)]
        action: notifications::NotificationCommands,
    },
    /// Manage issue templates - create and use templates
    #[command(alias = "tpl")]
    #[command(after_help = r#"EXAMPLES:
    linear templates list                   # List all templates
    linear tpl create bug --team ENG --priority 2 --label bug
    linear tpl show bug                     # View template details"#)]
    Templates {
        #[command(subcommand)]
        action: templates::TemplateCommands,
    },
    /// Time tracking - log and view time entries
    #[command(alias = "tm")]
    #[command(after_help = r#"EXAMPLES:
    linear time log LIN-123 2h              # Log 2 hours on issue
    linear tm list --issue LIN-123          # List time entries"#)]
    Time {
        #[command(subcommand)]
        action: time::TimeCommands,
    },
    /// Fetch uploads from Linear with authentication
    #[command(alias = "up")]
    #[command(after_help = r#"EXAMPLES:
    linear uploads fetch URL                # Output to stdout (for piping)
    linear up fetch URL -f file.png         # Save to file
    linear up fetch URL | base64            # Pipe to another tool"#)]
    Uploads {
        #[command(subcommand)]
        action: uploads::UploadCommands,
    },
    /// Interactive mode - TUI for browsing and managing issues
    #[command(alias = "int")]
    #[command(after_help = r#"EXAMPLES:
    linear interactive                      # Launch interactive mode
    linear interactive --team ENG           # Preselect team

Use arrow keys to navigate, Enter to select, q to quit."#)]
    Interactive {
        /// Preselect team by key, name, or ID
        #[arg(short, long)]
        team: Option<String>,
    },
    /// Detect current Linear issue from git branch - for AI agents
    #[command(alias = "ctx")]
    #[command(after_help = r#"EXAMPLES:
    linear context                          # Show current issue from branch
    linear ctx --output json                # Get as JSON for parsing

Detects issue ID from branch names like:
  - lin-123-fix-bug
  - feature/LIN-456-new-feature
  - scw-789-some-task"#)]
    Context,
    /// Manage favorites - quick access to issues/projects
    #[command(alias = "fav")]
    #[command(after_help = r#"EXAMPLES:
    linear favorites list                   # List favorites
    linear fav add LIN-123                  # Add issue to favorites
    linear fav remove LIN-123               # Remove from favorites"#)]
    Favorites {
        #[command(subcommand)]
        action: favorites::FavoriteCommands,
    },
    /// Manage roadmaps - view and manage roadmap planning
    #[command(alias = "rm")]
    #[command(after_help = r#"EXAMPLES:
    linear roadmaps list                    # List all roadmaps
    linear rm get ROADMAP_ID                # View roadmap details
    linear rm create "Q1 Plan"              # Create a roadmap
    linear rm update ID -n "Q2 Plan"        # Update roadmap name"#)]
    Roadmaps {
        #[command(subcommand)]
        action: roadmaps::RoadmapCommands,
    },
    /// Manage initiatives - create, update, and track initiatives
    #[command(alias = "init")]
    #[command(after_help = r#"EXAMPLES:
    linear initiatives list                 # List all initiatives
    linear init get INITIATIVE_ID           # View initiative details
    linear init create "H1 Goals"           # Create an initiative
    linear init update ID -s "Active"       # Update initiative status"#)]
    Initiatives {
        #[command(subcommand)]
        action: initiatives::InitiativeCommands,
    },
    /// Triage inbox - manage unassigned issues
    #[command(alias = "tr")]
    #[command(after_help = r#"EXAMPLES:
    linear triage list                      # List triage issues
    linear tr claim LIN-123                 # Claim an issue
    linear tr snooze LIN-123 --duration 1w  # Snooze for a week"#)]
    Triage {
        #[command(subcommand)]
        action: triage::TriageCommands,
    },
    /// View metrics - velocity, burndown, progress
    #[command(alias = "mt")]
    #[command(after_help = r#"EXAMPLES:
    linear metrics cycle CYCLE_ID           # Cycle metrics
    linear mt project PROJECT_ID            # Project progress
    linear mt velocity TEAM --cycles 5      # Team velocity"#)]
    Metrics {
        #[command(subcommand)]
        action: metrics::MetricsCommands,
    },
    /// Manage project milestones - list, create, update, delete milestones
    #[command(alias = "ms")]
    #[command(after_help = r#"EXAMPLES:
    linear milestones list -p "My Project"  # List milestones
    linear ms get MILESTONE_ID              # View milestone details
    linear ms create "Beta Release" -p PROJ # Create milestone
    linear ms update ID --target-date +2w   # Update target date
    linear ms delete ID --force             # Delete milestone"#)]
    Milestones {
        #[command(subcommand)]
        action: commands::milestones::MilestoneCommands,
    },
    /// Export issues to CSV, JSON, or Markdown
    #[command(alias = "exp")]
    #[command(after_help = r#"EXAMPLES:
    linear export csv --team ENG            # Export team issues to CSV
    linear exp csv -f issues.csv            # Export to file
    linear exp json --team ENG --pretty     # Export as pretty JSON
    linear exp markdown --team ENG          # Export as Markdown
    linear exp projects-csv -f projects.csv # Export projects to CSV"#)]
    Export {
        #[command(subcommand)]
        action: export::ExportCommands,
    },
    /// Import issues from CSV or JSON files
    #[command(alias = "im")]
    #[command(after_help = r#"EXAMPLES:
    linear import csv issues.csv -t ENG           # Import from CSV
    linear im csv issues.csv -t ENG --dry-run     # Preview without creating
    linear im json issues.json -t ENG             # Import from JSON"#)]
    Import {
        #[command(subcommand)]
        action: import::ImportCommands,
    },
    /// View issue history and activity
    #[command(alias = "hist")]
    #[command(after_help = r#"EXAMPLES:
    linear history issue LIN-123            # View issue activity
    linear hist issue LIN-123 --limit 50    # More entries"#)]
    History {
        #[command(subcommand)]
        action: history::HistoryCommands,
    },
    /// Manage custom views - create, apply, and manage saved views
    #[command(alias = "v")]
    #[command(after_help = r#"EXAMPLES:
    linear views list                       # List all custom views
    linear v list --shared                  # List shared views only
    linear v get "My View"                  # View details
    linear v create "Bug Triage" --shared   # Create a shared view
    linear v delete VIEW_ID --force         # Delete a view"#)]
    Views {
        #[command(subcommand)]
        action: views::ViewCommands,
    },
    /// Manage webhooks - create, update, delete, listen for events
    #[command(alias = "wh")]
    #[command(after_help = r#"EXAMPLES:
    linear webhooks list                    # List all webhooks
    linear wh create URL --events Issue     # Create webhook
    linear wh delete WEBHOOK_ID --force     # Delete webhook
    linear wh rotate-secret WEBHOOK_ID      # Rotate webhook secret
    linear wh listen --port 9000            # Listen for events locally"#)]
    Webhooks {
        #[command(subcommand)]
        action: webhooks::WebhookCommands,
    },
    /// Watch for updates (polling)
    #[command(after_help = r#"EXAMPLES:
    linear watch issue LIN-123             # Watch single issue
    linear watch issue LIN-123 --interval 30  # Poll every 30 seconds
    linear watch project PROJECT_ID        # Watch a project
    linear watch team ENG                  # Watch a team"#)]
    Watch {
        #[command(subcommand)]
        action: WatchCommands,
    },
    /// Manage issue relationships - parent/child, blocking, related
    #[command(alias = "rel")]
    #[command(after_help = r#"EXAMPLES:
    linear relations list LIN-123           # List issue relationships
    linear rel add LIN-1 -r blocks LIN-2    # LIN-1 blocks LIN-2
    linear rel parent LIN-2 LIN-1           # Set LIN-1 as parent of LIN-2
    linear rel unparent LIN-2               # Remove parent"#)]
    Relations {
        #[command(subcommand)]
        action: relations::RelationCommands,
    },
    /// Show current authenticated user (alias for `users me`)
    #[command(alias = "me")]
    Whoami,
    /// Mark the current branch's issue as Done
    #[command(after_help = r#"EXAMPLES:
    linear done                              # Mark current branch issue as Done
    linear done --status "In Progress"       # Set to specific status instead

Reads the current git branch, extracts the issue ID (e.g. feat/SCW-123-title → SCW-123),
and updates the issue status."#)]
    Done {
        /// Status to set (default: "Done")
        #[arg(short, long, default_value = "Done")]
        status: String,
    },
    /// Guided onboarding wizard - configure auth, team, and output format
    #[command(after_help = r#"EXAMPLES:
    linear setup                             # Run interactive setup wizard

Walks you through:
  1. Setting your Linear API key
  2. Choosing a default team
  3. Selecting output format (table or json)"#)]
    Setup,
    /// Sprint planning - manage cycle-based sprints
    #[command(alias = "sp")]
    #[command(after_help = r#"EXAMPLES:
    linear sprint status -t ENG            # Current sprint status
    linear sp progress -t ENG              # Sprint progress bar
    linear sp plan -t ENG                  # Next sprint's planned issues
    linear sp carry-over -t ENG --force    # Move incomplete issues to next cycle"#)]
    Sprint {
        #[command(subcommand)]
        action: sprint::SprintCommands,
    },
    /// Generate shell completions
    #[command(alias = "comp")]
    #[command(after_help = r#"EXAMPLES:
    linear completions bash > ~/.bash_completion.d/linear
    linear completions zsh > ~/.zfunc/_linear
    linear completions fish > ~/.config/fish/completions/linear.fish
    linear comp powershell > linear.ps1
    linear comp dynamic bash   # Dynamic completions with argument value hints
    linear comp dynamic zsh    # Dynamic completions for zsh"#)]
    Completions {
        #[command(subcommand)]
        action: CompletionCommands,
    },
    /// Internal: provide dynamic completion values (hidden from help)
    #[command(name = "_complete", hide = true)]
    Complete {
        /// What to complete: teams, projects, issues, statuses, users, labels
        #[arg(long = "type")]
        type_: String,
        /// Partial input to filter
        #[arg(long, default_value = "")]
        prefix: String,
        /// Team context for scoped completions (e.g. statuses)
        #[arg(long)]
        team: Option<String>,
    },
    /// Configure CLI settings - API keys, default team, and workspaces
    #[command(after_help = r#"KNOWN KEYS:
    api-key         API key (get only; set via `config set-key`)
    profile         Active workspace profile
    default-team    Default team key/name (also: default_team, team)

EXAMPLES:
    printf '%s\n' "$LINEAR_API_KEY" | linear config set-key
    linear config get api-key               # Get API key (masked)
    linear config set default-team ENG      # Persist default team
    linear config get default-team          # Show default team
    linear config set profile work          # Switch profile
    linear config show                      # Show configuration
    printf '%s\n' "$LINEAR_API_KEY" | linear config workspace-add work
    linear config workspace-switch work     # Switch workspace

ENV OVERRIDES:
    LINEAR_CLI_TEAM     Overrides default-team for this process
    LINEAR_CLI_PROFILE  Overrides active profile
    LINEAR_CLI_OUTPUT   Default output format"#)]
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Set API key
    #[command(after_help = r#"EXAMPLE:
    printf '%s\n' "$LINEAR_API_KEY" | linear config set-key"#)]
    SetKey,
    /// Get a configuration value
    Get {
        /// Config key: api-key, profile, default-team
        key: ConfigGetKey,
        /// Output raw value without masking
        #[arg(long)]
        raw: bool,
    },
    /// Set a configuration value
    Set {
        /// Config key: profile, default-team
        key: ConfigSetKey,
        /// Value to set
        value: String,
    },
    /// Show current configuration
    Show,
    /// Generate shell completions
    #[command(after_help = r#"EXAMPLES:
    linear config completions bash > ~/.bash_completion.d/linear
    linear config completions zsh > ~/.zfunc/_linear
    linear config completions fish > ~/.config/fish/completions/linear.fish
    linear config completions powershell > linear.ps1"#)]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Add a new workspace
    #[command(alias = "add")]
    #[command(after_help = r#"EXAMPLE:
    printf '%s\n' "$LINEAR_API_KEY" | linear config workspace-add personal"#)]
    WorkspaceAdd {
        /// Workspace name
        name: String,
    },
    /// List all workspaces
    #[command(alias = "list")]
    WorkspaceList,
    /// Switch to a different workspace
    #[command(alias = "use")]
    #[command(after_help = r#"EXAMPLE:
    linear config workspace-switch personal"#)]
    WorkspaceSwitch {
        /// Workspace name to switch to
        name: String,
    },
    /// Show current workspace
    #[command(alias = "current")]
    WorkspaceCurrent,
    /// Remove a workspace
    #[command(alias = "rm")]
    WorkspaceRemove {
        /// Workspace name to remove
        name: String,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum ConfigGetKey {
    #[value(alias = "api_key")]
    ApiKey,
    Profile,
    #[value(alias = "default_team", alias = "team")]
    DefaultTeam,
}

impl std::fmt::Display for ConfigGetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey => write!(f, "api-key"),
            Self::Profile => write!(f, "profile"),
            Self::DefaultTeam => write!(f, "default-team"),
        }
    }
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum ConfigSetKey {
    Profile,
    #[value(alias = "default_team", alias = "team")]
    DefaultTeam,
}

impl std::fmt::Display for ConfigSetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile => write!(f, "profile"),
            Self::DefaultTeam => write!(f, "default-team"),
        }
    }
}

#[derive(Subcommand)]
enum WatchCommands {
    /// Watch an issue for updates
    Issue {
        /// Issue identifier to watch
        id: String,
        /// Polling interval in seconds
        #[arg(short, long, default_value = "10")]
        interval: u64,
    },
    /// Watch a project for updates
    Project {
        /// Project ID to watch
        id: String,
        /// Polling interval in seconds
        #[arg(short, long, default_value = "10")]
        interval: u64,
    },
    /// Watch a team for updates
    Team {
        /// Team key or ID to watch
        team: String,
        /// Polling interval in seconds
        #[arg(short, long, default_value = "10")]
        interval: u64,
    },
}

#[derive(Subcommand)]
enum CompletionCommands {
    /// Generate static shell completions (command names and flags)
    Static {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Generate dynamic shell completions (argument values from Linear API)
    Dynamic {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

fn main() -> Result<()> {
    // Spawn with 8 MB stack to accommodate clap derive macro stack usage
    // with many subcommand variants (default 1 MB overflows in debug builds).
    let builder = std::thread::Builder::new().stack_size(8 * 1024 * 1024);
    let handler = builder.spawn(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime")
            .block_on(async_main())
    })?;
    let exit_code = handler.join().unwrap()?;
    std::process::exit(exit_code);
}

async fn async_main() -> Result<i32> {
    let cli = Cli::parse();
    if cli.no_color || cli.color_mode == ColorChoice::Never {
        colored::control::set_override(false);
    } else if cli.color_mode == ColorChoice::Always {
        colored::control::set_override(true);
    }
    set_cli_state(DisplayOptions {
        width: cli.width,
        no_truncate: cli.no_truncate,
    });
    if let Some(profile) = cli.profile.as_deref() {
        std::env::set_var("LINEAR_CLI_PROFILE", profile);
    }
    api::set_default_retry(cli.retry);
    let filters = parse_filters(&cli.filter)?;
    let pagination = PaginationOptions {
        limit: cli.limit,
        after: cli.after.clone(),
        before: cli.before.clone(),
        page_size: cli.page_size,
        all: cli.all,
    };
    let json_opts = JsonOutputOptions::new(
        cli.compact,
        if cli.fields.is_empty() {
            None
        } else {
            Some(cli.fields.clone())
        },
        cli.sort.clone(),
        cli.order,
        true,
    );
    let output = OutputOptions {
        format: cli.output,
        json: json_opts,
        format_template: cli.format.clone(),
        filters,
        fail_on_empty: cli.fail_on_empty,
        pagination,
        cache: cache::CacheOptions {
            ttl_seconds: cli.cache_ttl,
            no_cache: cli.no_cache,
        },
        dry_run: cli.dry_run,
    };
    let agent_opts = AgentOptions {
        quiet: cli.quiet,
        id_only: cli.id_only,
        dry_run: cli.dry_run,
        yes: cli.yes,
    };

    output::set_quiet_mode(
        cli.quiet
            || matches!(cli.output, OutputFormat::Json | OutputFormat::Ndjson)
            // The hidden _complete command feeds shell completion; any stderr noise
            // (e.g. keyring warnings on a keyless/headless box) corrupts the caller's
            // 2>/dev/null contract, so keep it silent.
            || matches!(cli.command, Commands::Complete { .. }),
    );
    set_yes_mode(cli.yes);

    if cli.schema {
        let schema = serde_json::json!({
            "schema_version": "1.0",
            "schema_file": "docs/json/schema.json",
        });
        if matches!(cli.output, OutputFormat::Ndjson) || cli.compact {
            println!("{}", serde_json::to_string(&schema)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&schema)?);
        }
        return Ok(0);
    }

    if should_check_for_updates(&cli) {
        if let Some(exit_code) = update::maybe_prompt_for_update(cli.yes).await? {
            return Ok(exit_code);
        }
    }

    let exit_code = {
        // Keep the pager guard scoped so cleanup runs before main exits.
        // Never page interactive commands (setup/auth/TUI) — less captures keys
        // and can leave the terminal in a bad state on macOS (see issue #34).
        let _pager_guard = if should_use_pager(
            cli.no_pager,
            &cli.output,
            cli.quiet,
            command_disables_pager(&cli.command),
        ) {
            setup_pager()
        } else {
            None
        };

        let result = run_command(cli.command, &output, agent_opts, cli.retry).await;

        match result {
            Ok(()) => 0,
            Err(e) => {
                // Check if JSON output requested for structured errors
                if output.is_json() {
                    if let Some(cli_error) = e.downcast_ref::<CliError>() {
                        let error_json = serde_json::json!({
                            "error": true,
                            "message": cli_error.message,
                            "code": cli_error.code(),
                            "details": cli_error.details,
                            "retry_after": cli_error.retry_after,
                        });
                        eprintln!(
                            "{}",
                            serde_json::to_string(&error_json).unwrap_or_else(|_| e.to_string())
                        );
                    } else {
                        let error_json = serde_json::json!({
                            "error": true,
                            "message": e.to_string(),
                            "code": categorize_error(&e),
                            "details": null,
                            "retry_after": null,
                        });
                        eprintln!(
                            "{}",
                            serde_json::to_string(&error_json).unwrap_or_else(|_| e.to_string())
                        );
                    }
                } else {
                    eprintln!("Error: {}", e);
                }
                categorize_error(&e) as i32
            }
        }
    };

    Ok(exit_code)
}

fn should_check_for_updates(cli: &Cli) -> bool {
    let _ = cli;
    false
}

/// Categorize error for exit codes: 1=general error, 2=not found, 3=auth error
fn categorize_error(e: &anyhow::Error) -> u8 {
    if let Some(cli_error) = e.downcast_ref::<CliError>() {
        return cli_error.code();
    }
    let msg = e.to_string().to_lowercase();
    if msg.contains("not found") || msg.contains("does not exist") {
        2
    } else if msg.contains("unauthorized")
        || msg.contains("api key")
        || msg.contains("authentication")
    {
        3
    } else if msg.contains("rate limit") || msg.contains("too many requests") {
        4
    } else {
        1
    }
}

async fn run_command(
    command: Commands,
    output: &OutputOptions,
    agent_opts: AgentOptions,
    retry: u32,
) -> Result<()> {
    match command {
        Commands::Common => {
            println!("Common tasks:");
            println!("  linear issues list -t ENG");
            println!("  linear issues get LIN-123");
            println!("  linear issues create \"Title\" -t ENG");
            println!("  linear issues update LIN-123 -s Done");
            println!("  linear projects list");
            println!("  linear teams list");
            println!("  linear git checkout LIN-123");
            println!("  linear git pr LIN-123 --draft");
            println!("  linear interactive --team ENG");
            println!();
            println!("Tips:");
            println!("  Use --help after any command for more options.");
            println!("  Use --output json or --output ndjson for scripting/LLMs.");
            println!("  Use --no-color for logs/CI.");
            println!("  Use --limit/--page-size/--all for pagination.");
        }
        Commands::Agent => {
            println!("Agent harness:");
            println!("  Use --output json or --output ndjson for machine-readable output.");
            println!("  Use --compact and --fields to reduce tokens.");
            println!("  Use --sort/--order to stabilize list outputs.");
            println!("  Use --filter to reduce list results.");
            println!("  Use --id-only for chaining create/update commands.");
            println!("  Use --data - for JSON input on issue create/update.");
            println!("  Use --yes to auto-confirm all prompts (deletes, destructive ops).");
            println!("  Use 'linear done' to mark current branch's issue as Done.");
            println!();
            println!("Examples:");
            println!("  linear issues list --output json --compact --fields identifier,title");
            println!("  linear issues list --output ndjson --filter state.name=In\\ Progress");
            println!("  linear issues get LIN-123 --output json");
            println!("  linear issues update LIN-123 --data - --dry-run");
            println!("  linear context --output json --id-only");
            println!();
            println!("Schemas:");
            println!("  See docs/json/ for sample outputs.");
            println!("  Use --schema to print the current schema version.");
        }
        Commands::Update { check } => update::handle(check, output, agent_opts).await?,
        Commands::Projects { action } => projects::handle(action, output).await?,
        Commands::ProjectUpdates { action } => project_updates::handle(action, output).await?,
        Commands::Issues { action } => issues::handle(action, output, agent_opts).await?,
        Commands::Attachments { action } => attachments::handle(action, output).await?,
        Commands::Labels { action } => labels::handle(action, output).await?,
        Commands::Teams { action } => teams::handle(action, output).await?,
        Commands::Users { action } => users::handle(action, output).await?,
        Commands::Cycles { action } => cycles::handle(action, output).await?,
        Commands::Comments { action } => comments::handle(action, output).await?,
        Commands::Documents { action } => documents::handle(action, output).await?,
        Commands::Search { action } => search::handle(action, output).await?,
        Commands::Sync { action } => sync::handle(action, output).await?,
        Commands::Statuses { action } => statuses::handle(action, output).await?,
        Commands::Git { action } => git::handle(action).await?,
        Commands::Bulk { action } => bulk::handle(action, output).await?,
        Commands::Cache { action } => commands::cache::handle(action).await?,
        Commands::Notifications { action } => notifications::handle(action, output).await?,
        Commands::Templates { action } => templates::handle(action, output).await?,
        Commands::Time { action } => time::handle(action, output).await?,
        Commands::Uploads { action } => uploads::handle(action).await?,
        Commands::Interactive { team } => interactive::run(config::resolve_team_arg(team)).await?,
        Commands::Context => handle_context(output, agent_opts, retry).await?,
        Commands::Favorites { action } => favorites::handle(action, output).await?,
        Commands::Roadmaps { action } => {
            roadmaps::handle(action, output, &output.pagination).await?
        }
        Commands::Initiatives { action } => {
            initiatives::handle(action, output, &output.pagination).await?
        }
        Commands::Triage { action } => triage::handle(action, output).await?,
        Commands::Metrics { action } => metrics::handle(action, output).await?,
        Commands::Milestones { action } => commands::milestones::handle(action, output).await?,
        Commands::Export { action } => export::handle(action, output).await?,
        Commands::Import { action } => import::handle(action, output).await?,
        Commands::History { action } => history::handle(action, output).await?,
        Commands::Views { action } => views::handle(action, output).await?,
        Commands::Webhooks { action } => webhooks::handle(action, output).await?,
        Commands::Watch { action } => match action {
            WatchCommands::Issue { id, interval } => {
                watch::watch_issue(&id, interval, output).await?
            }
            WatchCommands::Project { id, interval } => {
                watch::watch_project(&id, interval, output).await?
            }
            WatchCommands::Team { team, interval } => {
                watch::watch_team(&team, interval, output).await?
            }
        },
        Commands::Relations { action } => relations::handle(action, output).await?,
        Commands::Whoami => users::handle(users::UserCommands::Me, output).await?,
        Commands::Done { status } => handle_done(&status, output, agent_opts, retry).await?,
        Commands::Setup => setup::handle(output).await?,
        Commands::Sprint { action } => sprint::handle(action, output).await?,
        Commands::Completions { action } => match action {
            CompletionCommands::Static { shell } => {
                let mut cmd = Cli::command();
                generate(shell, &mut cmd, "linear-cli", &mut std::io::stdout());
            }
            CompletionCommands::Dynamic { shell } => {
                print_dynamic_completion_script(shell);
            }
        },
        Commands::Complete {
            type_,
            prefix,
            team,
        } => handle_complete(&type_, &prefix, team.as_deref()).await?,
        Commands::Auth { action } => auth::handle(action, output).await?,
        Commands::Api { action } => commands::api::handle(action, output).await?,
        Commands::Doctor { check_api, fix } => doctor::run(output, check_api, fix).await?,
        Commands::Config { action } => match action {
            ConfigCommands::SetKey => {
                config::set_api_key_from_stdin_or_prompt()?;
                if !agent_opts.quiet {
                    println!("API key saved successfully!");
                }
            }
            ConfigCommands::Get { key, raw } => {
                config::config_get(&key.to_string(), raw)?;
            }
            ConfigCommands::Set { key, value } => {
                config::config_set(&key.to_string(), &value)?;
            }
            ConfigCommands::Show => {
                config::show_config()?;
            }
            ConfigCommands::Completions { shell } => {
                let mut cmd = Cli::command();
                generate(shell, &mut cmd, "linear-cli", &mut std::io::stdout());
            }
            ConfigCommands::WorkspaceAdd { name } => {
                config::workspace_add_from_stdin_or_prompt(&name)?;
            }
            ConfigCommands::WorkspaceList => {
                config::workspace_list()?;
            }
            ConfigCommands::WorkspaceSwitch { name } => {
                config::workspace_switch(&name)?;
            }
            ConfigCommands::WorkspaceCurrent => {
                config::workspace_current()?;
            }
            ConfigCommands::WorkspaceRemove { name } => {
                config::workspace_remove(&name)?;
            }
        },
    }

    Ok(())
}

/// Commands that own the terminal (prompts, password entry, full-screen TUI).
/// Paging these breaks keyboard input and can leave the tty raw after exit.
fn command_disables_pager(command: &Commands) -> bool {
    // Terminal owners only (password prompts / TUI). Doctor is output-oriented.
    matches!(
        command,
        Commands::Setup | Commands::Interactive { .. } | Commands::Auth { .. }
    )
}

/// Determine if pager should be used
fn should_use_pager(
    no_pager: bool,
    format: &OutputFormat,
    quiet: bool,
    interactive_command: bool,
) -> bool {
    if no_pager || quiet || interactive_command {
        return false;
    }
    // Only page table output, not JSON/NDJSON (those are for scripts)
    if !matches!(format, OutputFormat::Table) {
        return false;
    }
    // Only page when stdout is a terminal
    std::io::stdout().is_terminal()
}

/// Set up pager by spawning pager process and redirecting stdout.
/// Returns a guard that waits for the pager to finish when dropped.
fn setup_pager() -> Option<PagerGuard> {
    // Set LESS options if not already set (like git does)
    // -R: raw control chars (colors), -X: don't clear screen, -F: quit if one screen
    if std::env::var("LESS").is_err() {
        std::env::set_var("LESS", "-R -X -F");
    }

    let pager_cmd = std::env::var("PAGER").unwrap_or_else(|_| "less".to_string());
    let (program, args) = resolve_pager_command(&pager_cmd, trust_pager_enabled());
    if pager_program_basename(&program) == Some("cat") || program.is_empty() {
        return None;
    }

    // Try to spawn pager with stdin piped
    let mut child = match std::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return None, // Pager not available, continue without it
    };

    let Some(child_stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let pager_fd = child_stdin.as_raw_fd();
        let stdout_redirect = match StdoutRedirectGuard::redirect_to(pager_fd) {
            Ok(guard) => Some(guard),
            Err(_) => {
                drop(child_stdin);
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        };
        Some(PagerGuard {
            child,
            stdin: Some(child_stdin),
            stdout_redirect,
        })
    }

    #[cfg(not(unix))]
    {
        // On non-Unix platforms, pager redirect is not supported — skip
        drop(child_stdin);
        let _ = child.kill();
        None
    }
}

fn pager_program_basename(program: &str) -> Option<&str> {
    std::path::Path::new(program).file_name()?.to_str()
}

fn trust_pager_enabled() -> bool {
    std::env::var("LINEAR_CLI_TRUST_PAGER")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn resolve_pager_command(pager_cmd: &str, trust_pager: bool) -> (String, Vec<String>) {
    let pager_cmd = pager_cmd.trim();
    if pager_cmd.is_empty() {
        return (String::new(), Vec::new());
    }

    let mut parts = pager_cmd.split_whitespace();
    let Some(program) = parts.next() else {
        return (String::new(), Vec::new());
    };

    let trusted = ["less", "more", "most", "bat", "cat"];
    let program_path = std::path::Path::new(program);
    let has_path_component = program_path.components().count() > 1;

    if trusted.contains(&program) && !has_path_component {
        let args = parts.map(|part| part.to_string()).collect();
        return (program.to_string(), args);
    }

    if trust_pager && program_path.is_absolute() {
        let args = parts.map(|part| part.to_string()).collect();
        return (program.to_string(), args);
    }

    if has_path_component {
        eprintln!(
            "Ignoring untrusted PAGER '{}'; set LINEAR_CLI_TRUST_PAGER=1 to allow absolute pager paths, otherwise falling back to less",
            pager_cmd
        );
    } else {
        eprintln!(
            "Ignoring untrusted PAGER '{}'; falling back to less",
            pager_cmd
        );
    }
    ("less".to_string(), Vec::new())
}

/// Guard that waits for the pager process to exit when dropped
struct PagerGuard {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    #[cfg(unix)]
    stdout_redirect: Option<StdoutRedirectGuard>,
}

impl Drop for PagerGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // Restore stdout before shutting down the pager; macOS terminal state is
            // sensitive to pager teardown when fd 1 still points at the pager pipe.
            self.stdout_redirect.take();
        }

        // Close stdin pipe so the pager sees EOF.
        self.stdin.take();
        // Then wait for pager to finish
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
struct StdoutRedirectGuard {
    saved_stdout_fd: i32,
}

#[cfg(unix)]
impl StdoutRedirectGuard {
    fn redirect_to(target_fd: i32) -> std::io::Result<Self> {
        let saved_stdout_fd = unsafe { libc::dup(libc::STDOUT_FILENO) };
        if saved_stdout_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        if unsafe { libc::dup2(target_fd, libc::STDOUT_FILENO) } < 0 {
            let err = std::io::Error::last_os_error();
            unsafe {
                libc::close(saved_stdout_fd);
            }
            return Err(err);
        }

        Ok(Self { saved_stdout_fd })
    }
}

#[cfg(unix)]
impl Drop for StdoutRedirectGuard {
    fn drop(&mut self) {
        unsafe {
            libc::dup2(self.saved_stdout_fd, libc::STDOUT_FILENO);
            libc::close(self.saved_stdout_fd);
        }
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::sync::{Mutex, OnceLock};

    #[cfg(unix)]
    static STDOUT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[cfg(unix)]
    fn stdout_test_lock() -> &'static Mutex<()> {
        STDOUT_TEST_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[cfg(unix)]
    struct Pipe {
        read_fd: i32,
        write_fd: i32,
    }

    #[cfg(unix)]
    impl Pipe {
        fn new() -> Self {
            let mut fds = [0; 2];
            let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
            assert_eq!(
                rc,
                0,
                "pipe creation failed: {}",
                std::io::Error::last_os_error()
            );
            Self {
                read_fd: fds[0],
                write_fd: fds[1],
            }
        }

        fn read_available(&self) -> String {
            let flags = unsafe { libc::fcntl(self.read_fd, libc::F_GETFL) };
            assert!(
                flags >= 0,
                "fcntl(F_GETFL) failed: {}",
                std::io::Error::last_os_error()
            );
            let rc = unsafe { libc::fcntl(self.read_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
            assert!(
                rc >= 0,
                "fcntl(F_SETFL) failed: {}",
                std::io::Error::last_os_error()
            );

            let mut bytes = Vec::new();
            let mut buf = [0_u8; 256];

            loop {
                let read = unsafe { libc::read(self.read_fd, buf.as_mut_ptr().cast(), buf.len()) };
                if read > 0 {
                    bytes.extend_from_slice(&buf[..read as usize]);
                    continue;
                }
                if read == 0 {
                    break;
                }

                let err = std::io::Error::last_os_error();
                match err.raw_os_error() {
                    Some(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK => break,
                    _ => panic!("read failed: {err}"),
                }
            }

            String::from_utf8(bytes).expect("pipe output should be valid utf-8")
        }
    }

    #[cfg(unix)]
    fn write_stdout(text: &str) {
        let bytes = text.as_bytes();
        let written =
            unsafe { libc::write(libc::STDOUT_FILENO, bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(
            written,
            bytes.len() as isize,
            "stdout write failed: {}",
            std::io::Error::last_os_error()
        );
    }

    #[cfg(unix)]
    impl Drop for Pipe {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.read_fd);
                libc::close(self.write_fd);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_stdout_redirect_guard_restores_previous_stdout() {
        let _stdout_lock = stdout_test_lock().lock().unwrap();
        let outer_pipe = Pipe::new();
        let inner_pipe = Pipe::new();

        let outer_guard = StdoutRedirectGuard::redirect_to(outer_pipe.write_fd)
            .expect("outer stdout redirect should succeed");

        {
            let inner_guard = StdoutRedirectGuard::redirect_to(inner_pipe.write_fd)
                .expect("inner stdout redirect should succeed");
            write_stdout("inner redirect\n");
            drop(inner_guard);
        }

        write_stdout("restored outer redirect\n");

        drop(outer_guard);

        let inner_output = inner_pipe.read_available();
        let outer_output = outer_pipe.read_available();

        assert!(
            inner_output.contains("inner redirect\n"),
            "inner pipe should capture redirected stdout, got: {inner_output:?}"
        );
        assert!(
            outer_output.contains("restored outer redirect\n"),
            "outer pipe should receive stdout after restoration, got: {outer_output:?}"
        );
    }

    #[test]
    fn test_should_use_pager_disabled_for_interactive_commands() {
        assert!(!should_use_pager(false, &OutputFormat::Table, false, true));
        // Non-interactive table on TTY would still depend on is_terminal; quiet/json off.
        assert!(!should_use_pager(true, &OutputFormat::Table, false, false));
        assert!(!should_use_pager(false, &OutputFormat::Json, false, false));
        assert!(!should_use_pager(false, &OutputFormat::Table, true, false));
    }

    #[test]
    fn test_command_disables_pager_for_setup() {
        assert!(command_disables_pager(&Commands::Setup));
        assert!(!command_disables_pager(&Commands::Common));
    }

    #[test]
    fn test_sanitize_completion_value_strips_shell_metacharacters() {
        let value = "$(touch /tmp/pwn); hello|world && rm -rf /";
        assert_eq!(
            sanitize_completion_value(value),
            "touch /tmp/pwn helloworld  rm -rf /"
        );
    }

    #[test]
    fn test_sanitize_completion_value_keeps_common_identifiers() {
        let value = "LIN-123 team_alpha/user@example.com";
        assert_eq!(sanitize_completion_value(value), value);
    }

    #[test]
    fn test_resolve_pager_command_allows_known_bare_program() {
        let (program, args) = resolve_pager_command("less -R -F", false);
        assert_eq!(program, "less");
        assert_eq!(args, vec!["-R", "-F"]);
    }

    #[test]
    fn test_resolve_pager_command_rejects_relative_path_bypass() {
        let (program, args) = resolve_pager_command("./less", false);
        assert_eq!(program, "less");
        assert!(args.is_empty());
    }

    #[test]
    fn test_resolve_pager_command_rejects_relative_path_even_when_trusted() {
        let (program, args) = resolve_pager_command("./less -R", true);
        assert_eq!(program, "less");
        assert!(args.is_empty());
    }

    // Unix path roots are absolute only on Unix; Windows uses drive-letter paths.
    #[cfg(unix)]
    #[test]
    fn test_resolve_pager_command_rejects_absolute_path_bypass() {
        let (program, args) = resolve_pager_command("/tmp/evil/less --steal", false);
        assert_eq!(program, "less");
        assert!(args.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_pager_command_allows_absolute_path_when_trusted() {
        let (program, args) = resolve_pager_command("/usr/bin/less -R -F", true);
        assert_eq!(program, "/usr/bin/less");
        assert_eq!(args, vec!["-R", "-F"]);
    }

    #[cfg(windows)]
    #[test]
    fn test_resolve_pager_command_allows_windows_absolute_path_when_trusted() {
        let (program, args) = resolve_pager_command(r"C:\bin\less -R -F", true);
        assert_eq!(program, r"C:\bin\less");
        assert_eq!(args, vec!["-R", "-F"]);
    }
}

fn sanitize_completion_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\r' | '\n' | '\t' => ' ',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn sanitize_completion_value(value: &str) -> String {
    sanitize_completion_field(value)
        .chars()
        .filter(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, ' ' | '.' | '_' | '-' | ':' | '/' | '@' | '+')
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Handle the context command - detect current Linear issue from git branch
async fn handle_context(
    output: &OutputOptions,
    agent_opts: AgentOptions,
    retry: u32,
) -> Result<()> {
    // Get current git branch
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();

    let branch = match branch_output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => {
            anyhow::bail!("Not in a git repository or git not available");
        }
    };

    // Extract issue ID from branch name using regex
    static ISSUE_RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = ISSUE_RE.get_or_init(|| regex::Regex::new(r"(?i)([a-z]+-\d+)").unwrap());

    let issue_id = re
        .find(&branch)
        .map(|m| m.as_str().to_uppercase())
        .ok_or_else(|| anyhow::anyhow!("No Linear issue ID found in branch: {}", branch))?;

    if output.is_json() || output.has_template() {
        if agent_opts.id_only {
            print_json_owned(serde_json::json!(issue_id), output)?;
            return Ok(());
        }
        // Fetch issue details for JSON output
        let client = api::LinearClient::new_with_retry(retry)?;
        let query = r#"
            query($id: String!) {
                issue(id: $id) {
                    id
                    identifier
                    title
                    state { name }
                    assignee { name }
                    priority
                    url
                }
            }
        "#;

        let result = client
            .query(query, Some(serde_json::json!({ "id": issue_id })))
            .await;

        match result {
            Ok(data) => {
                let issue = &data["data"]["issue"];
                if issue.is_null() {
                    print_json_owned(
                        serde_json::json!({
                            "branch": branch,
                            "issue_id": issue_id,
                            "found": false,
                        }),
                        output,
                    )?;
                } else {
                    print_json_owned(
                        serde_json::json!({
                            "branch": branch,
                            "issue_id": issue_id,
                            "found": true,
                            "issue": issue,
                        }),
                        output,
                    )?;
                }
            }
            Err(_) => {
                print_json_owned(
                    serde_json::json!({
                        "branch": branch,
                        "issue_id": issue_id,
                        "found": false,
                    }),
                    output,
                )?;
            }
        }
    } else {
        println!("{}", issue_id);
    }

    Ok(())
}

/// Handle the `done` command — mark the current branch's issue as Done (or a custom status)
async fn handle_done(
    status: &str,
    output: &OutputOptions,
    agent_opts: AgentOptions,
    retry: u32,
) -> Result<()> {
    // Get current git branch
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();

    let branch = match branch_output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            anyhow::bail!("Not in a git repository or git not available");
        }
    };

    // Extract issue ID from branch name
    static DONE_ISSUE_RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = DONE_ISSUE_RE.get_or_init(|| regex::Regex::new(r"(?i)([a-z]+-\d+)").unwrap());

    let issue_id = re
        .find(&branch)
        .map(|m| m.as_str().to_uppercase())
        .ok_or_else(|| anyhow::anyhow!("No Linear issue ID found in branch: {}", branch))?;

    if output.dry_run {
        if output.is_json() || output.has_template() {
            print_json_owned(
                serde_json::json!({
                    "dry_run": true,
                    "would_update": {
                        "issue_id": issue_id,
                        "status": status,
                    }
                }),
                output,
            )?;
        } else {
            println!("[DRY RUN] Would set {} to status: {}", issue_id, status);
        }
        return Ok(());
    }

    let client = api::LinearClient::new_with_retry(retry)?;

    // Get the issue's team to resolve the correct state
    let query = r#"
        query($id: String!) {
            issue(id: $id) {
                id
                identifier
                title
                team { id }
            }
        }
    "#;

    let result = client
        .query(query, Some(serde_json::json!({ "id": issue_id })))
        .await?;
    let issue = &result["data"]["issue"];
    if issue.is_null() {
        anyhow::bail!("Issue not found: {}", issue_id);
    }

    let team_id = issue["team"]["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Could not determine team for {}", issue_id))?;

    let state_id = api::resolve_state_id(&client, team_id, status).await?;

    let mutation = r#"
        mutation($id: String!, $stateId: String!) {
            issueUpdate(id: $id, input: { stateId: $stateId }) {
                success
                issue {
                    id
                    identifier
                    title
                    state { name }
                }
            }
        }
    "#;

    let result = client
        .mutate(
            mutation,
            Some(serde_json::json!({ "id": issue_id, "stateId": state_id })),
        )
        .await?;

    let success = result["data"]["issueUpdate"]["success"]
        .as_bool()
        .unwrap_or(false);

    if !success {
        anyhow::bail!("Failed to update issue {}", issue_id);
    }

    if output.is_json() || output.has_template() {
        let updated = &result["data"]["issueUpdate"]["issue"];
        print_json_owned(
            serde_json::json!({
                "issue_id": issue_id,
                "status": status,
                "updated": updated,
            }),
            output,
        )?;
    } else if !agent_opts.quiet {
        let identifier = result["data"]["issueUpdate"]["issue"]["identifier"]
            .as_str()
            .unwrap_or(&issue_id);
        let new_state = result["data"]["issueUpdate"]["issue"]["state"]["name"]
            .as_str()
            .unwrap_or(status);
        println!("+ {} -> {}", identifier, new_state);
    }

    Ok(())
}

/// Handle the hidden `_complete` command -- output dynamic completion values
async fn handle_complete(type_: &str, prefix: &str, team: Option<&str>) -> Result<()> {
    // Try to use cache first for fast completions; fall back to API
    let cache = cache::Cache::new().ok();

    match type_ {
        "teams" => complete_teams(cache.as_ref(), prefix).await,
        "projects" => complete_projects(cache.as_ref(), prefix).await,
        "issues" => complete_issues(prefix).await,
        "statuses" => complete_statuses(cache.as_ref(), prefix, team).await,
        "users" => complete_users(cache.as_ref(), prefix).await,
        "labels" => complete_labels(cache.as_ref(), prefix).await,
        _ => Ok(()), // Unknown type, return empty
    }
}

/// Complete team keys with descriptions
async fn complete_teams(cache: Option<&cache::Cache>, prefix: &str) -> Result<()> {
    let teams = if let Some(data) = cache.and_then(|c| c.get(cache::CacheType::Teams)) {
        data.as_array().cloned().unwrap_or_default()
    } else {
        match fetch_teams_for_completion().await {
            Ok(t) => t,
            Err(_) => return Ok(()),
        }
    };

    let prefix_lower = prefix.to_lowercase();
    for team in &teams {
        let key = team["key"].as_str().unwrap_or("");
        let name = team["name"].as_str().unwrap_or("");
        if prefix.is_empty()
            || key.to_lowercase().starts_with(&prefix_lower)
            || name.to_lowercase().starts_with(&prefix_lower)
        {
            println!(
                "{}\t{}",
                sanitize_completion_value(key),
                sanitize_completion_field(name)
            );
        }
    }
    Ok(())
}

/// Complete project names with descriptions
async fn complete_projects(cache: Option<&cache::Cache>, prefix: &str) -> Result<()> {
    let projects = if let Some(data) = cache.and_then(|c| c.get(cache::CacheType::Projects)) {
        data.as_array().cloned().unwrap_or_default()
    } else {
        match fetch_projects_for_completion().await {
            Ok(p) => p,
            Err(_) => return Ok(()),
        }
    };

    let prefix_lower = prefix.to_lowercase();
    for project in &projects {
        let name = project["name"].as_str().unwrap_or("");
        let state = project["state"].as_str().unwrap_or("");
        if prefix.is_empty() || name.to_lowercase().starts_with(&prefix_lower) {
            println!(
                "{}\t{}",
                sanitize_completion_value(name),
                sanitize_completion_field(state)
            );
        }
    }
    Ok(())
}

/// Complete issue identifiers with titles (no cache -- issues change frequently)
async fn complete_issues(prefix: &str) -> Result<()> {
    let client = match api::LinearClient::new() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let query = r#"
        query($first: Int) {
            issues(first: $first, orderBy: updatedAt) {
                nodes {
                    identifier
                    title
                }
            }
        }
    "#;

    let result = match client
        .query(query, Some(serde_json::json!({ "first": 50 })))
        .await
    {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let empty = vec![];
    let issues = result["data"]["issues"]["nodes"]
        .as_array()
        .unwrap_or(&empty);

    let prefix_upper = prefix.to_uppercase();
    for issue in issues {
        let id = issue["identifier"].as_str().unwrap_or("");
        let title = issue["title"].as_str().unwrap_or("");
        if prefix.is_empty() || id.to_uppercase().starts_with(&prefix_upper) {
            println!(
                "{}\t{}",
                sanitize_completion_value(id),
                sanitize_completion_field(title)
            );
        }
    }
    Ok(())
}

/// Complete workflow state names with category
async fn complete_statuses(
    cache: Option<&cache::Cache>,
    prefix: &str,
    team: Option<&str>,
) -> Result<()> {
    let client = match api::LinearClient::new() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    // Resolve team to get statuses -- if no team given, try to get first team as default
    let team_id = if let Some(t) = team {
        match api::resolve_team_id(
            &client,
            t,
            &cache::CacheOptions {
                ttl_seconds: None,
                no_cache: false,
            },
        )
        .await
        {
            Ok(id) => id,
            Err(_) => return Ok(()),
        }
    } else {
        // Try to get first team as default
        let teams = if let Some(data) = cache.and_then(|c| c.get(cache::CacheType::Teams)) {
            data.as_array().cloned().unwrap_or_default()
        } else {
            match fetch_teams_for_completion().await {
                Ok(t) => t,
                Err(_) => return Ok(()),
            }
        };
        match teams.first().and_then(|t| t["id"].as_str()) {
            Some(id) => id.to_string(),
            None => return Ok(()),
        }
    };

    // Check cache for statuses
    let states = if let Some(cached) =
        cache.and_then(|c| c.get_keyed(cache::CacheType::Statuses, &team_id))
    {
        cached["states"].as_array().cloned().unwrap_or_default()
    } else {
        // Fetch from API
        let query = r#"
                query($teamId: String!) {
                    team(id: $teamId) {
                        states {
                            nodes {
                                id
                                name
                                type
                            }
                        }
                    }
                }
            "#;

        match client
            .query(query, Some(serde_json::json!({ "teamId": team_id })))
            .await
        {
            Ok(result) => result["data"]["team"]["states"]["nodes"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            Err(_) => return Ok(()),
        }
    };

    let prefix_lower = prefix.to_lowercase();
    for state in &states {
        let name = state["name"].as_str().unwrap_or("");
        let type_ = state["type"].as_str().unwrap_or("");
        if prefix.is_empty() || name.to_lowercase().starts_with(&prefix_lower) {
            println!(
                "{}\t{}",
                sanitize_completion_value(name),
                sanitize_completion_field(type_)
            );
        }
    }
    Ok(())
}

/// Complete user display names / emails
async fn complete_users(cache: Option<&cache::Cache>, prefix: &str) -> Result<()> {
    let users = if let Some(data) = cache.and_then(|c| c.get(cache::CacheType::Users)) {
        data.as_array().cloned().unwrap_or_default()
    } else {
        match fetch_users_for_completion().await {
            Ok(u) => u,
            Err(_) => return Ok(()),
        }
    };

    let prefix_lower = prefix.to_lowercase();
    for user in &users {
        let name = user["name"].as_str().unwrap_or("");
        let email = user["email"].as_str().unwrap_or("");
        let display = if !email.is_empty() { email } else { name };
        if prefix.is_empty()
            || name.to_lowercase().starts_with(&prefix_lower)
            || email.to_lowercase().starts_with(&prefix_lower)
        {
            println!(
                "{}\t{}",
                sanitize_completion_value(display),
                sanitize_completion_field(name)
            );
        }
    }
    Ok(())
}

/// Complete label names
async fn complete_labels(cache: Option<&cache::Cache>, prefix: &str) -> Result<()> {
    let labels = if let Some(data) = cache.and_then(|c| c.get(cache::CacheType::Labels)) {
        data.as_array().cloned().unwrap_or_default()
    } else {
        match fetch_labels_for_completion().await {
            Ok(l) => l,
            Err(_) => return Ok(()),
        }
    };

    let prefix_lower = prefix.to_lowercase();
    for label in &labels {
        let name = label["name"].as_str().unwrap_or("");
        let color = label["color"].as_str().unwrap_or("");
        if prefix.is_empty() || name.to_lowercase().starts_with(&prefix_lower) {
            println!(
                "{}\t{}",
                sanitize_completion_value(name),
                sanitize_completion_field(color)
            );
        }
    }
    Ok(())
}

/// Fetch teams from API for completion (lightweight query)
async fn fetch_teams_for_completion() -> Result<Vec<serde_json::Value>> {
    let client = api::LinearClient::new()?;
    let query = r#"
        query {
            teams(first: 50) {
                nodes { id name key }
            }
        }
    "#;
    let result = client.query(query, None).await?;
    Ok(result["data"]["teams"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// Fetch projects from API for completion (lightweight query)
async fn fetch_projects_for_completion() -> Result<Vec<serde_json::Value>> {
    let client = api::LinearClient::new()?;
    let query = r#"
        query {
            projects(first: 50, orderBy: updatedAt) {
                nodes { id name state }
            }
        }
    "#;
    let result = client.query(query, None).await?;
    Ok(result["data"]["projects"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// Fetch users from API for completion (lightweight query)
async fn fetch_users_for_completion() -> Result<Vec<serde_json::Value>> {
    let client = api::LinearClient::new()?;
    let query = r#"
        query {
            users(first: 50) {
                nodes { id name email }
            }
        }
    "#;
    let result = client.query(query, None).await?;
    Ok(result["data"]["users"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// Fetch labels from API for completion (lightweight query)
async fn fetch_labels_for_completion() -> Result<Vec<serde_json::Value>> {
    let client = api::LinearClient::new()?;
    let query = r#"
        query {
            issueLabels(first: 50) {
                nodes { id name color }
            }
        }
    "#;
    let result = client.query(query, None).await?;
    Ok(result["data"]["issueLabels"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// Print shell-specific dynamic completion script
fn print_dynamic_completion_script(shell: Shell) {
    match shell {
        Shell::Bash => print!("{}", BASH_DYNAMIC_COMPLETIONS),
        Shell::Zsh => print!("{}", ZSH_DYNAMIC_COMPLETIONS),
        Shell::Fish => print!("{}", FISH_DYNAMIC_COMPLETIONS),
        Shell::PowerShell => print!("{}", POWERSHELL_DYNAMIC_COMPLETIONS),
        _ => {
            eprintln!("Dynamic completions not supported for this shell. Supported: bash, zsh, fish, powershell");
        }
    }
}

const BASH_DYNAMIC_COMPLETIONS: &str = r#"# Dynamic completions for linear-cli (bash)
# Source this file or add to ~/.bashrc:
#   eval "$(linear-cli completions dynamic bash)"

_linear_cli_dynamic() {
    _linear_cli_collect() {
        local type="$1"
        shift
        local line value
        COMPREPLY=()
        while IFS=$'\t' read -r value _; do
            [[ -z "$value" ]] && continue
            [[ "$value" == "$cur"* ]] && COMPREPLY+=("$value")
        done < <(linear-cli _complete --type "$type" --prefix "$cur" "$@" 2>/dev/null)
    }

    local cur prev
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    # Find --team value in command line for scoped completions
    local team_val=""
    local i
    for (( i=1; i < COMP_CWORD; i++ )); do
        if [[ "${COMP_WORDS[i]}" == "-t" || "${COMP_WORDS[i]}" == "--team" ]]; then
            team_val="${COMP_WORDS[i+1]}"
            break
        fi
    done

    case "$prev" in
        -t|--team)
            _linear_cli_collect teams
            return 0
            ;;
        -s|--status)
            if [[ -n "$team_val" ]]; then
                _linear_cli_collect statuses --team "$team_val"
            else
                _linear_cli_collect statuses
            fi
            return 0
            ;;
        --project)
            _linear_cli_collect projects
            return 0
            ;;
        --label|-l)
            _linear_cli_collect labels
            return 0
            ;;
        --assignee|--user)
            _linear_cli_collect users
            return 0
            ;;
    esac

    # Complete issue IDs when the previous word is a subcommand that takes an issue
    local subcmds_taking_issue="get update start close done archive unarchive comment link assign move transfer open"
    for subcmd in $subcmds_taking_issue; do
        if [[ "$prev" == "$subcmd" ]]; then
            _linear_cli_collect issues
            return 0
        fi
    done
}

# Register the dynamic completion function
complete -o default -F _linear_cli_dynamic linear-cli
"#;

const ZSH_DYNAMIC_COMPLETIONS: &str = r#"# Dynamic completions for linear-cli (zsh)
# Source this file or add to ~/.zshrc:
#   eval "$(linear-cli completions dynamic zsh)"

_linear_cli_dynamic() {
    local -a completions
    local team_val=""

    # Extract --team value from command line
    local -i i
    for (( i=2; i < CURRENT; i++ )); do
        if [[ "$words[$i]" == "-t" || "$words[$i]" == "--team" ]]; then
            team_val="$words[$((i+1))]"
            break
        fi
    done

    case "$words[$((CURRENT-1))]" in
        -t|--team)
            completions=(${(f)"$(linear-cli _complete --type teams --prefix "$words[$CURRENT]" 2>/dev/null)"})
            _describe 'team' completions
            return
            ;;
        -s|--status)
            local team_arg=""
            if [[ -n "$team_val" ]]; then
                team_arg="--team $team_val"
            fi
            completions=(${(f)"$(linear-cli _complete --type statuses --prefix "$words[$CURRENT]" ${=team_arg} 2>/dev/null)"})
            _describe 'status' completions
            return
            ;;
        --project)
            completions=(${(f)"$(linear-cli _complete --type projects --prefix "$words[$CURRENT]" 2>/dev/null)"})
            _describe 'project' completions
            return
            ;;
        --label|-l)
            completions=(${(f)"$(linear-cli _complete --type labels --prefix "$words[$CURRENT]" 2>/dev/null)"})
            _describe 'label' completions
            return
            ;;
        --assignee|--user)
            completions=(${(f)"$(linear-cli _complete --type users --prefix "$words[$CURRENT]" 2>/dev/null)"})
            _describe 'user' completions
            return
            ;;
        get|update|start|close|done|archive|unarchive|comment|link|assign|move|transfer|open)
            completions=(${(f)"$(linear-cli _complete --type issues --prefix "$words[$CURRENT]" 2>/dev/null)"})
            _describe 'issue' completions
            return
            ;;
    esac
}

compdef _linear_cli_dynamic linear-cli
"#;

const FISH_DYNAMIC_COMPLETIONS: &str = r#"# Dynamic completions for linear-cli (fish)
# Source this file or save to ~/.config/fish/completions/linear-cli-dynamic.fish:
#   linear-cli completions dynamic fish > ~/.config/fish/completions/linear-cli-dynamic.fish

# Team completions
complete -c linear-cli -l team -s t -x -a '(linear-cli _complete --type teams 2>/dev/null | string replace \t "\t")'

# Status completions (tries to pick up --team from current command line)
complete -c linear-cli -l status -s s -x -a '(linear-cli _complete --type statuses 2>/dev/null | string replace \t "\t")'

# Project completions
complete -c linear-cli -l project -x -a '(linear-cli _complete --type projects 2>/dev/null | string replace \t "\t")'

# Label completions
complete -c linear-cli -l label -s l -x -a '(linear-cli _complete --type labels 2>/dev/null | string replace \t "\t")'

# User completions
complete -c linear-cli -l assignee -x -a '(linear-cli _complete --type users 2>/dev/null | string replace \t "\t")'
complete -c linear-cli -l user -x -a '(linear-cli _complete --type users 2>/dev/null | string replace \t "\t")'

# Issue ID completions for subcommands that take an issue
for subcmd in get update start close done archive unarchive comment link assign move transfer open
    complete -c linear-cli -n "__fish_seen_subcommand_from $subcmd" -x -a '(linear-cli _complete --type issues 2>/dev/null | string replace \t "\t")'
end
"#;

const POWERSHELL_DYNAMIC_COMPLETIONS: &str = r#"# Dynamic completions for linear-cli (PowerShell)
# Source this file or add to your $PROFILE:
#   linear-cli completions dynamic powershell | Invoke-Expression

Register-ArgumentCompleter -CommandName linear-cli -ScriptBlock {
    param($commandName, $wordToComplete, $cursorPosition)

    $tokens = $wordToComplete -split '\s+'
    $prev = if ($tokens.Length -gt 1) { $tokens[-2] } else { '' }
    $current = $tokens[-1]

    # Find --team value in tokens
    $teamVal = ''
    for ($i = 0; $i -lt $tokens.Length; $i++) {
        if ($tokens[$i] -eq '-t' -or $tokens[$i] -eq '--team') {
            if ($i + 1 -lt $tokens.Length) { $teamVal = $tokens[$i + 1] }
            break
        }
    }

    $type = switch ($prev) {
        { $_ -in '-t', '--team' } { 'teams' }
        { $_ -in '-s', '--status' } { 'statuses' }
        '--project' { 'projects' }
        { $_ -in '-l', '--label' } { 'labels' }
        { $_ -in '--assignee', '--user' } { 'users' }
        { $_ -in 'get', 'update', 'start', 'close', 'done', 'archive', 'unarchive', 'comment', 'link', 'assign', 'move', 'transfer', 'open' } { 'issues' }
        default { $null }
    }

    if ($type) {
        $teamArg = if ($teamVal -and $type -eq 'statuses') { "--team $teamVal" } else { '' }
        $results = linear-cli _complete --type $type --prefix $current $teamArg 2>$null
        if ($results) {
            $results | ForEach-Object {
                $parts = $_ -split '\t', 2
                $value = $parts[0]
                $desc = if ($parts.Length -gt 1) { $parts[1] } else { '' }
                [System.Management.Automation.CompletionResult]::new($value, $value, 'ParameterValue', $desc)
            }
        }
    }
}
"#;
