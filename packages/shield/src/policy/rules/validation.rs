//! Source predicate admission, domain inventory and path-reference syntax.
use super::{Error, argument_rules, selectors};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn validate_predicates(
    value: &Value,
    domain: &str,
    requirements: bool,
    allow: bool,
) -> Result<(), Error> {
    let (identities, predicates): (&[&str], &[&str]) = match domain {
        "argv" => (
            &["executable"],
            &["arguments", "targets", "possible_effects", "argument_paths"],
        ),
        "shell" => (&["source", "dialect"], &["source", "dialect_version"]),
        "file" => (
            &["action"],
            &["sources", "destination", "recursive", "targets"],
        ),
        "web" => (&["action"], &["url", "query", "method", "request_fields"]),
        "mcp" => (&["server", "tool"], &["arguments", "fields", "connection"]),
        _ => return Err(Error::Contract("unsupported rule domain")),
    };
    let mut allowed = vec!["cwd", "environment"];
    allowed.extend_from_slice(predicates);
    if !requirements {
        allowed.push("domain");
        allowed.extend_from_slice(identities);
    }
    selectors::fields(value, &allowed)?;
    if !requirements {
        for identity in identities {
            if value.get(*identity).is_none() {
                return Err(Error::Contract("domain identity required"));
            }
        }
    }
    for (key, operand) in value.as_object().ok_or(Error::Syntax)? {
        match key.as_str() {
            "domain" => {}
            "executable" if operand.get("root").is_some() => reference(operand)?,
            "executable" | "source" | "query" | "server" | "tool" => {
                selectors::text(operand, key != "source" && key != "query")?;
            }
            "dialect" => enum_selector(operand, &["posix", "bash", "powershell", "cmd"])?,
            "dialect_version" => {
                if selectors::exact(operand).is_none() {
                    return Err(Error::Syntax);
                }
                selectors::text(operand, true)?;
            }
            "action" => enum_selector(
                operand,
                if domain == "file" {
                    &["read", "write", "copy", "move", "remove"]
                } else {
                    &["search", "fetch"]
                },
            )?,
            "arguments" if domain == "argv" => {
                argument_rules::validate(operand)?;
                if operand["kind"] == "risk_tokens" && allow {
                    return Err(Error::Contract("risk tokens cannot allow"));
                }
            }
            "arguments" => {
                if !operand.is_object() || value.get("fields").is_some() {
                    return Err(Error::Syntax);
                }
            }
            "fields" | "request_fields" | "connection" => selectors::field_map(operand)?,
            "environment" => environment(operand)?,
            "cwd" | "destination" => path_predicate(operand)?,
            "sources" => quantified(operand, allow)?,
            "targets" => targets(operand, allow)?,
            "argument_paths" => argument_paths(operand)?,
            "recursive" => {
                if !operand.is_boolean() {
                    return Err(Error::Syntax);
                }
            }
            "possible_effects" => {
                selectors::strings(operand, true, true)?;
                serde_json::from_value::<Vec<crate::effects::Effect>>(operand.clone())
                    .map_err(|_| Error::Contract("invalid effect"))?;
            }
            "url" => url_predicate(operand)?,
            "method" => method(operand)?,
            _ => return Err(Error::Syntax),
        }
    }
    if domain == "shell"
        && allow
        && value
            .get("source")
            .is_some_and(|s| selectors::exact(s).is_none())
    {
        return Err(Error::Contract(
            "source patterns cannot grant whole-shell Allow",
        ));
    }
    Ok(())
}

fn enum_selector(value: &Value, allowed: &[&str]) -> Result<(), Error> {
    if let Some(s) = value.as_str()
        && allowed.contains(&s)
    {
        return Ok(());
    }
    selectors::fields(value, &["one_of"])?;
    selectors::strings(&value["one_of"], true, true)?;
    if value["one_of"]
        .as_array()
        .ok_or(Error::Syntax)?
        .iter()
        .any(|v| !allowed.contains(&v.as_str().unwrap_or_default()))
    {
        return Err(Error::Syntax);
    }
    Ok(())
}

