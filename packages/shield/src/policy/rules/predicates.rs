//! Stateless domain/predicate checks over current request facts; no policy authority.
use super::{Error, Facts, Input, Provider, argument_rules, selectors};
use serde_json::{Value, json};

pub(super) fn domain_matches(
    c: &Value,
    input: &Input,
    patterns: &dyn Provider,
) -> Result<bool, Error> {
    Ok(match input {
        Input::Argv { executable, .. } if c["domain"] == "argv" => {
            c["executable"].get("root").is_some()
                || selectors::matches(&c["executable"], executable, patterns)?
        }
        Input::Shell { dialect, .. } if c["domain"] == "shell" => {
            selectors::matches(&c["dialect"], dialect, patterns)?
        }
        Input::File { action, .. } if c["domain"] == "file" => {
            selectors::matches(&c["action"], action, patterns)?
        }
        Input::Web { action, .. } if c["domain"] == "web" => {
            selectors::matches(&c["action"], action, patterns)?
        }
        Input::Mcp { server, tool, .. } if c["domain"] == "mcp" => {
            selectors::matches(&c["server"], server, patterns)?
                && selectors::matches(&c["tool"], tool, patterns)?
        }
        _ => false,
    })
}

pub(super) type Failures = Vec<(String, &'static str)>;
pub(super) fn checks(
    value: &Value,
    facts: &Facts<'_>,
    patterns: &dyn Provider,
) -> Result<Failures, Error> {
    let mut failures = Vec::new();
    for (key, predicate) in value.as_object().ok_or(Error::Syntax)? {
        let codes = predicate_codes(key, predicate, facts, patterns)?;
        for code in codes {
            if !failures.contains(&(key.clone(), code)) {
                failures.push((key.clone(), code));
            }
        }
    }
    Ok(failures)
}

fn predicate_codes(
    key: &str,
    predicate: &Value,
    facts: &Facts<'_>,
    patterns: &dyn Provider,
) -> Result<Vec<&'static str>, Error> {
    let mut codes = Vec::new();
    match key {
        "executable" if predicate.get("root").is_some() => codes.extend(
            facts
                .locations
                .map_or(Some("evidence_unavailable"), |l| l.executable(predicate)),
        ),
        "domain" | "action" | "dialect" | "server" | "tool" | "executable" => {}
        "cwd" => codes.extend(if facts.known_cwd {
            facts
                .locations
                .map_or(Ok(Some("evidence_unavailable")), |l| {
                    l.cwd(predicate, patterns)
                })?
        } else {
            Some("evidence_unavailable")
        }),
        "environment" => {
            if let Some(environment) = &facts.context.environment {
                if let Some(exact) = predicate.get("exact") {
                    if *exact != json!(environment) {
                        codes.push("predicate_mismatch");
                    }
                } else {
                    for (name, selector) in predicate["fields"].as_object().ok_or(Error::Syntax)? {
                        if let Some(v) = environment.get(name) {
                            if !selectors::matches(selector, v, patterns)? {
                                codes.push("predicate_mismatch");
                            }
                        } else {
                            codes.push("field_missing");
                        }
                    }
                }
            } else {
                codes.push("evidence_unavailable");
            }
        }
        "arguments" => match facts.input {
            Input::Argv { arguments, .. } if facts.known_arguments => {
                codes.extend(argument_rules::matches(predicate, arguments, patterns)?.failures);
            }
            Input::Argv { arguments, .. }
                if ["any", "prefix", "risk_tokens"]
                    .contains(&predicate["kind"].as_str().unwrap_or_default()) =>
            {
                codes.extend(argument_rules::matches(predicate, arguments, patterns)?.failures);
            }
            Input::Mcp { arguments, .. } => {
                if predicate != arguments {
                    codes.push("predicate_mismatch");
                }
            }
            _ => codes.push("evidence_unavailable"),
        },
        "argument_paths" => codes.extend(argument_paths(predicate, facts, patterns)?),
        "targets" | "sources" | "destination" if !facts.known_arguments => {
            codes.push("evidence_unavailable");
        }
        "targets" | "sources" | "destination" => codes.extend(
            facts
                .locations
                .map_or(Ok(Some("evidence_unavailable")), |l| {
                    l.predicate(key, predicate, patterns)
                })?,
        ),
        "possible_effects" => {
            if let Input::Argv { .. } = facts.input {
                let effects: Vec<crate::effects::Effect> =
                    serde_json::from_value(predicate.clone()).map_err(|_| Error::Syntax)?;
                if facts
                    .effects
                    .is_none_or(|h| !effects.iter().any(|e| h.may(*e)))
                {
                    codes.push("evidence_unavailable");
                }
            } else {
                codes.push("evidence_unavailable");
            }
        }
        _ => return typed_codes(key, predicate, facts, patterns),
    }
    Ok(codes)
}

