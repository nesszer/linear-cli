use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Map, Value};
use std::io::{self, BufRead, IsTerminal};

use crate::api::LinearClient;
use crate::output::{print_json_owned, reject_unsupported_dry_run, OutputOptions};
use crate::pagination::{paginate_nodes, PaginationOptions};

#[derive(Subcommand)]
pub enum ApiCommands {
    /// Execute a raw GraphQL query
    #[command(after_help = r#"EXAMPLES:
    linear api query '{ viewer { id name email } }'
    linear api query '{ teams { nodes { id key name } } }'
    linear api query -v teamId=abc123 'query($teamId: String!) { team(id: $teamId) { name } }'
    echo '{ viewer { id } }' | linear api query
    echo '{ viewer { id } }' | linear api query -"#)]
    Query {
        /// GraphQL query string. Use "-" to read from stdin, or omit when piping.
        query: Option<String>,

        /// Variables as key=value pairs (e.g. -v id=abc123 -v name=test)
        #[arg(short = 'v', long = "variable", value_name = "KEY=VALUE")]
        variables: Vec<String>,

        /// Auto-paginate through all results (requires nodes + pageInfo in query)
        #[arg(long)]
        paginate: bool,

        /// JSON path to nodes array (dot-separated, e.g. data.issues.nodes)
        #[arg(long, default_value = "")]
        nodes_path: String,

        /// JSON path to pageInfo object (dot-separated, e.g. data.issues.pageInfo)
        #[arg(long, default_value = "")]
        page_info_path: String,
    },
    /// Execute a raw GraphQL mutation
    #[command(after_help = r#"EXAMPLES:
    linear api mutate -v title="New Issue" -v teamId=abc123 \
        'mutation($title: String!, $teamId: String!) { issueCreate(input: { title: $title, teamId: $teamId }) { issue { id identifier } } }'
    cat mutation.graphql | linear api mutate -
    echo '...' | linear api mutate"#)]
    Mutate {
        /// GraphQL mutation string. Use "-" to read from stdin, or omit when piping.
        query: Option<String>,

        /// Variables as key=value pairs (e.g. -v id=abc123 -v name=test)
        #[arg(short = 'v', long = "variable", value_name = "KEY=VALUE")]
        variables: Vec<String>,
    },
}

pub async fn handle(cmd: ApiCommands, output: &OutputOptions) -> Result<()> {
    match cmd {
        ApiCommands::Query {
            query,
            variables,
            paginate,
            nodes_path,
            page_info_path,
        } => {
            reject_unsupported_dry_run(output.dry_run, "api query")?;
            let resolved = resolve_query_source(query)?;
            run_query(
                &resolved,
                &variables,
                paginate,
                &nodes_path,
                &page_info_path,
                output,
            )
            .await
        }
        ApiCommands::Mutate { query, variables } => {
            reject_unsupported_dry_run(output.dry_run, "api mutate")?;
            let resolved = resolve_query_source(query)?;
            run_mutate(&resolved, &variables, output).await
        }
    }
}

/// Resolve the GraphQL query source: explicit string, "-" for stdin, or auto-detect piped stdin.
fn resolve_query_source(query: Option<String>) -> Result<String> {
    match query {
        Some(q) if q == "-" => read_stdin(),
        Some(q) => Ok(q),
        None => {
            if !io::stdin().is_terminal() {
                read_stdin()
            } else {
                anyhow::bail!(
                    "No query provided. Pass a GraphQL query string as an argument, \
                     or pipe one via stdin:\n  \
                     echo '{{ viewer {{ id }} }}' | linear-cli api query"
                )
            }
        }
    }
}

fn read_stdin() -> Result<String> {
    let stdin = io::stdin();
    let lines: Vec<String> = stdin.lock().lines().map_while(Result::ok).collect();
    let query = lines.join("\n");
    if query.trim().is_empty() {
        anyhow::bail!("Empty query received from stdin");
    }
    Ok(query)
}

fn read_query(input: &str) -> Result<String> {
    if input == "-" {
        read_stdin()
    } else {
        Ok(input.to_string())
    }
}

fn parse_variables(vars: &[String]) -> Result<Option<Value>> {
    if vars.is_empty() {
        return Ok(None);
    }

    let mut map = Map::new();
    for var in vars {
        let (key, value) = var
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("Invalid variable format '{}'. Use key=value.", var))?;

        // Try to parse as JSON value (number, bool, null, object, array)
        // Fall back to string if parsing fails
        let json_value = serde_json::from_str(value).unwrap_or_else(|_| json!(value));
        map.insert(key.to_string(), json_value);
    }

    Ok(Some(Value::Object(map)))
}

