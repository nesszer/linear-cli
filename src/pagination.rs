use anyhow::Result;
use serde_json::{Map, Value};
use std::future::Future;

use crate::api::LinearClient;
use crate::json_path::get_path;

#[derive(Debug, Clone, Default)]
pub struct PaginationOptions {
    pub limit: Option<usize>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub page_size: Option<usize>,
    pub all: bool,
}

impl PaginationOptions {
    pub fn with_default_limit(&self, default_limit: usize) -> Self {
        let mut options = self.clone();
        if !options.all && options.limit.is_none() {
            options.limit = Some(default_limit);
        }
        options
    }

    /// The dual of [`with_default_limit`]: an unbounded request (no `--limit`,
    /// no `--all`) is upgraded to fetch every page. Used by detail views that
    /// should return a full nested connection rather than the API's default
    /// first page. An explicit `--limit` / `--all` is passed through untouched.
    pub fn with_default_all(&self) -> Self {
        let mut options = self.clone();
        if !options.all && options.limit.is_none() {
            options.all = true;
        }
        options
    }

    pub fn effective_page_size(&self, default_page_size: usize) -> usize {
        self.page_size.unwrap_or(default_page_size).max(1)
    }
}

pub async fn paginate_nodes(
    client: &LinearClient,
    query: &str,
    base_variables: Map<String, Value>,
    nodes_path: &[&str],
    page_info_path: &[&str],
    options: &PaginationOptions,
    default_page_size: usize,
) -> Result<Vec<Value>> {
    let mut items: Vec<Value> = Vec::new();
    let remaining = if options.all { None } else { options.limit };
    let mut after = options.after.clone();
    let mut before = options.before.clone();
    let mut forward = before.is_none();

    if options.after.is_some() && options.before.is_some() {
        before = None;
        forward = true;
    }

    let page_size = options.effective_page_size(default_page_size);

    loop {
        let batch_size = remaining
            .map(|r| r.saturating_sub(items.len()).min(page_size))
            .unwrap_or(page_size)
            .max(1);

        // Build per-page variables: start with pagination keys, then extend with base
        let mut page_vars = Map::with_capacity(base_variables.len() + 2);
        if forward {
            page_vars.insert(
                "first".to_string(),
                Value::Number(serde_json::Number::from(batch_size as u64)),
            );
            if let Some(ref cursor) = after {
                page_vars.insert("after".to_string(), Value::String(cursor.clone()));
            }
        } else {
            page_vars.insert(
                "last".to_string(),
                Value::Number(serde_json::Number::from(batch_size as u64)),
            );
            if let Some(ref cursor) = before {
                page_vars.insert("before".to_string(), Value::String(cursor.clone()));
            }
        }
        // Extend with base variables (these don't change between iterations)
        for (k, v) in &base_variables {
            page_vars.insert(k.clone(), v.clone());
        }

        let result = client.query(query, Some(Value::Object(page_vars))).await?;

        let nodes = get_path(&result, nodes_path)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        items.extend(nodes);

        if let Some(limit) = remaining {
            if limit <= items.len() {
                items.truncate(limit);
                break;
            }
        }

        if !options.all && options.limit.is_none() {
            break;
        }

        let page_info = get_path(&result, page_info_path).and_then(|v| v.as_object());
        let Some(page_info) = page_info else { break };

        if forward {
            let has_next = page_info
                .get("hasNextPage")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !has_next {
                break;
            }
            after = page_info
                .get("endCursor")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if after.is_none() {
                break;
            }
        } else {
            let has_prev = page_info
                .get("hasPreviousPage")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !has_prev {
                break;
            }
            before = page_info
                .get("startCursor")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if before.is_none() {
                break;
            }
        }
    }

    Ok(items)
}

