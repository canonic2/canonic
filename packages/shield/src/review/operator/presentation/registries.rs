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

fn trust(text: &mut String, value: &Value) {
    for (field, name) in [
        ("source_identity", "Source identity"),
        ("registry_identity", "Registry identity"),
        ("signing_key_fingerprint_sha256", "Signing key SHA-256"),
        ("basis", "Trust basis"),
    ] {
        if value[field].is_string() {
            let _ = writeln!(text, "  {name}: {}", label(&value[field]));
        }
    }
}

/// The review of a registry change.
pub(super) fn review(value: &Value) -> Option<String> {
    if value["operation"] == "registry versioned trust" {
        return Some(versioned_trust(value));
    }
    configuration_review(value)
}
fn versioned_trust(value: &Value) -> String {
    let mut text = format!(
        "Trust delegated registry roles for {} across this user's projects\n  {}\n",
        label(&value["alias"]),
        origin(&value["source"])
    );
    trust(&mut text, &value["trust"]);
    for (name, root) in [
        ("Previous", &value["previous_root"]),
        ("Delegated", &value["root"]),
    ] {
        if !root.is_object() {
            continue;
        }
        let _ = writeln!(
            text,
            "{name} root version {} · expires {}",
            label(&root["version"]),
            label(&root["expires_at"])
        );
        for role in ["root", "catalog", "revocations"] {
            let fingerprints = root[role]["key_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .map(label)
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                text,
                "  {role}: threshold {} · key SHA-256 {fingerprints}",
                label(&root[role]["threshold"])
            );
        }
    }
    let _ = writeln!(
        text,
        "Revocation sequence {}\n{}\n{}",
        label(&value["revocation_sequence"]),
        label(&value["scope_effect"]),
        label(&value["policy_effect"])
    );
    text
}
fn configuration_review(value: &Value) -> Option<String> {
    if value["operation"] != "registry configuration" {
        return None;
    }
    let alias = label(&value["alias"]);
    let mut text = match value["change"].as_str()? {
        "add" | "replace" => {
            let mut text = if value["change"] == "replace" {
                let mut text = format!(
                    "Replace registry {alias} for {}\nPrevious source:\n  {}\n",
                    scope(value),
                    origin(&value["previous_source"])
                );
                trust(&mut text, &value["previous_trust"]);
                text.push_str("Replacement source:\n");
                text
            } else {
                format!("Register {alias} for {}\n", scope(value))
            };
            let _ = writeln!(text, "  {}", origin(&value["source"]));
            trust(&mut text, &value["trust"]);
            text.push_str("Installed packs keep their source and policy.\n");
            text
        }
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
    let before = &value["effective_source"]["before"];
    let after = &value["effective_source"]["after"];
    if value["scope"] == "project" {
        let affected = if value["change"] == "remove" && after["scope"] == "global" {
            Some(("Reveals user registry", after))
        } else if value["change"] == "add" && before["scope"] == "global" {
            Some(("Shadows user registry", before))
        } else {
            None
        };
        if let Some((effect, source)) = affected {
            let _ = writeln!(text, "{effect} {alias}:\n  {}", origin(&source["source"]));
            trust(&mut text, &source["trust"]);
        }
    }
    Some(text)
}

/// The receipt of a confirmed registry change.
pub(super) fn receipt(value: &Value) -> String {
    let alias = label(&value["alias"]);
    let mut text = match value["change"].as_str() {
        Some("add") => format!("Registered {alias} for {}.", scope(value)),
        Some("replace") => format!("Replaced registry {alias} for {}.", scope(value)),
        Some("remove") => format!("Removed registry {alias} from {}.", scope(value)),
        Some("default") => format!("{alias} is the default registry for {}.", scope(value)),
        Some("adopt-versioned-trust") => format!(
            "Trusted delegated registry roles for {alias} across this user's projects. Known revoked artifacts are blocked; policy permissions require separate review."
        ),
        _ => return super::fallback(value),
    };
    if value["change"] == "remove" && value["default"]["before"] != value["default"]["after"] {
        let _ = write!(
            text,
            " The default registry is {}.",
            label(&value["default"]["after"])
        );
    }
    if value["replayed"] == true {
        text.push_str(" Reconciled the committed change.");
    }
    if value["repair_needed"] == true {
        text.push_str(" The change is committed; preserve conflicting manual registry edits and reconcile the configuration before another change.");
    }
    if value["durability"] == "unconfirmed" {
        text.push_str(" Durability is unconfirmed; retry to reconcile this committed generation.");
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

    #[test]
    fn replacement_and_revealed_user_source_show_their_exact_trust() {
        let replacement = json!({"operation":"registry configuration","change":"replace","alias":"acme","scope":"project",
            "previous_source":{"location":"https://old.example.com/"},"previous_trust":{"source_identity":"old-identity","signing_key_fingerprint_sha256":"old-fingerprint"},
            "source":{"location":"https://new.example.com/"},"trust":{"source_identity":"new-identity","signing_key_fingerprint_sha256":"new-fingerprint"}});
        let text = review(&replacement).unwrap();
        for expected in [
            "Replace registry acme",
            "old.example.com",
            "new.example.com",
            "old-identity",
            "new-identity",
            "old-fingerprint",
            "new-fingerprint",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        let removal = json!({"operation":"registry configuration","change":"remove","alias":"acme","scope":"project",
            "source":{"location":"/srv/project"},"default":{"before":"acme","after":"acme"},
            "effective_source":{"after":{"scope":"global","source":{"location":"/srv/user"},"trust":{"source_identity":"user-identity"}}}});
        let text = review(&removal).unwrap();
        assert!(text.contains("Reveals user registry acme"));
        assert!(text.contains("/srv/user"));
        assert!(text.contains("user-identity"));
    }
}