fn argument_paths(
    predicate: &Value,
    facts: &Facts<'_>,
    patterns: &dyn Provider,
) -> Result<Option<&'static str>, Error> {
    let Input::Argv { arguments, .. } = facts.input else {
        return Ok(Some("evidence_unavailable"));
    };
    if !facts.known_arguments || !facts.known_cwd {
        return Ok(Some("evidence_unavailable"));
    }
    let parsed = argument_rules::matches(&predicate["grammar"], arguments, patterns)?;
    if let Some(code) = parsed.failures.first() {
        return Ok(Some(*code));
    }
    let Some(locations) = facts.locations else {
        return Ok(Some("evidence_unavailable"));
    };
    let check = |path: &str, selection: &Value| {
        locations.argument_path(
            path,
            &selection["path"],
            selection["recursive"] == true,
            patterns,
        )
    };
    let mut unknown = false;
    for record in &parsed.records {
        if let Some(selection) = predicate["options"].get(&record.name) {
            let Some(path) = record.value.as_deref() else {
                unknown = true;
                continue;
            };
            match check(path, selection)? {
                Some("evidence_unavailable") => unknown = true,
                Some(code) => return Ok(Some(code)),
                None => {}
            }
        }
    }
    if let Some(selection) = predicate.get("operands") {
        for operand in &parsed.operands {
            match check(operand, selection)? {
                Some("evidence_unavailable") => unknown = true,
                Some(code) => return Ok(Some(code)),
                None => {}
            }
        }
    }
    Ok(unknown.then_some("evidence_unavailable"))
}
fn typed_codes(
    key: &str,
    predicate: &Value,
    facts: &Facts<'_>,
    patterns: &dyn Provider,
) -> Result<Vec<&'static str>, Error> {
    let mut codes = Vec::new();
    match key {
        "source" => {
            if let Input::Shell { source, .. } = facts.input {
                if !selectors::matches(predicate, source, patterns)? {
                    codes.push("predicate_mismatch");
                }
            } else {
                codes.push("evidence_unavailable");
            }
        }
        "dialect_version" => {
            if let Input::Shell {
                dialect_version: Some(version),
                ..
            } = facts.input
            {
                if !selectors::matches(predicate, version, patterns)? {
                    codes.push("predicate_mismatch");
                }
            } else {
                codes.push("evidence_unavailable");
            }
        }
        "recursive" => {
            if let Input::File { recursive, .. } = facts.input {
                if predicate.as_bool() != Some(*recursive) {
                    codes.push("predicate_mismatch");
                }
            } else {
                codes.push("evidence_unavailable");
            }
        }
        "fields" => {
            if let Input::Mcp { arguments, .. } = facts.input {
                codes.extend(selectors::fields_match(predicate, arguments, patterns)?);
            } else {
                codes.push("evidence_unavailable");
            }
        }
        "connection" => {
            if let Input::Mcp {
                connection: Some(connection),
                ..
            } = facts.input
            {
                codes.extend(selectors::fields_match(predicate, connection, patterns)?);
            } else {
                codes.push("evidence_unavailable");
            }
        }
        "request_fields" => {
            if let Input::Web { conditions, .. } = facts.input {
                codes.extend(selectors::fields_match(predicate, conditions, patterns)?);
            } else {
                codes.push("evidence_unavailable");
            }
        }
        "query" | "method" => {
            if let Input::Web { query, method, .. } = facts.input {
                if let Some(v) = if key == "query" { query } else { method } {
                    if !selectors::matches(predicate, v, patterns)? {
                        codes.push("predicate_mismatch");
                    }
                } else {
                    codes.push("field_missing");
                }
            } else {
                codes.push("evidence_unavailable");
            }
        }
        "url" => {
            if let Input::Web { url: Some(url), .. } = facts.input {
                codes.extend(url_matches(predicate, url, patterns)?);
            } else {
                codes.push("field_missing");
            }
        }
        _ => return Err(Error::Contract("unknown prepared predicate")),
    }
    Ok(codes)
}

fn url_matches(
    predicate: &Value,
    input: &str,
    patterns: &dyn Provider,
) -> Result<Option<&'static str>, Error> {
    if input.chars().any(char::is_control) || input.contains('\\') || !input.contains("://") {
        return Ok(Some("predicate_mismatch"));
    }
    let Ok(url) = url::Url::parse(input) else {
        return Ok(Some("predicate_mismatch"));
    };
    let scheme = predicate["schemes"]
        .as_array()
        .ok_or(Error::Syntax)?
        .iter()
        .any(|s| s.as_str() == Some(url.scheme()));
    let host = predicate["hosts"]
        .as_array()
        .ok_or(Error::Syntax)?
        .iter()
        .any(|s| s.as_str() == url.host_str());
    let port = predicate.get("ports").is_none_or(|p| {
        p.as_array().is_some_and(|v| {
            v.iter()
                .any(|n| n.as_u64() == url.port_or_known_default().map(u64::from))
        })
    });
    if !scheme || !host || !port {
        return Ok(Some("predicate_mismatch"));
    }
    if let Some(path) = predicate.get("path")
        && !selectors::matches(path, url.path(), patterns)?
    {
        return Ok(Some("predicate_mismatch"));
    }
    Ok(None)
}
