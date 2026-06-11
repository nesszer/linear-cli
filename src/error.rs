use serde_json::Value;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    General,     // exit code 1
    NotFound,    // exit code 2
    Auth,        // exit code 3
    RateLimited, // exit code 4
}

impl ErrorKind {
    pub fn exit_code(self) -> u8 {
        match self {
            ErrorKind::General => 1,
            ErrorKind::NotFound => 2,
            ErrorKind::Auth => 3,
            ErrorKind::RateLimited => 4,
        }
    }

    pub fn is_retryable(self) -> bool {
        matches!(self, ErrorKind::RateLimited)
    }
}

#[derive(Debug)]
pub struct CliError {
    pub kind: ErrorKind,
    pub message: String,
    pub details: Option<Value>,
    pub retry_after: Option<u64>,
}

impl CliError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            details: None,
            retry_after: None,
        }
    }

    pub fn general(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::General, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Auth, message)
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::RateLimited, message)
    }

    pub fn code(&self) -> u8 {
        self.kind.exit_code()
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn with_retry_after(mut self, retry_after: Option<u64>) -> Self {
        self.retry_after = retry_after;
        self
    }

    /// Classify a GraphQL `errors` array into a `CliError` with a precise exit code.
    ///
    /// Linear reports application-level failures as HTTP 200 with an `errors`
    /// array, so the real signal lives in the first error's `extensions`
    /// (`statusCode` / `code`) or its message text rather than the transport
    /// status. The message stays "GraphQL error" so `Display` still appends the
    /// underlying error messages.
    pub fn from_graphql_errors(errors: &Value) -> Self {
        let first = errors.as_array().and_then(|arr| arr.first());
        let kind = first.map_or(ErrorKind::General, Self::classify_graphql_error);
        let mut err = Self::new(kind, "GraphQL error").with_details(errors.clone());
        if kind == ErrorKind::RateLimited {
            let retry_after = first
                .and_then(|e| e.get("extensions"))
                .and_then(|ext| ext.get("retryAfter"))
                .and_then(Value::as_u64);
            err = err.with_retry_after(retry_after);
        }
        err
    }

    fn classify_graphql_error(error: &Value) -> ErrorKind {
        let ext = error.get("extensions");

        // 1. Numeric status code carried in extensions, when present.
        if let Some(status) = ext
            .and_then(|e| e.get("statusCode"))
            .and_then(Value::as_u64)
        {
            match status {
                401 | 403 => return ErrorKind::Auth,
                404 => return ErrorKind::NotFound,
                429 => return ErrorKind::RateLimited,
                _ => {}
            }
        }

        // 2. Symbolic code string (e.g. AUTHENTICATION_ERROR, RATELIMITED).
        if let Some(code) = ext.and_then(|e| e.get("code")).and_then(Value::as_str) {
            let code = code.to_ascii_uppercase();
            if code.contains("AUTH") {
                return ErrorKind::Auth;
            }
            if code.contains("RATELIM") {
                return ErrorKind::RateLimited;
            }
            if code.contains("NOT_FOUND") {
                return ErrorKind::NotFound;
            }
        }

        // 3. Message text — Linear reports "Entity not found" with statusCode 400.
        let msg = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| {
                ext.and_then(|e| e.get("userPresentableMessage"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("");
        let lower = msg.to_ascii_lowercase();
        if lower.contains("not found") || lower.contains("could not find") {
            return ErrorKind::NotFound;
        }

        ErrorKind::General
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;

        // Extract and display GraphQL error messages from details if present
        if let Some(details) = &self.details {
            if let Some(errors) = details.as_array() {
                // details is a GraphQL errors array
                let messages: Vec<&str> = errors
                    .iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect();
                if !messages.is_empty() {
                    write!(f, ": {}", messages.join("; "))?;
                }
            } else if let Some(message) = details.get("message").and_then(|m| m.as_str()) {
                // details is an object with a message field
                write!(f, ": {}", message)?;
            } else if let Some(errors) = details.get("errors").and_then(|e| e.as_array()) {
                // details has a nested errors array
                let messages: Vec<&str> = errors
                    .iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect();
                if !messages.is_empty() {
                    write!(f, ": {}", messages.join("; "))?;
                }
            }
        }

        Ok(())
    }
}

impl Error for CliError {}

/// Re-wrap `source` with a new human-readable `message`, preserving the original
/// [`CliError`]'s `kind` / `details` / `retry_after` when `source` is (or wraps,
/// via `anyhow` context) a `CliError`.
///
/// This is the canonical place commands re-message an error without losing its
/// exit-code semantics: a wrapped rate-limited error stays [`ErrorKind::RateLimited`]
/// (exit 4) with its `retry_after`, instead of being flattened to a generic
/// error. When `source` is not a `CliError`, a generic error carrying `message`
/// is returned — callers should embed the original cause in `message` so the
/// string-based exit-code heuristics still apply.
pub fn rewrap_with_message(source: &anyhow::Error, message: impl Into<String>) -> anyhow::Error {
    let message = message.into();
    match source.downcast_ref::<CliError>() {
        Some(cli) => {
            let mut wrapped = CliError::new(cli.kind, message).with_retry_after(cli.retry_after);
            if let Some(details) = &cli.details {
                wrapped = wrapped.with_details(details.clone());
            }
            wrapped.into()
        }
        None => anyhow::anyhow!("{message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_display_without_details() {
        let err = CliError::general("Simple error");
        assert_eq!(err.to_string(), "Simple error");
    }

    #[test]
    fn test_display_with_graphql_errors_array() {
        let errors = json!([
            {"message": "Field 'foo' not found"},
            {"message": "Invalid query syntax"}
        ]);
        let err = CliError::general("GraphQL error").with_details(errors);
        assert_eq!(
            err.to_string(),
            "GraphQL error: Field 'foo' not found; Invalid query syntax"
        );
    }

    #[test]
    fn test_display_with_single_graphql_error() {
        let errors = json!([{"message": "Entity not found"}]);
        let err = CliError::general("GraphQL error").with_details(errors);
        assert_eq!(err.to_string(), "GraphQL error: Entity not found");
    }

    #[test]
    fn test_display_with_object_message() {
        let details = json!({"message": "Rate limit exceeded", "code": 429});
        let err = CliError::rate_limited("API error").with_details(details);
        assert_eq!(err.to_string(), "API error: Rate limit exceeded");
    }

    #[test]
    fn test_display_with_nested_errors_array() {
        let details = json!({
            "errors": [
                {"message": "Permission denied"},
                {"message": "Insufficient scope"}
            ]
        });
        let err = CliError::auth("Auth error").with_details(details);
        assert_eq!(
            err.to_string(),
            "Auth error: Permission denied; Insufficient scope"
        );
    }

    #[test]
    fn test_display_with_empty_errors_array() {
        let errors = json!([]);
        let err = CliError::general("GraphQL error").with_details(errors);
        assert_eq!(err.to_string(), "GraphQL error");
    }

    #[test]
    fn test_display_with_errors_missing_message() {
        let errors = json!([{"code": 123}, {"extensions": {}}]);
        let err = CliError::general("GraphQL error").with_details(errors);
        assert_eq!(err.to_string(), "GraphQL error");
    }

    #[test]
    fn test_error_kind_exit_codes() {
        assert_eq!(ErrorKind::General.exit_code(), 1);
        assert_eq!(ErrorKind::NotFound.exit_code(), 2);
        assert_eq!(ErrorKind::Auth.exit_code(), 3);
        assert_eq!(ErrorKind::RateLimited.exit_code(), 4);
    }

    #[test]
    fn test_error_kind_retryable() {
        assert!(!ErrorKind::General.is_retryable());
        assert!(!ErrorKind::NotFound.is_retryable());
        assert!(!ErrorKind::Auth.is_retryable());
        assert!(ErrorKind::RateLimited.is_retryable());
    }

    #[test]
    fn test_convenience_constructors() {
        let err = CliError::not_found("Issue not found");
        assert_eq!(err.code(), 2);
        assert_eq!(err.kind, ErrorKind::NotFound);
    }

    #[test]
    fn rewrap_preserves_kind_and_retry_after() {
        let source: anyhow::Error = CliError::rate_limited("429")
            .with_retry_after(Some(9))
            .into();
        let wrapped = rewrap_with_message(&source, "while fetching LIN-1");
        let cli = wrapped
            .downcast_ref::<CliError>()
            .expect("CliError preserved");
        assert_eq!(cli.kind, ErrorKind::RateLimited);
        assert_eq!(cli.code(), 4);
        assert_eq!(cli.retry_after, Some(9));
        assert_eq!(cli.message, "while fetching LIN-1");
    }

    #[test]
    fn rewrap_preserves_details() {
        let source: anyhow::Error = CliError::general("boom")
            .with_details(json!({ "field": "value" }))
            .into();
        let wrapped = rewrap_with_message(&source, "new message");
        let cli = wrapped
            .downcast_ref::<CliError>()
            .expect("CliError preserved");
        assert_eq!(cli.details, Some(json!({ "field": "value" })));
    }

    #[test]
    fn rewrap_plain_error_keeps_message() {
        let source = anyhow::anyhow!("not found");
        let wrapped = rewrap_with_message(&source, "Failed to resolve 'x': not found");
        assert!(wrapped.downcast_ref::<CliError>().is_none());
        assert_eq!(wrapped.to_string(), "Failed to resolve 'x': not found");
    }

    #[test]
    fn rewrap_finds_cli_error_behind_anyhow_context() {
        use anyhow::Context;
        // A CliError wrapped in an anyhow context layer must still be found,
        // so re-messaging across command boundaries preserves the exit code.
        let source = Err::<(), _>(CliError::auth("401"))
            .context("resolving user")
            .unwrap_err();
        let wrapped = rewrap_with_message(&source, "Failed to resolve user 'me'");
        assert_eq!(
            wrapped
                .downcast_ref::<CliError>()
                .expect("CliError preserved")
                .code(),
            3
        );
    }

    #[test]
    fn graphql_entity_not_found_maps_to_not_found() {
        // Linear returns "Entity not found" as HTTP 200 + errors array with
        // statusCode 400 (NOT 404), so classification must fall through to the
        // message-text check. This is the common `i get <bad-id>` path.
        let errors = json!([{
            "message": "Entity not found: Issue",
            "extensions": {
                "code": "INPUT_ERROR",
                "statusCode": 400,
                "type": "invalid input",
                "userError": true,
                "userPresentableMessage": "Could not find referenced Issue."
            }
        }]);
        let err = CliError::from_graphql_errors(&errors);
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_eq!(err.code(), 2);
        assert_eq!(err.to_string(), "GraphQL error: Entity not found: Issue");
    }

    #[test]
    fn graphql_auth_error_maps_to_auth() {
        let errors = json!([{
            "message": "Authentication required",
            "extensions": { "code": "AUTHENTICATION_ERROR" }
        }]);
        let err = CliError::from_graphql_errors(&errors);
        assert_eq!(err.kind, ErrorKind::Auth);
        assert_eq!(err.code(), 3);
    }

    #[test]
    fn graphql_rate_limit_maps_to_rate_limited_with_retry_after() {
        let errors = json!([{
            "message": "Too many requests",
            "extensions": { "code": "RATELIMITED", "retryAfter": 30 }
        }]);
        let err = CliError::from_graphql_errors(&errors);
        assert_eq!(err.kind, ErrorKind::RateLimited);
        assert_eq!(err.code(), 4);
        assert_eq!(err.retry_after, Some(30));
    }

    #[test]
    fn graphql_status_code_takes_precedence() {
        // A 403 in extensions classifies as Auth even without a code string.
        let errors = json!([{
            "message": "denied",
            "extensions": { "statusCode": 403 }
        }]);
        assert_eq!(CliError::from_graphql_errors(&errors).code(), 3);
    }

    #[test]
    fn graphql_generic_error_stays_general() {
        let errors = json!([{ "message": "Field 'foo' is invalid" }]);
        let err = CliError::from_graphql_errors(&errors);
        assert_eq!(err.kind, ErrorKind::General);
        assert_eq!(err.code(), 1);
    }

    #[test]
    fn graphql_empty_errors_stays_general() {
        let errors = json!([]);
        assert_eq!(
            CliError::from_graphql_errors(&errors).kind,
            ErrorKind::General
        );
    }
}
