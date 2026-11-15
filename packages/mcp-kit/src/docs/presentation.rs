//! Plain documentation pages and shell-quoted continuation commands, without I/O.

use serde_json::Value;
use std::fmt::Write as _;

/// Formats a documentation result for a terminal. `command` is the product's
/// complete docs command prefix, such as `acme docs search`. Arguments are the
/// same effective MCP arguments used to obtain the result. The next-page line
/// preserves query, path and limit and shell-quotes every value.
#[must_use]
pub fn human(command: &str, arguments: &Value, result: &Value) -> String {
    let mut output = String::new();
    if let Some(content) = result["content"].as_str() {
        output.push_str(content);
    }
    for row in result["documents"].as_array().into_iter().flatten() {
        let _ = writeln!(
            output,
            "{}  {} ({} lines)",
            text(&row["path"]),
            text(&row["title"]),
            row["total_lines"]
        );
    }
    for row in result["matches"].as_array().into_iter().flatten() {
        let _ = writeln!(
            output,
            "{}:{}: {}",
            text(&row["path"]),
            row["line"],
            text(&row["text"])
        );
    }
    if let Some(cursor) = result["next_cursor"].as_str() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        let _ = write!(output, "Next: {command}");
        if let Some(path) = arguments["path"].as_str()
            && arguments["query"].is_string()
        {
            let _ = write!(output, " --path={}", quote(path));
        }
        if let Some(limit) = arguments["limit"].as_u64() {
            let _ = write!(output, " --limit {limit}");
        }
        let _ = write!(output, " --cursor={}", quote(cursor));
        if let Some(positional) = arguments["query"]
            .as_str()
            .or_else(|| arguments["path"].as_str())
        {
            let _ = write!(output, " -- {}", quote(positional));
        }
        output.push('\n');
    }
    if output.is_empty() && !result["content"].is_string() {
        output.push_str("Nothing found.\n");
    }
    output
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn continuation_preserves_filters_and_quotes_shell_values() {
        let output = human(
            "acme docs search",
            &json!({"query":"it's a test", "path":"guides/a b.md", "limit":1}),
            &json!({"matches":[],"next_cursor":"abc_def"}),
        );
        assert_eq!(
            output,
            "Next: acme docs search --path='guides/a b.md' --limit 1 --cursor='abc_def' -- 'it'\\''s a test'\n"
        );
    }
}
