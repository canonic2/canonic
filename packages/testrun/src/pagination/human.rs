//! Copyable CLI continuations retain selectors, filters and shell quoting.
use serde_json::Value;

fn find(value: &Value, field: &str) -> Option<Value> {
    match value {
        Value::Object(fields) => fields
            .get(field)
            .filter(|value| {
                if field == "nextCursor" {
                    value.is_string()
                } else {
                    value.is_object()
                }
            })
            .cloned()
            .or_else(|| fields.values().find_map(|value| find(value, field))),
        Value::Array(values) => values.iter().find_map(|value| find(value, field)),
        _ => None,
    }
}
fn command(words: Vec<String>) -> String {
    words
        .into_iter()
        .map(|word| {
            if !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:/@=".contains(&byte))
            {
                word
            } else if cfg!(windows) {
                format!("'{}'", word.replace('\'', "''"))
            } else {
                format!("'{}'", word.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
pub(crate) fn hint(args: impl IntoIterator<Item = String>, value: &Value) -> Option<String> {
    let mut words = vec!["testrun".to_owned()];
    if let Some(mut request) = find(value, "nextRequest") {
        let target = format!("browser:{}", request["context"]["connectionId"].as_str()?);
        let paging = if request["operation"] == "diagnostics_read" {
            &mut request["query"]
        } else {
            &mut request
        };
        let cursor = paging["cursor"].as_str()?.to_owned();
        let limit = paging["limit"].as_u64().unwrap_or(50);
        paging.as_object_mut()?.remove("cursor");
        paging.as_object_mut()?.remove("limit");
        words.extend([
            "--target".into(),
            target,
            "browser".into(),
            "send".into(),
            request.to_string(),
            "--cursor".into(),
            cursor,
            "--limit".into(),
            limit.to_string(),
        ]);
    } else {
        let cursor = find(value, "nextCursor")?.as_str()?.to_owned();
        let mut args = args.into_iter().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--cursor" {
                args.next();
                continue;
            }
            if arg.starts_with("--cursor=") || arg == "--json" {
                continue;
            }
            words.push(arg);
        }
        words.extend(["--cursor".into(), cursor]);
    }
    Some(format!("Next page: {}", command(words)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn hints_preserve_selectors_and_quote_filters() {
        let hint = hint(
            [
                "testrun",
                "--target",
                "device:acme",
                "app",
                "list",
                "--name",
                "Acme's game",
                "--limit",
                "2",
                "--cursor",
                "previous",
            ]
            .map(str::to_owned),
            &json!({"nextCursor":"opaque"}),
        )
        .unwrap();
        assert!(hint.starts_with("Next page: testrun --target device:acme app list --name "));
        assert!(hint.contains("--limit 2"));
        assert!(hint.ends_with("--cursor opaque"));
        assert!(!hint.contains("previous"));
    }
}