/// Stream paginated results, calling a handler for each batch of nodes.
///
/// This is memory-efficient for large exports because it processes each page
/// as it arrives rather than accumulating all results in memory.
///
/// The handler receives each batch of nodes and can process them immediately
/// (e.g., write to CSV). Returns the total number of items processed.
///
/// # Arguments
///
/// * `client` - The Linear API client
/// * `query` - GraphQL query with pagination variables
/// * `variables` - Initial query variables
/// * `nodes_path` - JSON path to the nodes array in the response
/// * `page_info_path` - JSON path to the pageInfo object in the response
/// * `options` - Pagination options (limit, page_size, all, etc.)
/// * `default_page_size` - Default page size if not specified in options
/// * `handler` - Async function called with each batch of nodes
///
/// # Example
///
/// ```ignore
/// let count = stream_nodes(
///     &client,
///     query,
///     vars,
///     &["data", "issues", "nodes"],
///     &["data", "issues", "pageInfo"],
///     &pagination,
///     250,
///     |batch| async {
///         for issue in batch {
///             wtr.write_record(&[...])?;
///         }
///         Ok(())
///     },
/// ).await?;
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn stream_nodes<F, Fut>(
    client: &LinearClient,
    query: &str,
    base_variables: Map<String, Value>,
    nodes_path: &[&str],
    page_info_path: &[&str],
    options: &PaginationOptions,
    default_page_size: usize,
    mut handler: F,
) -> Result<usize>
where
    F: FnMut(Vec<Value>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut total: usize = 0;
    let limit = if options.all { None } else { options.limit };
    let mut after = options.after.clone();

    let page_size = options.effective_page_size(default_page_size);

    loop {
        // Calculate batch size respecting limit
        let batch_size = limit
            .map(|l| l.saturating_sub(total).min(page_size))
            .unwrap_or(page_size)
            .max(1);

        // If we've already hit the limit, stop
        if let Some(l) = limit {
            if total >= l {
                break;
            }
        }

        // Build per-page variables: pagination keys + base variables
        let mut page_vars = Map::with_capacity(base_variables.len() + 2);
        page_vars.insert(
            "first".to_string(),
            Value::Number(serde_json::Number::from(batch_size as u64)),
        );
        if let Some(ref cursor) = after {
            page_vars.insert("after".to_string(), Value::String(cursor.clone()));
        }
        for (k, v) in &base_variables {
            page_vars.insert(k.clone(), v.clone());
        }

        let result = client.query(query, Some(Value::Object(page_vars))).await?;

        let mut nodes = get_path(&result, nodes_path)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Truncate if we'd exceed the limit
        if let Some(l) = limit {
            let remaining = l.saturating_sub(total);
            if nodes.len() > remaining {
                nodes.truncate(remaining);
            }
        }

        let count = nodes.len();
        if count == 0 {
            break;
        }

        total += count;

        // Process this batch
        handler(nodes).await?;

        // Check if we've hit the limit
        if let Some(l) = limit {
            if total >= l {
                break;
            }
        }

        // Check for more pages (only if we want all or have a limit we haven't reached)
        if !options.all && options.limit.is_none() {
            break;
        }

        let page_info = get_path(&result, page_info_path).and_then(|v| v.as_object());
        let Some(page_info) = page_info else { break };

        let has_next = page_info
            .get("hasNextPage")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !has_next {
            break;
        }

        after = page_info
            .get("endCursor")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if after.is_none() {
            break;
        }
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_options_default() {
        let opts = PaginationOptions::default();
        assert!(opts.limit.is_none());
        assert!(opts.after.is_none());
        assert!(opts.before.is_none());
        assert!(opts.page_size.is_none());
        assert!(!opts.all);
    }

    #[test]
    fn test_with_default_limit_sets_limit_when_none() {
        let opts = PaginationOptions::default();
        let result = opts.with_default_limit(50);
        assert_eq!(result.limit, Some(50));
    }

    #[test]
    fn test_with_default_limit_preserves_existing_limit() {
        let opts = PaginationOptions {
            limit: Some(10),
            ..Default::default()
        };
        let result = opts.with_default_limit(50);
        assert_eq!(result.limit, Some(10));
    }

    #[test]
    fn test_with_default_limit_skipped_when_all() {
        let opts = PaginationOptions {
            all: true,
            ..Default::default()
        };
        let result = opts.with_default_limit(50);
        assert!(result.limit.is_none());
    }

    #[test]
    fn test_with_default_all_sets_all_when_unbounded() {
        // No --limit and no --all: upgrade to fetch every page (detail-view
        // default — this is the projects-truncation fix).
        let opts = PaginationOptions::default();
        let result = opts.with_default_all();
        assert!(result.all);
        assert!(result.limit.is_none());
    }

    #[test]
    fn test_with_default_all_preserves_explicit_limit() {
        // An explicit --limit is a deliberate cap; don't upgrade it to all.
        let opts = PaginationOptions {
            limit: Some(10),
            ..Default::default()
        };
        let result = opts.with_default_all();
        assert!(!result.all);
        assert_eq!(result.limit, Some(10));
    }

    #[test]
    fn test_with_default_all_preserves_explicit_all() {
        let opts = PaginationOptions {
            all: true,
            ..Default::default()
        };
        let result = opts.with_default_all();
        assert!(result.all);
        assert!(result.limit.is_none());
    }

    #[test]
    fn test_with_default_all_carries_through_cursor_and_page_size() {
        // Other pagination fields ride along when we upgrade to all.
        let opts = PaginationOptions {
            after: Some("cursor".to_string()),
            page_size: Some(25),
            ..Default::default()
        };
        let result = opts.with_default_all();
        assert!(result.all);
        assert_eq!(result.after.as_deref(), Some("cursor"));
        assert_eq!(result.page_size, Some(25));
    }

    #[test]
    fn test_effective_page_size_default() {
        let opts = PaginationOptions::default();
        assert_eq!(opts.effective_page_size(100), 100);
    }

    #[test]
    fn test_effective_page_size_custom() {
        let opts = PaginationOptions {
            page_size: Some(25),
            ..Default::default()
        };
        assert_eq!(opts.effective_page_size(100), 25);
    }

    #[test]
    fn test_effective_page_size_minimum_one() {
        let opts = PaginationOptions {
            page_size: Some(0),
            ..Default::default()
        };
        assert_eq!(opts.effective_page_size(100), 1);
    }
}