async fn run_query(
    query_str: &str,
    variables: &[String],
    paginate: bool,
    nodes_path: &str,
    page_info_path: &str,
    output: &OutputOptions,
) -> Result<()> {
    let query = read_query(query_str)?;
    let vars = parse_variables(variables)?;
    let client = LinearClient::new()?;

    if paginate {
        // Parse paths
        let nodes: Vec<&str> = if nodes_path.is_empty() {
            // Try to auto-detect from query
            anyhow::bail!(
                "--paginate requires --nodes-path and --page-info-path.\n\
                 Example: --nodes-path data.issues.nodes --page-info-path data.issues.pageInfo"
            );
        } else {
            nodes_path.split('.').collect()
        };

        let page_info: Vec<&str> = if page_info_path.is_empty() {
            anyhow::bail!(
                "--paginate requires --page-info-path.\n\
                 Example: --page-info-path data.issues.pageInfo"
            );
        } else {
            page_info_path.split('.').collect()
        };

        let base_vars = if let Some(Value::Object(m)) = vars {
            m
        } else {
            Map::new()
        };

        let pagination = PaginationOptions {
            all: true,
            page_size: Some(50),
            ..Default::default()
        };

        let nodes_refs: Vec<&str> = nodes.to_vec();
        let page_info_refs: Vec<&str> = page_info.to_vec();

        let results = paginate_nodes(
            &client,
            &query,
            base_vars,
            &nodes_refs,
            &page_info_refs,
            &pagination,
            50,
        )
        .await?;

        print_json_owned(json!(results), output)?;
    } else {
        let result = client.query(&query, vars).await?;
        print_json_owned(result, output)?;
    }

    Ok(())
}

async fn run_mutate(query_str: &str, variables: &[String], output: &OutputOptions) -> Result<()> {
    let query = read_query(query_str)?;
    let vars = parse_variables(variables)?;
    let client = LinearClient::new()?;

    let result = client.mutate(&query, vars).await?;
    print_json_owned(result, output)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dry_run_rejects_query_before_resolving_source() {
        let output = OutputOptions {
            format: crate::OutputFormat::Table,
            json: crate::output::JsonOutputOptions::new(
                false,
                None,
                None,
                crate::output::SortOrder::Asc,
                true,
            ),
            format_template: None,
            filters: vec![],
            fail_on_empty: false,
            pagination: crate::pagination::PaginationOptions::default(),
            cache: crate::cache::CacheOptions::default(),
            dry_run: true,
        };

        let error = handle(
            ApiCommands::Query {
                query: None,
                variables: vec![],
                paginate: false,
                nodes_path: String::new(),
                page_info_path: String::new(),
            },
            &output,
        )
        .await
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("--dry-run is not supported for `api query`"));
    }

    #[test]
    fn test_parse_variables_empty() {
        let result = parse_variables(&[]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_variables_string() {
        let vars = vec!["name=hello".to_string()];
        let result = parse_variables(&vars).unwrap().unwrap();
        assert_eq!(result["name"], json!("hello"));
    }

    #[test]
    fn test_parse_variables_number() {
        let vars = vec!["count=42".to_string()];
        let result = parse_variables(&vars).unwrap().unwrap();
        assert_eq!(result["count"], json!(42));
    }

    #[test]
    fn test_parse_variables_bool() {
        let vars = vec!["active=true".to_string()];
        let result = parse_variables(&vars).unwrap().unwrap();
        assert_eq!(result["active"], json!(true));
    }

    #[test]
    fn test_parse_variables_multiple() {
        let vars = vec![
            "name=test".to_string(),
            "count=5".to_string(),
            "active=false".to_string(),
        ];
        let result = parse_variables(&vars).unwrap().unwrap();
        assert_eq!(result["name"], json!("test"));
        assert_eq!(result["count"], json!(5));
        assert_eq!(result["active"], json!(false));
    }

    #[test]
    fn test_parse_variables_invalid() {
        let vars = vec!["invalid".to_string()];
        assert!(parse_variables(&vars).is_err());
    }

    #[test]
    fn test_parse_variables_json_object() {
        let vars = vec![r#"filter={"name":{"eq":"test"}}"#.to_string()];
        let result = parse_variables(&vars).unwrap().unwrap();
        assert_eq!(result["filter"]["name"]["eq"], json!("test"));
    }

    #[test]
    fn test_read_query_direct() {
        let q = read_query("{ viewer { id } }").unwrap();
        assert_eq!(q, "{ viewer { id } }");
    }

    #[test]
    fn test_resolve_query_source_explicit() {
        let q = resolve_query_source(Some("{ viewer { id } }".to_string())).unwrap();
        assert_eq!(q, "{ viewer { id } }");
    }

    #[test]
    fn test_resolve_query_source_none_tty() {
        // When stdin is a TTY and no query provided, should error
        // In test environment stdin is usually not a TTY, so this may read empty stdin
        // and return an error either way
        let result = resolve_query_source(None);
        assert!(result.is_err());
    }
}
