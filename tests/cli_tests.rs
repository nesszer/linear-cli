use std::process::Command;

/// Helper to run CLI commands and capture output
fn run_cli(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_linear-cli"))
        .args(args)
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);

    (code, stdout, stderr)
}

#[test]
fn test_help_command() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("A powerful CLI for Linear.app"));
    assert!(stdout.contains("Commands:"));
}

#[test]
fn test_update_in_top_level_help() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("update"));
}

#[test]
fn test_version_command() {
    let (code, stdout, _stderr) = run_cli(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("linear") || stdout.contains("0.1"));
}

#[test]
fn test_update_help() {
    let (code, stdout, _stderr) = run_cli(&["update", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--check"));
    assert!(stdout.contains("linear-cli update"));
}

#[test]
fn test_projects_help() {
    let (code, stdout, _stderr) = run_cli(&["projects", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("members"));
}

#[test]
fn test_projects_members_help() {
    let (code, stdout, _stderr) = run_cli(&["projects", "members", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("List project members"),
        "projects members should show help"
    );
}

#[test]
fn test_issues_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("start"));
    assert!(stdout.contains("stop"));
}

#[test]
fn test_teams_help() {
    let (code, stdout, _stderr) = run_cli(&["teams", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("members"));
}

#[test]
fn test_teams_members_help() {
    let (code, stdout, _stderr) = run_cli(&["teams", "members", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("List members"),
        "teams members should show help"
    );
}

#[test]
fn test_config_help() {
    let (code, stdout, _stderr) = run_cli(&["config", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("set-key"));
    assert!(stdout.contains("show"));
    assert!(stdout.contains("workspace-add"));
    assert!(stdout.contains("workspace-list"));
}

#[test]
fn test_top_level_help_does_not_expose_api_key_flag() {
    let (code, _stdout, stderr) = run_cli(&["--api-key", "dummy", "common"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("unexpected argument '--api-key'") || stderr.contains("--api-key"));
}

#[test]
fn test_config_set_help_only_allows_profile() {
    let (code, _stdout, stderr) = run_cli(&["config", "set", "api-key", "dummy"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("invalid value") || stderr.contains("api-key"));
}

#[test]
fn test_bulk_help() {
    let (code, stdout, _stderr) = run_cli(&["bulk", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("update-state"));
    assert!(stdout.contains("assign"));
    assert!(stdout.contains("label"));
}

#[test]
fn test_search_help() {
    let (code, stdout, _stderr) = run_cli(&["search", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("issues"));
    assert!(stdout.contains("projects"));
}

#[test]
fn test_git_help() {
    let (code, stdout, _stderr) = run_cli(&["git", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("checkout"));
    assert!(stdout.contains("branch"));
}

#[test]
fn test_sync_help() {
    let (code, stdout, _stderr) = run_cli(&["sync", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("status"));
    assert!(stdout.contains("push"));
}

#[test]
fn test_aliases_work() {
    // Test short aliases
    let (code1, stdout1, _) = run_cli(&["p", "--help"]);
    let (code2, stdout2, _) = run_cli(&["projects", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);

    let (code3, stdout3, _) = run_cli(&["i", "--help"]);
    let (code4, stdout4, _) = run_cli(&["issues", "--help"]);
    assert_eq!(code3, 0);
    assert_eq!(code4, 0);
    assert_eq!(stdout3, stdout4);
}

#[test]
fn test_output_format_option() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--output"));
    assert!(stdout.contains("table"));
    assert!(stdout.contains("json"));
}

#[test]
fn test_invalid_command() {
    let (code, _stdout, stderr) = run_cli(&["invalid-command"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("error") || stderr.contains("invalid"));
}

// --- Additional alias tests ---

#[test]
fn test_teams_alias() {
    let (code1, stdout1, _) = run_cli(&["t", "--help"]);
    let (code2, stdout2, _) = run_cli(&["teams", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_comments_alias() {
    let (code1, stdout1, _) = run_cli(&["cm", "--help"]);
    let (code2, stdout2, _) = run_cli(&["comments", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_git_alias() {
    let (code1, stdout1, _) = run_cli(&["g", "--help"]);
    let (code2, stdout2, _) = run_cli(&["git", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_search_alias() {
    let (code1, stdout1, _) = run_cli(&["s", "--help"]);
    let (code2, stdout2, _) = run_cli(&["search", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

// --- Help text completeness ---

#[test]
fn test_notifications_help() {
    let (code, stdout, _stderr) = run_cli(&["notifications", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("read"));
}

#[test]
fn test_labels_help() {
    let (code, stdout, _stderr) = run_cli(&["labels", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("update"));
}

#[test]
fn test_labels_update_help() {
    let (code, stdout, _stderr) = run_cli(&["labels", "update", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Update a label"),
        "labels update should show help"
    );
    assert!(stdout.contains("--name"), "should accept --name flag");
    assert!(stdout.contains("--color"), "should accept --color flag");
}

#[test]
fn test_cycles_help() {
    let (code, stdout, _stderr) = run_cli(&["cycles", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("get"));
}

#[test]
fn test_cycles_get_help() {
    let (code, stdout, _stderr) = run_cli(&["cycles", "get", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Get cycle details"),
        "cycles get should show help"
    );
}

#[test]
fn test_cache_help() {
    let (code, stdout, _stderr) = run_cli(&["cache", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("status"));
    assert!(stdout.contains("clear"));
}

#[test]
fn test_export_help() {
    let (code, stdout, _stderr) = run_cli(&["export", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("csv"));
}

#[test]
fn test_uploads_help() {
    let (code, stdout, _stderr) = run_cli(&["uploads", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("fetch"));
}

// --- Subcommand help tests ---

#[test]
fn test_issues_list_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--team"));
    assert!(stdout.contains("--state"));
    assert!(stdout.contains("--assignee"));
}

#[test]
fn test_issues_create_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--team"));
    assert!(stdout.contains("--priority"));
    assert!(stdout.contains("--description"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_bulk_update_state_help() {
    let (code, stdout, _stderr) = run_cli(&["bulk", "update-state", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("STATE"));
    assert!(stdout.contains("--issues"));
}

#[test]
fn test_bulk_requires_issue_ids_with_actionable_example() {
    let (code, _stdout, stderr) = run_cli(&["bulk", "update-state", "Done"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("No issues specified"));
    assert!(stderr.contains("linear bulk update-state Done -i LIN-1,LIN-2"));
}

// --- Global flags ---

#[test]
fn test_quiet_flag_exists() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--quiet") || stdout.contains("-q"));
}

#[test]
fn test_dry_run_flag_exists() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_compact_flag_exists() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--compact"));
}

#[test]
fn test_fields_flag_exists() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--fields"));
}

// --- CLI name consistency ---

#[test]
fn test_binary_name_in_help() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    // The help should show the binary name
    assert!(
        stdout.contains("linear-cli") || stdout.contains("Usage:"),
        "Help output should contain binary name or usage info"
    );
}

#[test]
fn test_version_contains_semver() {
    let (code, stdout, _stderr) = run_cli(&["--version"]);
    assert_eq!(code, 0);
    // Version should contain a semver-like pattern (digit.digit)
    assert!(
        stdout.chars().any(|c| c == '.'),
        "Version output should contain a dot-separated version number"
    );
}

// --- Help tests for commands without coverage ---

#[test]
fn test_time_help() {
    let (code, stdout, _stderr) = run_cli(&["time", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("log"));
    assert!(stdout.contains("list"));
}

#[test]
fn test_relations_help() {
    let (code, stdout, _stderr) = run_cli(&["relations", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("add"));
    assert!(stdout.contains("remove"));
}

#[test]
fn test_favorites_help() {
    let (code, stdout, _stderr) = run_cli(&["favorites", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("add"));
}

#[test]
fn test_roadmaps_help() {
    let (code, stdout, _stderr) = run_cli(&["roadmaps", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("get"));
}

#[test]
fn test_initiatives_help() {
    let (code, stdout, _stderr) = run_cli(&["initiatives", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("get"));
}

#[test]
fn test_documents_help() {
    let (code, stdout, _stderr) = run_cli(&["documents", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("get"));
    assert!(stdout.contains("create"));
}

#[test]
fn test_context_help() {
    let (code, stdout, _stderr) = run_cli(&["context", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("context") || stdout.contains("issue") || stdout.contains("branch"));
}

// --- Alias tests for commands without coverage ---

#[test]
fn test_time_alias() {
    let (code1, stdout1, _) = run_cli(&["tm", "--help"]);
    let (code2, stdout2, _) = run_cli(&["time", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_relations_alias() {
    let (code1, stdout1, _) = run_cli(&["rel", "--help"]);
    let (code2, stdout2, _) = run_cli(&["relations", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_favorites_alias() {
    let (code1, stdout1, _) = run_cli(&["fav", "--help"]);
    let (code2, stdout2, _) = run_cli(&["favorites", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_roadmaps_alias() {
    let (code1, stdout1, _) = run_cli(&["rm", "--help"]);
    let (code2, stdout2, _) = run_cli(&["roadmaps", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_initiatives_alias() {
    let (code1, stdout1, _) = run_cli(&["init", "--help"]);
    let (code2, stdout2, _) = run_cli(&["initiatives", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_documents_alias() {
    let (code1, stdout1, _) = run_cli(&["d", "--help"]);
    let (code2, stdout2, _) = run_cli(&["documents", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_context_alias() {
    let (code1, stdout1, _) = run_cli(&["ctx", "--help"]);
    let (code2, stdout2, _) = run_cli(&["context", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

// --- v0.3.4 new subcommand tests ---

#[test]
fn test_watch_help() {
    let (code, stdout, _stderr) = run_cli(&["watch", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("issue"));
    assert!(stdout.contains("project"));
    assert!(stdout.contains("team"));
}

#[test]
fn test_watch_issue_help() {
    let (code, stdout, _stderr) = run_cli(&["watch", "issue", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--interval"));
}

#[test]
fn test_watch_project_help() {
    let (code, stdout, _stderr) = run_cli(&["watch", "project", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--interval"));
}

#[test]
fn test_watch_team_help() {
    let (code, stdout, _stderr) = run_cli(&["watch", "team", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--interval"));
}

#[test]
fn test_roadmaps_create_help() {
    let (code, stdout, _stderr) = run_cli(&["roadmaps", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--description"));
}

#[test]
fn test_roadmaps_update_help() {
    let (code, stdout, _stderr) = run_cli(&["roadmaps", "update", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--name"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_initiatives_create_help() {
    let (code, stdout, _stderr) = run_cli(&["initiatives", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--description"));
    assert!(stdout.contains("--status"));
}

#[test]
fn test_initiatives_update_help() {
    let (code, stdout, _stderr) = run_cli(&["initiatives", "update", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--name"));
    assert!(stdout.contains("--status"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_documents_delete_help() {
    let (code, stdout, _stderr) = run_cli(&["documents", "delete", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--force"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_roadmaps_help_includes_create() {
    let (code, stdout, _stderr) = run_cli(&["roadmaps", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("update"));
}

#[test]
fn test_initiatives_help_includes_create() {
    let (code, stdout, _stderr) = run_cli(&["initiatives", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("update"));
}

#[test]
fn test_documents_help_includes_delete() {
    let (code, stdout, _stderr) = run_cli(&["documents", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("delete"));
}

// --- v0.3.5 new subcommand tests ---

#[test]
fn test_triage_help() {
    let (code, stdout, _stderr) = run_cli(&["triage", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("claim"));
}

#[test]
fn test_triage_alias() {
    let (code1, stdout1, _) = run_cli(&["tr", "--help"]);
    let (code2, stdout2, _) = run_cli(&["triage", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_notifications_archive_help() {
    let (code, stdout, _stderr) = run_cli(&["notifications", "archive", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("id") || stdout.contains("ID"));
}

#[test]
fn test_notifications_archive_all_help() {
    let (code, _stdout, _stderr) = run_cli(&["notifications", "archive-all", "--help"]);
    assert_eq!(code, 0);
}

#[test]
fn test_notifications_help_includes_archive() {
    let (code, stdout, _stderr) = run_cli(&["notifications", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("archive"));
}

#[test]
fn test_cycles_create_help() {
    let (code, stdout, _stderr) = run_cli(&["cycles", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--team"));
    assert!(stdout.contains("--name"));
    assert!(stdout.contains("--starts-at"));
    assert!(stdout.contains("--ends-at"));
}

#[test]
fn test_cycles_update_help() {
    let (code, stdout, _stderr) = run_cli(&["cycles", "update", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--name"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_cycles_help_includes_create() {
    let (code, stdout, _stderr) = run_cli(&["cycles", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("update"));
}

// --- v0.3.6 OAuth tests ---

#[test]
fn test_auth_help_includes_oauth() {
    let (code, stdout, _stderr) = run_cli(&["auth", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("oauth"),
        "auth help should list oauth subcommand"
    );
    assert!(
        stdout.contains("revoke"),
        "auth help should list revoke subcommand"
    );
    assert!(stdout.contains("login"));
    assert!(stdout.contains("logout"));
    assert!(stdout.contains("status"));
}

#[test]
fn test_auth_oauth_help() {
    let (code, stdout, _stderr) = run_cli(&["auth", "oauth", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--client-id"));
    assert!(stdout.contains("--scopes"));
    assert!(stdout.contains("--port"));
    assert!(stdout.contains("--secure"));
}

#[test]
fn test_auth_oauth_default_scopes() {
    let (code, stdout, _stderr) = run_cli(&["auth", "oauth", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("read,write,admin"),
        "default scopes should be read,write,admin"
    );
}

#[test]
fn test_auth_oauth_default_port() {
    let (code, stdout, _stderr) = run_cli(&["auth", "oauth", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("8484"), "default port should be 8484");
}

#[test]
fn test_auth_revoke_help() {
    let (code, stdout, _stderr) = run_cli(&["auth", "revoke", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--force"));
}

#[test]
fn test_auth_status_help() {
    let (code, stdout, _stderr) = run_cli(&["auth", "status", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--validate"));
}

#[test]
fn test_auth_help_examples_include_oauth() {
    let (code, stdout, _stderr) = run_cli(&["auth", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("linear auth oauth"),
        "help examples should show oauth usage"
    );
    assert!(
        stdout.contains("linear auth revoke"),
        "help examples should show revoke usage"
    );
}

// --- v0.3.7 Views + Webhooks tests ---

#[test]
fn test_views_help() {
    let (code, stdout, _stderr) = run_cli(&["views", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("get"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("update"));
    assert!(stdout.contains("delete"));
}

#[test]
fn test_views_alias() {
    let (code1, stdout1, _) = run_cli(&["v", "--help"]);
    let (code2, stdout2, _) = run_cli(&["views", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_views_create_help() {
    let (code, stdout, _stderr) = run_cli(&["views", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--description"));
    assert!(stdout.contains("--team"));
    assert!(stdout.contains("--shared"));
    assert!(stdout.contains("--filter-json"));
    assert!(stdout.contains("--icon"));
    assert!(stdout.contains("--color"));
}

#[test]
fn test_views_update_help() {
    let (code, stdout, _stderr) = run_cli(&["views", "update", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--name"));
    assert!(stdout.contains("--description"));
    assert!(stdout.contains("--shared"));
    assert!(stdout.contains("--filter-json"));
}

#[test]
fn test_views_delete_help() {
    let (code, stdout, _stderr) = run_cli(&["views", "delete", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--force"));
}

#[test]
fn test_webhooks_help() {
    let (code, stdout, _stderr) = run_cli(&["webhooks", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("get"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("update"));
    assert!(stdout.contains("delete"));
    assert!(stdout.contains("rotate-secret"));
    assert!(stdout.contains("listen"));
}

#[test]
fn test_webhooks_alias() {
    let (code1, stdout1, _) = run_cli(&["wh", "--help"]);
    let (code2, stdout2, _) = run_cli(&["webhooks", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_webhooks_create_help() {
    let (code, stdout, _stderr) = run_cli(&["webhooks", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--events"));
    assert!(stdout.contains("--team"));
    assert!(stdout.contains("--all-teams"));
    assert!(stdout.contains("--label"));
    assert!(stdout.contains("--secret"));
}

#[test]
fn test_webhooks_update_help() {
    let (code, stdout, _stderr) = run_cli(&["webhooks", "update", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--url"));
    assert!(stdout.contains("--events"));
    assert!(stdout.contains("--enabled"));
    assert!(stdout.contains("--disabled"));
    assert!(stdout.contains("--label"));
}

#[test]
fn test_webhooks_delete_help() {
    let (code, stdout, _stderr) = run_cli(&["webhooks", "delete", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--force"));
}

#[test]
fn test_webhooks_listen_help() {
    let (code, stdout, _stderr) = run_cli(&["webhooks", "listen", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--port"));
    assert!(stdout.contains("--events"));
    assert!(stdout.contains("--team"));
    assert!(stdout.contains("--secret"));
    assert!(stdout.contains("--url"));
    assert!(stdout.contains("--json"));
    assert!(stdout.contains("ngrok") || stdout.contains("tunnel"));
}

#[test]
fn test_webhooks_rotate_secret_help() {
    let (code, stdout, _stderr) = run_cli(&["webhooks", "rotate-secret", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("<ID>") || stdout.contains("id") || stdout.contains("ID"));
}

#[test]
fn test_issues_list_view_flag() {
    let (code, stdout, _stderr) = run_cli(&["issues", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--view"),
        "issues list should have --view flag"
    );
}

#[test]
fn test_projects_list_view_flag() {
    let (code, stdout, _stderr) = run_cli(&["projects", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--view"),
        "projects list should have --view flag"
    );
}

#[test]
fn test_auth_oauth_default_scopes_include_admin() {
    let (code, stdout, _stderr) = run_cli(&["auth", "oauth", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("read,write,admin"),
        "default scopes should now include admin"
    );
}

// === Whoami command tests ===

#[test]
fn test_whoami_help() {
    let (code, stdout, _stderr) = run_cli(&["whoami", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("authenticated user") || stdout.contains("users me"),
        "whoami should describe showing current user"
    );
}

#[test]
fn test_whoami_alias_me() {
    // "me" should be an alias for whoami
    let (code, stdout, _stderr) = run_cli(&["me", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("authenticated user") || stdout.contains("users me"),
        "me alias should work for whoami"
    );
}

#[test]
fn test_help_shows_whoami() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("whoami"),
        "top-level help should list whoami command"
    );
}

#[test]
fn test_users_help() {
    let (code, stdout, _stderr) = run_cli(&["users", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"), "users should have list");
    assert!(stdout.contains("me"), "users should have me");
    assert!(stdout.contains("get"), "users should have get");
}

#[test]
fn test_users_get_help() {
    let (code, stdout, _stderr) = run_cli(&["users", "get", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Get user details"),
        "users get should show help"
    );
}

// === Raw GraphQL API command tests ===

#[test]
fn test_api_help() {
    let (code, stdout, _stderr) = run_cli(&["api", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("query"), "api help should mention query");
    assert!(stdout.contains("mutate"), "api help should mention mutate");
}

#[test]
fn test_api_query_help() {
    let (code, stdout, _stderr) = run_cli(&["api", "query", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--variable"),
        "api query should have --variable flag"
    );
    assert!(
        stdout.contains("--paginate"),
        "api query should have --paginate flag"
    );
}

#[test]
fn test_api_mutate_help() {
    let (code, stdout, _stderr) = run_cli(&["api", "mutate", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--variable"),
        "api mutate should have --variable flag"
    );
}

#[test]
fn test_help_shows_api() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("api") || stdout.contains("Api"),
        "top-level help should list api command"
    );
}

// === --since / --newer-than time filter tests ===

#[test]
fn test_issues_list_since_flag() {
    let (code, stdout, _stderr) = run_cli(&["issues", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--since"),
        "issues list should have --since flag"
    );
}

#[test]
fn test_issues_list_newer_than_alias() {
    let (code, stdout, _stderr) = run_cli(&["issues", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("newer-than") || stdout.contains("--since"),
        "issues list should support --newer-than alias"
    );
}

#[test]
fn test_issues_get_history_flag() {
    let (code, stdout, _stderr) = run_cli(&["issues", "get", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--history"),
        "issues get should have --history flag"
    );
}

#[test]
fn test_issues_get_comments_flag() {
    let (code, stdout, _stderr) = run_cli(&["issues", "get", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--comments"),
        "issues get should have --comments flag"
    );
}

#[test]
fn test_issues_open_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "open", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Open issue in browser"),
        "issues open should show help"
    );
}

#[test]
fn test_issues_list_group_by_flag() {
    let (code, stdout, _stderr) = run_cli(&["issues", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--group-by"),
        "issues list should have --group-by flag"
    );
}

#[test]
fn test_issues_list_count_only_flag() {
    let (code, stdout, _stderr) = run_cli(&["issues", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--count-only"),
        "issues list should have --count-only flag"
    );
}

#[test]
fn test_projects_open_help() {
    let (code, stdout, _stderr) = run_cli(&["projects", "open", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Open project in browser"),
        "projects open should show help"
    );
}

#[test]
fn test_issues_close_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "close", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Close an issue"),
        "issues close should show help"
    );
}

#[test]
fn test_issues_close_alias_done() {
    let (code, stdout, _stderr) = run_cli(&["issues", "done", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Close an issue"),
        "issues done alias should work"
    );
}

#[test]
fn test_issues_archive_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "archive", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Archive an issue"),
        "issues archive should show help"
    );
}

#[test]
fn test_issues_unarchive_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "unarchive", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Unarchive an issue"),
        "issues unarchive should show help"
    );
}

#[test]
fn test_issues_comment_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "comment", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Add a comment"),
        "issues comment should show help"
    );
    assert!(
        stdout.contains("--body"),
        "issues comment should accept --body flag"
    );
}

#[test]
fn test_issues_link_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "link", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Print the issue URL"),
        "issues link should show help"
    );
}

#[test]
fn test_issues_assign_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "assign", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Assign an issue"),
        "issues assign should show help"
    );
}

#[test]
fn test_issues_move_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "move", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Move an issue"),
        "issues move should show help"
    );
}

#[test]
fn test_issues_move_alias_mv() {
    let (code, stdout, _stderr) = run_cli(&["issues", "mv", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Move an issue"),
        "mv alias should work for move"
    );
}

#[test]
fn test_issues_transfer_help() {
    let (code, stdout, _stderr) = run_cli(&["issues", "transfer", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Transfer an issue"),
        "issues transfer should show help"
    );
}

// === Milestone CRUD tests ===

#[test]
fn test_milestones_help() {
    let (code, stdout, _stderr) = run_cli(&["milestones", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"), "milestones should have list");
    assert!(stdout.contains("get"), "milestones should have get");
    assert!(stdout.contains("create"), "milestones should have create");
    assert!(stdout.contains("update"), "milestones should have update");
    assert!(stdout.contains("delete"), "milestones should have delete");
}

#[test]
fn test_milestones_alias_ms() {
    let (code, stdout, _stderr) = run_cli(&["ms", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("list"), "ms alias should work");
}

#[test]
fn test_milestones_create_help() {
    let (code, stdout, _stderr) = run_cli(&["milestones", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--project"),
        "milestone create should require --project"
    );
    assert!(
        stdout.contains("--target-date"),
        "milestone create should have --target-date"
    );
}

#[test]
fn test_milestones_update_help() {
    let (code, stdout, _stderr) = run_cli(&["milestones", "update", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--name"),
        "milestone update should have --name"
    );
    assert!(
        stdout.contains("--target-date"),
        "milestone update should have --target-date"
    );
}

#[test]
fn test_milestones_delete_help() {
    let (code, stdout, _stderr) = run_cli(&["milestones", "delete", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--force"),
        "milestone delete should have --force"
    );
}

#[test]
fn test_help_shows_milestones() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("milestones") || stdout.contains("Milestones"),
        "top-level help should list milestones command"
    );
}

// === Pager support tests ===

#[test]
fn test_no_pager_flag() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--no-pager"),
        "global help should show --no-pager flag"
    );
}

#[test]
fn test_no_pager_env_var() {
    // Verify LINEAR_CLI_NO_PAGER env var is documented in help
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("LINEAR_CLI_NO_PAGER") || stdout.contains("no-pager"),
        "no-pager should be available as flag or env var"
    );
}

// === Behavioral tests ===

#[test]
fn test_count_only_flag_exists() {
    let (code, stdout, _stderr) = run_cli(&["issues", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--count-only"),
        "issues list should have --count-only flag"
    );
    // Verify it's described as returning a count
    assert!(
        stdout.contains("count") || stdout.contains("Count"),
        "--count-only should mention count in description"
    );
}

#[test]
fn test_dry_run_output() {
    // dry-run on create should not actually create, just preview
    let (code, stdout, stderr) = run_cli(&[
        "issues",
        "create",
        "Test dry run",
        "-t",
        "FAKE",
        "--dry-run",
    ]);
    // Should fail with auth error (no valid API key) but the flag should be accepted
    // If the CLI parses --dry-run without error before API call, that's correct behavior
    let combined = format!("{stdout}\n{stderr}");
    let parser_diagnostics = [
        "unexpected argument",
        "required arguments were not provided",
        "invalid value",
        "Usage:",
        "For more information, try '--help'",
    ];
    assert!(
        !parser_diagnostics
            .iter()
            .any(|diagnostic| combined.contains(diagnostic)),
        "dry-run should not fail at argument parsing; stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        code != 0 || combined.contains("dry_run") || combined.contains("DRY RUN"),
        "dry-run should either output preview or fail after parsing; stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn test_json_output_format() {
    // --output json should be accepted without error on help
    let (code, stdout, _stderr) = run_cli(&["--output", "json", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Commands:") || stdout.contains("linear-cli"),
        "help should still work with --output json"
    );
}

#[test]
fn test_filter_flag_exists() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--filter"),
        "global help should show --filter flag"
    );
}

#[test]
fn test_issues_list_project_filter() {
    let (code, stdout, _stderr) = run_cli(&["issues", "list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--project"),
        "issues list should support --project filter"
    );
}

// === v0.3.11-v0.3.13 Sprint commands ===

#[test]
fn test_sprint_help() {
    let (code, stdout, _stderr) = run_cli(&["sprint", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("status"),
        "sprint help should list status subcommand"
    );
    assert!(
        stdout.contains("progress"),
        "sprint help should list progress subcommand"
    );
    assert!(
        stdout.contains("plan"),
        "sprint help should list plan subcommand"
    );
    assert!(
        stdout.contains("carry-over"),
        "sprint help should list carry-over subcommand"
    );
}

#[test]
fn test_sprint_alias() {
    let (code1, stdout1, _) = run_cli(&["sp", "--help"]);
    let (code2, stdout2, _) = run_cli(&["sprint", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_sprint_status_requires_team() {
    let (code, _stdout, stderr) = run_cli(&["sprint", "status"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("--team") || stderr.contains("required"),
        "sprint status should require --team flag"
    );
}

#[test]
fn test_sprint_progress_requires_team() {
    let (code, _stdout, stderr) = run_cli(&["sprint", "progress"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("--team") || stderr.contains("required"),
        "sprint progress should require --team flag"
    );
}

#[test]
fn test_sprint_plan_requires_team() {
    let (code, _stdout, stderr) = run_cli(&["sprint", "plan"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("--team") || stderr.contains("required"),
        "sprint plan should require --team flag"
    );
}

#[test]
fn test_sprint_carry_over_requires_team() {
    let (code, _stdout, stderr) = run_cli(&["sprint", "carry-over"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("--team") || stderr.contains("required"),
        "sprint carry-over should require --team flag"
    );
}

// === Attachments commands ===

#[test]
fn test_attachments_help() {
    let (code, stdout, _stderr) = run_cli(&["attachments", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("list"),
        "attachments help should list list subcommand"
    );
    assert!(
        stdout.contains("get"),
        "attachments help should list get subcommand"
    );
    assert!(
        stdout.contains("create"),
        "attachments help should list create subcommand"
    );
    assert!(
        stdout.contains("update"),
        "attachments help should list update subcommand"
    );
    assert!(
        stdout.contains("delete"),
        "attachments help should list delete subcommand"
    );
    assert!(
        stdout.contains("link-url"),
        "attachments help should list link-url subcommand"
    );
}

#[test]
fn test_attachments_alias() {
    let (code1, stdout1, _) = run_cli(&["att", "--help"]);
    let (code2, stdout2, _) = run_cli(&["attachments", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_attachments_list_requires_issue() {
    let (code, _stdout, stderr) = run_cli(&["attachments", "list"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("ISSUE") || stderr.contains("required") || stderr.contains("issue"),
        "attachments list should require issue ID"
    );
}

#[test]
fn test_attachments_create_requires_args() {
    let (code, _stdout, stderr) = run_cli(&["attachments", "create"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("required") || stderr.contains("ISSUE"),
        "attachments create should require issue, title, and URL"
    );
}

#[test]
fn test_attachments_create_help() {
    let (code, stdout, _stderr) = run_cli(&["attachments", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--title"),
        "attachments create should accept --title"
    );
    assert!(
        stdout.contains("--url"),
        "attachments create should accept --url"
    );
}

#[test]
fn test_attachments_delete_help() {
    let (code, stdout, _stderr) = run_cli(&["attachments", "delete", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--force"),
        "attachments delete should accept --force"
    );
}

// === Project Updates commands ===

#[test]
fn test_project_updates_help() {
    let (code, stdout, _stderr) = run_cli(&["project-updates", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("list"),
        "project-updates help should list list subcommand"
    );
    assert!(
        stdout.contains("get"),
        "project-updates help should list get subcommand"
    );
    assert!(
        stdout.contains("create"),
        "project-updates help should list create subcommand"
    );
    assert!(
        stdout.contains("update"),
        "project-updates help should list update subcommand"
    );
    assert!(
        stdout.contains("archive"),
        "project-updates help should list archive subcommand"
    );
    assert!(
        stdout.contains("unarchive"),
        "project-updates help should list unarchive subcommand"
    );
}

#[test]
fn test_project_updates_alias() {
    let (code1, stdout1, _) = run_cli(&["pu", "--help"]);
    let (code2, stdout2, _) = run_cli(&["project-updates", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_project_updates_list_requires_project() {
    let (code, _stdout, stderr) = run_cli(&["project-updates", "list"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("PROJECT") || stderr.contains("required") || stderr.contains("project"),
        "project-updates list should require project argument"
    );
}

#[test]
fn test_project_updates_create_help() {
    let (code, stdout, _stderr) = run_cli(&["project-updates", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--body"),
        "project-updates create should accept --body"
    );
    assert!(
        stdout.contains("--health"),
        "project-updates create should accept --health"
    );
}

// === Templates Remote commands ===

#[test]
fn test_templates_remote_list_help() {
    let (code, stdout, _stderr) = run_cli(&["templates", "remote-list", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--type") || stdout.contains("template"),
        "templates remote-list should show type filter option"
    );
}

#[test]
fn test_templates_create_help_supports_non_interactive_flags() {
    let (code, stdout, _stderr) = run_cli(&["templates", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--team"));
    assert!(stdout.contains("--priority"));
    assert!(stdout.contains("--label"));
    assert!(stdout.contains("--title-prefix"));
}

#[test]
fn test_templates_create_dry_run_is_non_interactive_json() {
    let (code, stdout, stderr) = run_cli(&[
        "--dry-run",
        "--output",
        "json",
        "templates",
        "create",
        "agent-bug",
        "--team",
        "ENG",
        "--priority",
        "2",
        "--label",
        "bug",
        "--label",
        "agent",
        "--title-prefix",
        "[Bug]",
        "--description",
        "Checklist",
    ]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(stdout.contains("\"dry_run\""));
    assert!(stdout.contains("\"would_create\""));
    assert!(stdout.contains("\"agent-bug\""));
    assert!(stdout.contains("\"default_priority\": 2"));
}

#[test]
fn test_templates_remote_create_requires_args() {
    let (code, _stdout, stderr) = run_cli(&["templates", "remote-create"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("--name") || stderr.contains("--type") || stderr.contains("required"),
        "templates remote-create should require --name and --type"
    );
}

#[test]
fn test_templates_remote_create_help() {
    let (code, stdout, _stderr) = run_cli(&["templates", "remote-create", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--name"),
        "remote-create should accept --name"
    );
    assert!(
        stdout.contains("--type"),
        "remote-create should accept --type"
    );
    assert!(
        stdout.contains("--team"),
        "remote-create should accept --team"
    );
}

#[test]
fn test_templates_help_includes_remote() {
    let (code, stdout, _stderr) = run_cli(&["templates", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("remote-list"),
        "templates help should list remote-list subcommand"
    );
    assert!(
        stdout.contains("remote-create"),
        "templates help should list remote-create subcommand"
    );
    assert!(
        stdout.contains("remote-get"),
        "templates help should list remote-get subcommand"
    );
}

// === Import commands ===

#[test]
fn test_import_help() {
    let (code, stdout, _stderr) = run_cli(&["import", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("csv"),
        "import help should list csv subcommand"
    );
    assert!(
        stdout.contains("json"),
        "import help should list json subcommand"
    );
}

#[test]
fn test_import_alias() {
    let (code1, stdout1, _) = run_cli(&["im", "--help"]);
    let (code2, stdout2, _) = run_cli(&["import", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_import_csv_requires_file() {
    let (code, _stdout, stderr) = run_cli(&["import", "csv"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("FILE") || stderr.contains("required") || stderr.contains("file"),
        "import csv should require file path"
    );
}

#[test]
fn test_import_json_requires_file() {
    let (code, _stdout, stderr) = run_cli(&["import", "json"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("FILE") || stderr.contains("required") || stderr.contains("file"),
        "import json should require file path"
    );
}

#[test]
fn test_import_csv_help() {
    let (code, stdout, _stderr) = run_cli(&["import", "csv", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--team"), "import csv should accept --team");
    assert!(
        stdout.contains("--dry-run"),
        "import csv should accept --dry-run"
    );
}

#[test]
fn test_import_json_help() {
    let (code, stdout, _stderr) = run_cli(&["import", "json", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--team"),
        "import json should accept --team"
    );
    assert!(
        stdout.contains("--dry-run"),
        "import json should accept --dry-run"
    );
}

// === Export enhancements ===

#[test]
fn test_export_json_subcommand() {
    let (code, stdout, _stderr) = run_cli(&["export", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("json"),
        "export help should list json subcommand"
    );
}

#[test]
fn test_export_projects_csv_subcommand() {
    let (code, stdout, _stderr) = run_cli(&["export", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("projects-csv"),
        "export help should list projects-csv subcommand"
    );
}

#[test]
fn test_export_json_help() {
    let (code, stdout, _stderr) = run_cli(&["export", "json", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--team"),
        "export json should accept --team"
    );
    assert!(
        stdout.contains("--file"),
        "export json should accept --file"
    );
    assert!(
        stdout.contains("--pretty"),
        "export json should accept --pretty"
    );
}

#[test]
fn test_export_projects_csv_help() {
    let (code, stdout, _stderr) = run_cli(&["export", "projects-csv", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--file"),
        "export projects-csv should accept --file"
    );
    assert!(
        stdout.contains("--archived"),
        "export projects-csv should accept --archived"
    );
}

#[test]
fn test_export_alias() {
    let (code1, stdout1, _) = run_cli(&["exp", "--help"]);
    let (code2, stdout2, _) = run_cli(&["export", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

// === Completions commands ===

#[test]
fn test_completions_top_level_help() {
    let (code, stdout, _stderr) = run_cli(&["completions", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("static") || stdout.contains("Static"),
        "completions help should list static subcommand"
    );
    assert!(
        stdout.contains("dynamic") || stdout.contains("Dynamic"),
        "completions help should list dynamic subcommand"
    );
}

#[test]
fn test_completions_alias() {
    let (code1, stdout1, _) = run_cli(&["comp", "--help"]);
    let (code2, stdout2, _) = run_cli(&["completions", "--help"]);
    assert_eq!(code1, 0);
    assert_eq!(code2, 0);
    assert_eq!(stdout1, stdout2);
}

#[test]
fn test_completions_dynamic_help() {
    let (code, stdout, _stderr) = run_cli(&["completions", "dynamic", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("shell") || stdout.contains("Shell") || stdout.contains("SHELL"),
        "dynamic subcommand should accept shell argument"
    );
}

#[test]
fn test_completions_static_help() {
    let (code, stdout, _stderr) = run_cli(&["completions", "static", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("bash") || stdout.contains("Bash"),
        "static completions should accept bash shell"
    );
    assert!(
        stdout.contains("zsh") || stdout.contains("Zsh"),
        "static completions should accept zsh shell"
    );
    assert!(
        stdout.contains("fish") || stdout.contains("Fish"),
        "static completions should accept fish shell"
    );
    assert!(
        stdout.contains("powershell") || stdout.contains("PowerShell"),
        "static completions should accept powershell shell"
    );
}

#[test]
fn test_completions_dynamic_accepts_shells() {
    let (code, stdout, _stderr) = run_cli(&["completions", "dynamic", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("bash") || stdout.contains("Bash"),
        "dynamic completions should accept bash shell"
    );
    assert!(
        stdout.contains("zsh") || stdout.contains("Zsh"),
        "dynamic completions should accept zsh shell"
    );
    assert!(
        stdout.contains("fish") || stdout.contains("Fish"),
        "dynamic completions should accept fish shell"
    );
    assert!(
        stdout.contains("powershell") || stdout.contains("PowerShell"),
        "dynamic completions should accept powershell shell"
    );
}

// === Hidden _complete command ===

#[test]
fn test_complete_hidden_not_in_help() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        !stdout.contains("_complete"),
        "_complete should NOT appear in main help (it is hidden)"
    );
}

#[test]
fn test_complete_requires_type() {
    let (code, _stdout, stderr) = run_cli(&["_complete"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("--type") || stderr.contains("required"),
        "_complete should require --type flag"
    );
}

#[test]
fn test_complete_unknown_type() {
    let (code, stdout, stderr) = run_cli(&["_complete", "--type", "nonexistent_type_xyz"]);
    assert_eq!(code, 0, "unknown completion type should exit gracefully");
    assert!(
        stdout.trim().is_empty(),
        "unknown completion type should return no completions"
    );
    assert!(
        stderr.trim().is_empty(),
        "unknown completion type should not emit parser or panic output"
    );
}

// === Teams CRUD ===

#[test]
fn test_teams_create_requires_name() {
    let (code, _stdout, stderr) = run_cli(&["teams", "create"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("NAME") || stderr.contains("required") || stderr.contains("name"),
        "teams create should require name argument"
    );
}

#[test]
fn test_teams_create_help() {
    let (code, stdout, _stderr) = run_cli(&["teams", "create", "--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--key"), "teams create should accept --key");
    assert!(
        stdout.contains("--description"),
        "teams create should accept --description"
    );
    assert!(
        stdout.contains("--color"),
        "teams create should accept --color"
    );
}

#[test]
fn test_teams_update_requires_id() {
    let (code, _stdout, stderr) = run_cli(&["teams", "update"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("ID") || stderr.contains("required") || stderr.contains("id"),
        "teams update should require id argument"
    );
}

#[test]
fn test_teams_update_help() {
    let (code, stdout, _stderr) = run_cli(&["teams", "update", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--name"),
        "teams update should accept --name"
    );
    assert!(
        stdout.contains("--description"),
        "teams update should accept --description"
    );
}

#[test]
fn test_teams_delete_requires_id() {
    let (code, _stdout, stderr) = run_cli(&["teams", "delete"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("ID") || stderr.contains("required") || stderr.contains("id"),
        "teams delete should require id argument"
    );
}

#[test]
fn test_teams_delete_help() {
    let (code, stdout, _stderr) = run_cli(&["teams", "delete", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--force"),
        "teams delete should accept --force"
    );
}

#[test]
fn test_teams_help_includes_crud() {
    let (code, stdout, _stderr) = run_cli(&["teams", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("create"),
        "teams help should list create subcommand"
    );
    assert!(
        stdout.contains("update"),
        "teams help should list update subcommand"
    );
    assert!(
        stdout.contains("delete"),
        "teams help should list delete subcommand"
    );
}

// === Projects archive/unarchive ===

#[test]
fn test_projects_archive_requires_id() {
    let (code, _stdout, stderr) = run_cli(&["projects", "archive"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("ID") || stderr.contains("required") || stderr.contains("id"),
        "projects archive should require id argument"
    );
}

#[test]
fn test_projects_unarchive_requires_id() {
    let (code, _stdout, stderr) = run_cli(&["projects", "unarchive"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("ID") || stderr.contains("required") || stderr.contains("id"),
        "projects unarchive should require id argument"
    );
}

#[test]
fn test_projects_help_includes_archive() {
    let (code, stdout, _stderr) = run_cli(&["projects", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("archive"),
        "projects help should list archive subcommand"
    );
    assert!(
        stdout.contains("unarchive"),
        "projects help should list unarchive subcommand"
    );
}

#[test]
fn test_projects_help_includes_label_mgmt() {
    let (code, stdout, _stderr) = run_cli(&["projects", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("add-labels"),
        "projects help should list add-labels subcommand"
    );
    assert!(
        stdout.contains("remove-labels"),
        "projects help should list remove-labels subcommand"
    );
    assert!(
        stdout.contains("set-labels"),
        "projects help should list set-labels subcommand"
    );
}

// === Done command ===

#[test]
fn test_done_help() {
    let (code, stdout, _stderr) = run_cli(&["done", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--status") || stdout.contains("Done"),
        "done help should mention status flag or Done default"
    );
}

#[test]
fn test_done_in_top_level_help() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("done"),
        "top-level help should list done command"
    );
}

// === Setup command ===

#[test]
fn test_setup_help() {
    let (code, stdout, _stderr) = run_cli(&["setup", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("setup") || stdout.contains("wizard") || stdout.contains("onboarding"),
        "setup help should describe the setup wizard"
    );
}

#[test]
fn test_setup_in_top_level_help() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("setup"),
        "top-level help should list setup command"
    );
}

// === Doctor --fix ===

#[test]
fn test_doctor_fix_flag() {
    let (code, stdout, _stderr) = run_cli(&["doctor", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--fix"),
        "doctor help should show --fix flag"
    );
}

#[test]
fn test_doctor_check_api_flag() {
    let (code, stdout, _stderr) = run_cli(&["doctor", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--check-api"),
        "doctor help should show --check-api flag"
    );
}

// === Global --yes flag ===

#[test]
fn test_yes_flag_accepted() {
    let (code, stdout, _stderr) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--yes"),
        "global help should show --yes flag"
    );
}

#[test]
fn test_yes_flag_works_with_subcommand() {
    // --yes is global and should be accepted alongside any subcommand's help
    let (code, stdout, _stderr) = run_cli(&["--yes", "issues", "--help"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("list"),
        "--yes flag should not interfere with subcommand parsing"
    );
}

// ---- Exit-code / JSON-error contract (agent ergonomics) ----
// Offline checks via the `bulk label` empty-issues path (a failure reached
// before any network/config access). Credential-dependent paths are covered by
// the unit tests in src/commands/* and by live E2E.

/// Run the CLI with an isolated, empty config home and no ambient credentials.
/// Each call gets a unique HOME so parallel tests never share state.
fn run_cli_isolated(args: &[&str]) -> (i32, String, String) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp =
        std::env::temp_dir().join(format!("linear-cli-isolated-{}-{}", std::process::id(), n));
    let _ = std::fs::create_dir_all(&tmp);
    let output = Command::new(env!("CARGO_BIN_EXE_linear-cli"))
        .args(args)
        .env("HOME", &tmp)
        .env("XDG_CONFIG_HOME", &tmp)
        .env_remove("LINEAR_API_KEY")
        .env_remove("LINEAR_CLI_PROFILE")
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);

    (code, stdout, stderr)
}

#[test]
fn test_failing_command_exits_nonzero() {
    // `bulk label` with no issues is a logical failure: must not exit 0.
    let (code, _stdout, stderr) = run_cli_isolated(&["bulk", "label", "Bug"]);
    assert_ne!(
        code, 0,
        "a failing command must exit non-zero; stderr={stderr}"
    );
    assert!(
        (1..=4).contains(&code),
        "exit code should be a documented error code (1-4), got {code}"
    );
}

#[test]
fn test_json_error_body_emitted_on_stderr_not_stdout() {
    // stdout is empty on this path; the results-vs-error stream split is
    // unit-tested in src/commands/{bulk,issues,comments}.rs.
    let (code, stdout, stderr) = run_cli_isolated(&["bulk", "label", "Bug", "--output", "json"]);
    assert_ne!(code, 0, "failing command must exit non-zero");
    assert!(
        stderr.contains("\"error\":true"),
        "stderr should carry the JSON error body, got: {stderr}"
    );
    assert!(
        !stdout.contains("\"error\":true"),
        "stdout must not carry a second JSON error body, got: {stdout}"
    );
}

#[test]
fn test_successful_command_exits_zero() {
    let (code, stdout, _stderr) = run_cli_isolated(&["--version"]);
    assert_eq!(code, 0, "a successful command must exit 0");
    assert!(!stdout.is_empty());
}