fn argument_paths(value: &Value) -> Result<(), Error> {
    selectors::fields(value, &["grammar", "options", "operands"])?;
    let grammar = &value["grammar"];
    argument_rules::validate(grammar)?;
    if grammar["kind"] != "options" {
        return Err(Error::Contract(
            "argument paths require a complete options grammar",
        ));
    }
    let options = value
        .get("options")
        .map(|v| v.as_object().ok_or(Error::Syntax))
        .transpose()?;
    if options.is_none_or(serde_json::Map::is_empty) && value.get("operands").is_none() {
        return Err(Error::Contract("argument paths require a path selection"));
    }
    if let Some(options) = options {
        if options.len() > 256 {
            return Err(Error::Limit);
        }
        for (name, selection) in options {
            let declared = [grammar, &grammar["leading"]].iter().any(|phase| {
                phase["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|v| v.as_str().or_else(|| v["name"].as_str()) == Some(name.as_str()))
            });
            if !declared {
                return Err(Error::Contract(
                    "argument path option must declare a required value",
                ));
            }
            argument_path(selection)?;
        }
    }
    if let Some(selection) = value.get("operands") {
        argument_path(selection)?;
    }
    Ok(())
}

fn argument_path(value: &Value) -> Result<(), Error> {
    selectors::fields(value, &["path", "recursive"])?;
    path_predicate(&value["path"])?;
    if value.get("recursive").is_some_and(|v| !v.is_boolean()) {
        return Err(Error::Syntax);
    }
    Ok(())
}
fn environment(value: &Value) -> Result<(), Error> {
    selectors::fields(value, &["exact", "fields"])?;
    if value.as_object().is_none_or(|v| v.len() != 1) {
        return Err(Error::Syntax);
    }
    if let Some(exact) = value.get("exact") {
        let values: BTreeMap<String, String> =
            serde_json::from_value(exact.clone()).map_err(|_| Error::Syntax)?;
        if values.len() > 256 {
            return Err(Error::Limit);
        }
        for (name, text) in values {
            selectors::literal(&name, true)?;
            selectors::literal(&text, false)?;
        }
    } else {
        let fields = value["fields"].as_object().ok_or(Error::Syntax)?;
        if fields.is_empty() || fields.len() > 256 {
            return Err(Error::Limit);
        }
        for (name, selector) in fields {
            selectors::literal(name, true)?;
            selectors::text(selector, false)?;
        }
    }
    Ok(())
}
fn reference(value: &Value) -> Result<(), Error> {
    selectors::fields(value, &["root", "path"])?;
    let path = value
        .get("path")
        .map(|v| v.as_str().ok_or(Error::Syntax))
        .transpose()?;
    if let Some(path) = path {
        selectors::literal(path, true)?;
    }
    match value["root"].as_str() {
        Some("project" | "read" | "write")
            if path.is_none_or(|s| {
                !std::path::Path::new(s).is_absolute()
                    && !std::path::Path::new(s).components().any(|c| {
                        matches!(
                            c,
                            std::path::Component::ParentDir | std::path::Component::Prefix(_)
                        )
                    })
            }) =>
        {
            Ok(())
        }
        Some("absolute") if path.is_some_and(|s| std::path::Path::new(s).is_absolute()) => Ok(()),
        _ => Err(Error::Contract("invalid path reference")),
    }
}
fn path_predicate(value: &Value) -> Result<(), Error> {
    selectors::fields(value, &["exact", "within", "outside", "exclude", "match"])?;
    if ["exact", "within", "outside"]
        .iter()
        .filter(|k| value.get(**k).is_some())
        .count()
        != 1
    {
        return Err(Error::Syntax);
    }
    for key in ["exact", "within", "outside"] {
        if let Some(v) = value.get(key) {
            reference(v)?;
        }
    }
    if value.get("within").is_none()
        && (value.get("exclude").is_some() || value.get("match").is_some())
    {
        return Err(Error::Syntax);
    }
    if let Some(items) = value.get("exclude") {
        let items = items.as_array().ok_or(Error::Syntax)?;
        if items.len() > 256 {
            return Err(Error::Limit);
        }
        for item in items {
            reference(item)?;
        }
    }
    if let Some(selector) = value.get("match") {
        selectors::text(selector, false)?;
    }
    Ok(())
}
fn quantifier(value: &Value, allow: bool) -> Result<(), Error> {
    let all = value.get("all");
    let any = value.get("any");
    if all.is_some() == any.is_some() || (allow && any.is_some()) {
        return Err(Error::Contract("invalid path quantifier for Allow"));
    }
    path_predicate(all.or(any).ok_or(Error::Syntax)?)
}
fn quantified(value: &Value, allow: bool) -> Result<(), Error> {
    selectors::fields(value, &["all", "any"])?;
    quantifier(value, allow)
}
fn url_predicate(value: &Value) -> Result<(), Error> {
    selectors::fields(value, &["schemes", "hosts", "ports", "path"])?;
    selectors::strings(&value["schemes"], true, true)?;
    selectors::strings(&value["hosts"], true, true)?;
    for scheme in value["schemes"].as_array().ok_or(Error::Syntax)? {
        if !matches!(scheme.as_str(), Some("http" | "https")) {
            return Err(Error::Syntax);
        }
    }
    for host in value["hosts"].as_array().ok_or(Error::Syntax)? {
        let host = host.as_str().ok_or(Error::Syntax)?;
        let parsed = url::Url::parse(&format!("https://{host}/")).map_err(|_| Error::Syntax)?;
        if parsed.host_str() != Some(host)
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(Error::Contract("noncanonical URL host"));
        }
    }
    if let Some(ports) = value.get("ports") {
        let ports = ports.as_array().ok_or(Error::Syntax)?;
        let mut seen = BTreeSet::new();
        if ports.len() > 256 {
            return Err(Error::Limit);
        }
        if ports.is_empty() {
            return Err(Error::Contract("nonempty list required"));
        }
        for port in ports {
            let port = port.as_u64().ok_or(Error::Syntax)?;
            if !(1..=65535).contains(&port) || !seen.insert(port) {
                return Err(Error::Syntax);
            }
        }
    }
    if let Some(path) = value.get("path") {
        selectors::text(path, false)?;
    }
    Ok(())
}

fn targets(operand: &Value, allow: bool) -> Result<(), Error> {
    let items: Vec<_> = operand
        .as_array()
        .map_or_else(|| vec![operand], |v| v.iter().collect());
    if items.is_empty() || items.len() > 256 {
        return Err(Error::Limit);
    }
    for target in items {
        selectors::fields(target, &["action", "all", "any"])?;
        enum_selector(&target["action"], &["read", "write", "remove"])?;
        if !target["action"].is_string() {
            return Err(Error::Syntax);
        }
        quantifier(target, allow)?;
    }
    Ok(())
}
fn method(operand: &Value) -> Result<(), Error> {
    selectors::text(operand, true)?;
    let values = if let Some(s) = selectors::exact(operand) {
        vec![s]
    } else {
        operand["one_of"]
            .as_array()
            .ok_or(Error::Syntax)?
            .iter()
            .map(|v| v.as_str().ok_or(Error::Syntax))
            .collect::<Result<_, _>>()?
    };
    if values
        .iter()
        .any(|s| !s.bytes().all(|b| b.is_ascii_uppercase() || b == b'-'))
    {
        return Err(Error::Syntax);
    }
    Ok(())
}
