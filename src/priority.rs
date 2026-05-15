use colored::Colorize;

pub fn priority_to_string(priority: Option<i64>) -> String {
    match priority {
        Some(0) => "-".to_string(),
        Some(1) => "Urgent".red().to_string(),
        Some(2) => "High".yellow().to_string(),
        Some(3) => "Normal".to_string(),
        Some(4) => "Low".dimmed().to_string(),
        _ => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests that compare colored output force the colored crate's override
    // to `true`. Otherwise the production call and the test's expected value
    // each consult `should_colorize()` independently, and colored 2.x runs a
    // fresh `atty::is(Stdout)` check on every call — under parallel
    // `cargo test` the two consecutive checks can return different answers
    // on the same thread, so one side emits ANSI escapes and the other does
    // not. Forcing override(true) bypasses the racy TTY check.

    #[test]
    fn test_priority_none() {
        colored::control::set_override(true);
        assert_eq!(priority_to_string(None), "-");
    }

    #[test]
    fn test_priority_zero() {
        colored::control::set_override(true);
        assert_eq!(priority_to_string(Some(0)), "-");
    }

    #[test]
    fn test_priority_urgent() {
        colored::control::set_override(true);
        assert_eq!(priority_to_string(Some(1)), "Urgent".red().to_string());
    }

    #[test]
    fn test_priority_high() {
        colored::control::set_override(true);
        assert_eq!(priority_to_string(Some(2)), "High".yellow().to_string());
    }

    #[test]
    fn test_priority_normal() {
        colored::control::set_override(true);
        assert_eq!(priority_to_string(Some(3)), "Normal");
    }

    #[test]
    fn test_priority_low() {
        colored::control::set_override(true);
        assert_eq!(priority_to_string(Some(4)), "Low".dimmed().to_string());
    }

    #[test]
    fn test_priority_invalid() {
        colored::control::set_override(true);
        assert_eq!(priority_to_string(Some(5)), "-");
        assert_eq!(priority_to_string(Some(-1)), "-");
    }
}
