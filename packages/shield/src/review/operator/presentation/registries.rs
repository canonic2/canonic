//! Registry views: registrations in effect, reviews of registry changes and their receipts.
use super::{label, short};
use serde_json::Value;
use std::fmt::Write;

/// Where a registration's packs come from and how they are verified.
fn origin(source: &Value) -> String {
    if source["key"].is_string() {
        format!(
            "{} · signed by key {} · registry {}",
            label(&source["location"]),
            short(&source["key"]),
            label(&source["registry_id"])
        )
    } else {
        format!("{} · local directory", label(&source["location"]))
    }
}

/// `shield registry list`.
pub(super) fn list(value: &Value) -> String {
    let sources: Vec<_> = value["sources"].as_object().into_iter().flatten().collect();
    let width = sources
        .iter()
        .map(|(alias, _)| alias.len())
        .max()
        .unwrap_or(0);
    sources
        .iter()
        .fold(String::new(), |mut output, (alias, source)| {
            let scope = match source["scope"].as_str() {
                Some("builtin") => "built in",
                Some("global") => "user",
                Some(scope) => scope,
                None => "unknown",
            };
            let _ = write!(output, "{alias:<width$}  {} · {scope}", origin(source));
            if value["default"] == alias.as_str() {
                output.push_str(" · default");
            }
            output.push('\n');
            output
        })
}

fn scope(value: &Value) -> &'static str {
    if value["scope"] == "user" {
        "all projects"
    } else {
        "this project"
    }
}

/// The review of a registry change.
pub(super) fn review(value: &Value) -> Option<String> {
    if value["operation"] != "registry configuration" {
        return None;
    }
    let alias = label(&value["alias"]);
    let text = match value["change"].as_str()? {
        "add" => format!(
            "Register {alias} for {}\n  {}\nInstalled packs are not affected.\n",
            scope(value),
            origin(&value["source"])
        ),
        "remove" => {
            let mut text = format!(
                "Remove registry {alias} from {}\n  {}\nInstalled packs keep their source.\n",
                scope(value),
                origin(&value["source"])
            );
            if value["default"]["before"] != value["default"]["after"] {
                let _ = writeln!(
                    text,
                    "The default registry becomes {}.",
                    label(&value["default"]["after"])
                );
            }
            text
        }
        _ => format!(
            "Make {alias} the default registry for {} (was {})\n",
            scope(value),
            label(&value["default"]["before"])
        ),
    };
    Some(text)
}

/// The receipt of a confirmed registry change.
pub(super) fn receipt(value: &Value) -> String {
    let alias = label(&value["alias"]);
    let mut text = match value["change"].as_str() {
        Some("add") => format!("Registered {alias} for {}.", scope(value)),
        Some("remove") => format!("Removed registry {alias} from {}.", scope(value)),
        Some("default") => format!("{alias} is the default registry for {}.", scope(value)),
        _ => return super::fallback(value),
    };
    if value["change"] == "remove" && value["default"]["before"] != value["default"]["after"] {
        let _ = write!(
            text,
            " The default registry is {}.",
            label(&value["default"]["after"])
        );
    }
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registrations_show_origin_scope_and_default() {
        let text = list(&json!({"default":"acme","sources":{
            "acme":{"location":"/srv/acme","key":null,"scope":"project"},
            "main":{"location":"https://example.com/registry/","key":"0123456789abcdef","registry_id":"acme-main","scope":"builtin"}}}));
        assert_eq!(
            text,
            "acme  /srv/acme · local directory · project · default\nmain  https://example.com/registry/ · signed by key 0123456789ab · registry acme-main · built in\n"
        );
    }

    #[test]
    fn reviews_and_receipts_name_the_registry_and_scope() {
        let change = json!({"operation":"registry configuration","change":"remove","alias":"acme","scope":"project",
            "source":{"location":"/srv/acme"},"default":{"before":"acme","after":"main"}});
        assert_eq!(
            review(&change).expect("review"),
            "Remove registry acme from this project\n  /srv/acme · local directory\nInstalled packs keep their source.\nThe default registry becomes main.\n"
        );
        assert_eq!(
            receipt(&change),
            "Removed registry acme from this project. The default registry is main.\n"
        );
    }
}
