//! Command-level dry-run policy.
//!
//! Commands that own a non-API side effect must reject `--dry-run` before
//! authentication or dispatch. API mutations have a second fail-closed guard
//! in [`crate::api::LinearClient::mutate`], so adding a new mutation cannot
//! accidentally turn a preview into a write if this audit list is missed.

use crate::commands::{
    attachments, auth, comments, cycles, documents, export, favorites, git, initiatives, labels,
    notifications, project_updates, projects, roadmaps, sprint, teams, templates, time, triage,
    uploads, webhooks,
};
use crate::{Commands, ConfigCommands};

/// Return the command name for a dry-run-incompatible operation.
///
/// Keep this list explicit for operations that write local state, start an
/// interactive process, or otherwise have no preview. GraphQL mutations also
/// remain protected by the process-wide client guard, which is the defense in
/// depth for mutation paths added after this list was written.
pub(crate) fn unsupported_command(command: &Commands) -> Option<&'static str> {
    match command {
        Commands::Attachments {
            action:
                attachments::AttachmentCommands::Create { .. }
                | attachments::AttachmentCommands::Update { .. }
                | attachments::AttachmentCommands::Delete { .. }
                | attachments::AttachmentCommands::LinkUrl { .. },
        } => Some("attachments"),
        Commands::Bulk { .. } => Some("bulk"),
        Commands::Cycles { action }
            if !matches!(
                action,
                cycles::CycleCommands::Update { .. }
                    | cycles::CycleCommands::List { .. }
                    | cycles::CycleCommands::Get { .. }
                    | cycles::CycleCommands::Current { .. }
            ) =>
        {
            Some("cycles")
        }
        Commands::Comments {
            action:
                comments::CommentCommands::Create { .. }
                | comments::CommentCommands::Update { .. }
                | comments::CommentCommands::Delete { .. },
        } => Some("comments"),
        Commands::Favorites {
            action:
                favorites::FavoriteCommands::Add { .. } | favorites::FavoriteCommands::Remove { .. },
        } => Some("favorites"),
        Commands::Labels {
            action:
                labels::LabelCommands::Create { .. }
                | labels::LabelCommands::Delete { .. }
                | labels::LabelCommands::Update { .. },
        } => Some("labels"),
        Commands::Notifications {
            action:
                notifications::NotificationCommands::Read { .. }
                | notifications::NotificationCommands::ReadAll
                | notifications::NotificationCommands::Archive { .. }
                | notifications::NotificationCommands::ArchiveAll,
        } => Some("notifications"),
        Commands::ProjectUpdates {
            action:
                project_updates::ProjectUpdateCommands::Create { .. }
                | project_updates::ProjectUpdateCommands::Update { .. }
                | project_updates::ProjectUpdateCommands::Archive { .. }
                | project_updates::ProjectUpdateCommands::Unarchive { .. },
        } => Some("project-updates"),
        Commands::Projects { action }
            if !matches!(
                action,
                projects::ProjectCommands::Update { .. }
                    | projects::ProjectCommands::List { .. }
                    | projects::ProjectCommands::Get { .. }
                    | projects::ProjectCommands::Members { .. }
            ) =>
        {
            Some("projects")
        }
        Commands::Documents {
            action: documents::DocumentCommands::Create { .. },
        } => Some("documents"),
        Commands::Roadmaps {
            action:
                roadmaps::RoadmapCommands::Create { .. } | roadmaps::RoadmapCommands::Delete { .. },
        } => Some("roadmaps"),
        Commands::Initiatives {
            action:
                initiatives::InitiativeCommands::Create { .. }
                | initiatives::InitiativeCommands::Delete { .. },
        } => Some("initiatives"),
        Commands::Milestones {
            action: crate::commands::milestones::MilestoneCommands::Delete { .. },
        } => Some("milestones"),
        Commands::Issues { action }
            if !matches!(
                action,
                crate::commands::issues::IssueCommands::Create { .. }
                    | crate::commands::issues::IssueCommands::Update { .. }
                    | crate::commands::issues::IssueCommands::List { .. }
                    | crate::commands::issues::IssueCommands::Get { .. }
                    | crate::commands::issues::IssueCommands::Link { .. }
            ) =>
        {
            Some("issues")
        }
        Commands::Relations { action }
            if !matches!(
                action,
                crate::commands::relations::RelationCommands::List { .. }
            ) =>
        {
            Some("relations")
        }
        Commands::Sprint {
            action: sprint::SprintCommands::CarryOver { .. },
        } => Some("sprint"),
        Commands::Teams {
            action:
                teams::TeamCommands::Create { .. }
                | teams::TeamCommands::Update { .. }
                | teams::TeamCommands::Delete { .. },
        } => Some("teams"),
        Commands::Time {
            action:
                time::TimeCommands::Log { .. }
                | time::TimeCommands::Delete { .. }
                | time::TimeCommands::Update { .. },
        } => Some("time"),
        Commands::Triage {
            action: triage::TriageCommands::Claim { .. } | triage::TriageCommands::Snooze { .. },
        } => Some("triage"),
        Commands::Api {
            action: crate::commands::api::ApiCommands::Mutate { .. },
        } => Some("api mutate"),
        Commands::Api {
            action: crate::commands::api::ApiCommands::Query { .. },
        } => Some("api query"),
        Commands::Interactive { .. } => Some("interactive"),
        Commands::Git {
            action:
                git::GitCommands::Checkout { .. }
                | git::GitCommands::Create { .. }
                | git::GitCommands::Pr { .. },
        } => Some("git"),
        Commands::Done { .. } => Some("done"),
        Commands::Setup => Some("setup"),
        Commands::Auth { action } if !matches!(action, auth::AuthCommands::Status { .. }) => {
            Some("auth")
        }
        Commands::Cache {
            action: crate::commands::cache::CacheCommands::Clear { .. },
        } => Some("cache"),
        Commands::Config {
            action:
                ConfigCommands::SetKey
                | ConfigCommands::Set { .. }
                | ConfigCommands::WorkspaceAdd { .. }
                | ConfigCommands::WorkspaceSwitch { .. }
                | ConfigCommands::WorkspaceRemove { .. },
        } => Some("config"),
        Commands::Update { check: false } => Some("update"),
        Commands::Doctor { fix: true, .. } => Some("doctor"),
        Commands::Webhooks {
            action:
                webhooks::WebhookCommands::RotateSecret { .. }
                | webhooks::WebhookCommands::Listen { .. },
        } => Some("webhooks"),
        Commands::Templates {
            action:
                templates::TemplateCommands::Delete { .. }
                | templates::TemplateCommands::RemoteCreate { .. }
                | templates::TemplateCommands::RemoteUpdate { .. }
                | templates::TemplateCommands::RemoteDelete { .. },
        } => Some("templates"),
        Commands::Uploads {
            action: uploads::UploadCommands::Fetch { file: Some(_), .. },
        } => Some("uploads"),
        Commands::Export {
            action:
                export::ExportCommands::Csv { file: Some(_), .. }
                | export::ExportCommands::Markdown { file: Some(_), .. }
                | export::ExportCommands::Json { file: Some(_), .. }
                | export::ExportCommands::ProjectsCsv { file: Some(_), .. },
        } => Some("export"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::relations::RelationCommands;

    #[test]
    fn mutation_families_are_rejected_before_dispatch() {
        let cases = [
            (
                Commands::Bulk {
                    action: crate::commands::bulk::BulkCommands::Assign {
                        user: "me".to_string(),
                        issues: vec!["LIN-1".to_string()],
                    },
                },
                "bulk",
            ),
            (
                Commands::Relations {
                    action: RelationCommands::List {
                        id: "LIN-1".to_string(),
                    },
                },
                "",
            ),
            (
                Commands::Api {
                    action: crate::commands::api::ApiCommands::Mutate {
                        query: Some("mutation { noop }".to_string()),
                        variables: vec![],
                    },
                },
                "api mutate",
            ),
        ];

        assert_eq!(unsupported_command(&cases[0].0), Some(cases[0].1));
        assert_eq!(unsupported_command(&cases[1].0), None);
        assert_eq!(unsupported_command(&cases[2].0), Some(cases[2].1));
    }

    #[test]
    fn read_only_variants_are_not_rejected() {
        let command = Commands::Teams {
            action: teams::TeamCommands::List,
        };
        assert_eq!(unsupported_command(&command), None);
    }

    #[test]
    fn browser_open_variants_are_rejected() {
        let project = Commands::Projects {
            action: projects::ProjectCommands::Open {
                id: "project-id".to_string(),
            },
        };
        let issue = Commands::Issues {
            action: crate::commands::issues::IssueCommands::Open {
                id: "LIN-1".to_string(),
            },
        };

        assert_eq!(unsupported_command(&project), Some("projects"));
        assert_eq!(unsupported_command(&issue), Some("issues"));
    }

    #[test]
    fn raw_api_queries_are_rejected() {
        let command = Commands::Api {
            action: crate::commands::api::ApiCommands::Query {
                query: Some("{ viewer { id } }".to_string()),
                variables: vec![],
                paginate: false,
                nodes_path: String::new(),
                page_info_path: String::new(),
            },
        };

        assert_eq!(unsupported_command(&command), Some("api query"));
    }
}
