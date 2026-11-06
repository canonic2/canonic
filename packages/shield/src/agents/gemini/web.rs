//! The pinned host tokenizes prompts on whitespace, then rewrites GitHub blobs.
use super::text;
use serde_json::{Value, json};

pub(super) fn operation(input: &Value) -> Option<Value> {
    // Default execution fetches prompt URLs; direct-fetch mode uses url.
    // The hook does not identify that mode, so neither field can mask the other.
    if input.get("url").is_some() && input.get("prompt").is_some() {
        return None;
    }
    let targets: Vec<&str> = if let Some(value) = input.get("url") {
        vec![text(value, 8192)?]
    } else {
        text(&input["prompt"], 32768)?
            .split_whitespace()
            .filter(|s| s.contains("://"))
            .collect()
    };
    if targets.is_empty() || targets.len() > 16 {
        return None;
    }
    let mut urls = std::collections::BTreeSet::new();
    for target in targets {
        if target.chars().any(char::is_control) || target.contains('\\') {
            return None;
        }
        let mut url = url::Url::parse(target).ok()?;
        if !["https", "http"].contains(&url.scheme()) || url.host_str().is_none() {
            return None;
        }
        urls.insert(url.to_string());
        if url.host_str() == Some("github.com") && url.path().contains("/blob/") {
            let parts: Vec<_> = url.path().split('/').collect();
            if parts.len() >= 5 && parts[3] == "blob" {
                let path = format!("/{}/{}/{}", parts[1], parts[2], parts[4..].join("/"));
                url.set_host(Some("raw.githubusercontent.com")).ok()?;
                url.set_path(&path);
                urls.insert(url.to_string());
            }
        }
    }
    let operations: Vec<_> = urls
        .into_iter()
        .map(|url| json!({"kind":"web","action":"fetch","query":null,"url":url}))
        .collect();
    Some(json!({"kind":"batch","operations":operations}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gemini_web_fetch_rejects_mixed_url_and_prompt() {
        for input in [
            json!({"url":"https://docs.example.com/","prompt":"Fetch https://blocked.example.com/"}),
            json!({"url":"https://blocked.example.com/","prompt":"Fetch https://docs.example.com/"}),
            json!({"url":"https://docs.example.com/","prompt":"Fetch https://docs.example.com/"}),
            json!({"url":"https://docs.example.com/","prompt":null}),
            json!({"url":null,"prompt":"Fetch https://docs.example.com/"}),
        ] {
            assert!(operation(&input).is_none(), "{input}");
        }
    }
    #[test]
    fn gemini_web_fetch_maps_every_target_and_the_hosts_github_rewrite() {
        let value=operation(&json!({"prompt":"Compare https://docs.example.com/a with https://other.example.com/b"})).unwrap();
        assert_eq!(value["operations"].as_array().unwrap().len(), 2);
        let value =
            operation(&json!({"url":"https://github.com/acme/demo/blob/main/a.md"})).unwrap();
        assert_eq!(value["operations"].as_array().unwrap().len(), 2);
        assert!(
            value
                .to_string()
                .contains("raw.githubusercontent.com/acme/demo/main/a.md")
        );
        for prompt in [
            "no URL",
            "https://example.com \\https://other.example.com",
            "file:///tmp/acme",
        ] {
            assert!(operation(&json!({"prompt":prompt})).is_none());
        }
    }
}
