//! The normalized custom document shipped beside the original matcher schema.
use crate::policy::{
    Decision, Error, model,
    normalized::{Context, Input},
    rules,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::policy) struct NormalizedAuthority {
    pub(in crate::policy) schema_version: u8,
    pub(in crate::policy) id: String,
    pub(in crate::policy) decision: Decision,
    pub(in crate::policy) conditions: Value,
    #[serde(deserialize_with = "nullable")]
    pub(in crate::policy) reason: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub(in crate::policy) expires_at: Option<String>,
}

fn nullable<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    Option::deserialize(input)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u8,
    rules: Vec<NormalizedAuthority>,
}

pub(super) fn document(value: Value, input: &str) -> Result<model::CustomDocument, Error> {
    let source: Document = super::decode(value, input)?;
    if source.schema_version != 1 {
        return Err(Error::Contract("invalid historical project document"));
    }
    super::rule_count(source.rules.len())?;
    let mut ids = BTreeSet::new();
    let mut rules = Vec::with_capacity(source.rules.len());
    for row in source.rules {
        row.validate()?;
        if !ids.insert(row.id.clone()) {
            return Err(Error::Contract("duplicate custom rule ID"));
        }
        rules.push(row.translate()?);
    }
    model::CustomDocument::from_rules(rules)
}

impl NormalizedAuthority {
    fn translate(&self) -> Result<rules::Rule, Error> {
        let mut value = json!({"id":self.id,"decision":self.decision,"conditions":self.conditions});
        if let Some(reason) = &self.reason {
            value["reason"] = json!(reason);
        }
        if let Some(expiry) = &self.expires_at {
            value["expires_at"] = json!(expiry);
        }
        let mut rule: rules::Rule = serde_json::from_value(value).map_err(|_| Error::Syntax)?;
        rule.legacy_normalized = Some(self.clone());
        Ok(rule)
    }

    pub(in crate::policy) fn validate(&self) -> Result<(), Error> {
        if self.schema_version != 1
            || self.id.is_empty()
            || self.id.len() > 128
            || self.reason.as_ref().is_some_and(|s| s.len() > 4096)
            || crate::json::contains_nul(&json!(self))
        {
            return Err(Error::Contract("invalid historical project rule"));
        }
        if self.expires_at.as_deref().is_some_and(|s| !timestamp(s)) {
            return Err(Error::Contract("invalid rule expiration"));
        }
        let c = &self.conditions;
        let allowed: &[&str] = match c["domain"].as_str() {
            Some("exact_request") => &["domain", "input", "context", "executable"],
            Some("argv") => &[
                "domain",
                "cwd",
                "environment",
                "executable",
                "arguments",
                "possible_effects",
            ],
            Some("shell") => &[
                "domain",
                "cwd",
                "environment",
                "source",
                "dialect",
                "dialect_version",
            ],
            Some("file") => &[
                "domain",
                "cwd",
                "environment",
                "action",
                "sources",
                "destination",
                "recursive",
            ],
            Some("web") => &[
                "domain",
                "cwd",
                "environment",
                "action",
                "url",
                "query",
                "method",
                "conditions",
            ],
            Some("mcp") => &[
                "domain",
                "cwd",
                "environment",
                "server",
                "tool",
                "arguments",
            ],
            _ => return Err(Error::Contract("unsupported historical rule domain")),
        };
        fields(c, allowed)?;
        if c["domain"] == "exact_request" {
            validate_exact(c)?;
        } else {
            if let Some(cwd) = c.get("cwd").filter(|v| !v.is_null()) {
                path_selector(cwd)?;
            }
            if let Some(env) = c.get("environment").filter(|v| !v.is_null()) {
                serde_json::from_value::<BTreeMap<String, String>>(env.clone())
                    .map_err(|_| Error::Contract("invalid environment constraint"))?;
            }
            validate_domain(c)?;
        }
        let mut value = json!(self);
        if crate::evidence::redact(&mut value) {
            return Err(Error::Contract(
                "credentials cannot be stored in project rules",
            ));
        }
        Ok(())
    }

    pub(in crate::policy) fn matching(
        &self,
        request: &crate::policy::Request,
        facts: &rules::Facts<'_>,
        legacy_read: bool,
    ) -> Option<u8> {
        if matches!(facts.input, Input::Argv { .. }) && (!facts.known_cwd || !facts.known_arguments)
        {
            return None;
        }
        if self
            .expires_at
            .as_ref()
            .is_some_and(|s| facts.now.is_empty() || s.as_str() <= facts.now)
        {
            return None;
        }
        let c = &self.conditions;
        if legacy_read && (c["domain"] != "argv" || c.get("environment").is_some()) {
            return None;
        }
        let exact = c["domain"] == "exact_request";
        if !exact {
            if c.get("cwd")
                .filter(|v| !v.is_null())
                .is_some_and(|v| !path_matches(v, &facts.context.cwd))
            {
                return None;
            }
            if let Some(env) = c.get("environment")
                && (*env != json!(facts.context.environment)
                    || (env.is_null() && self.decision == Decision::Allow))
            {
                return None;
            }
        }
        let matched = if exact {
            // Resolved connection settings are evaluator evidence, not part of
            // the submitted identity retained by historical exact rules.
            let mut submitted = json!(facts.input);
            if matches!(facts.input, Input::Mcp { .. })
                && let Some(fields) = submitted.as_object_mut()
            {
                fields.remove("connection");
            }
            serde_json::from_value::<Input>(c["input"].clone())
                .is_ok_and(|input| json!(input) == submitted)
                && serde_json::from_value::<Context>(c["context"].clone())
                    .is_ok_and(|context| json!(context) == json!(facts.context))
                && c["executable"] == json!(facts.executable)
                && (self.decision != Decision::Allow || facts.context.environment.is_some())
        } else {
            domain_matches(c, facts.input) && effect_matches(c, request)
        };
        matched.then(|| {
            if exact {
                3
            } else if c["domain"] == "argv" && c["arguments"]["kind"] == "exact" {
                if c["cwd"].is_null() { 2 } else { 3 }
            } else {
                1
            }
        })
    }
}

pub(in crate::policy) fn validate_normalized_authority(
    rule: &rules::Rule,
    pack: bool,
) -> Result<(), Error> {
    let Some(authority) = &rule.legacy_normalized else {
        return Ok(());
    };
    if pack || rule.legacy.is_some() {
        return Err(Error::Contract(
            "historical normalized rules are project-only",
        ));
    }
    authority.validate()?;
    let expected = authority.translate()?;
    if serde_json::to_value(rule).map_err(|_| Error::Syntax)?
        != serde_json::to_value(expected).map_err(|_| Error::Syntax)?
    {
        return Err(Error::Contract(
            "historical normalized rule must preserve its exact source",
        ));
    }
    Ok(())
}

fn fields(value: &Value, allowed: &[&str]) -> Result<(), Error> {
    if value
        .as_object()
        .is_none_or(|v| v.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        Err(Error::Contract("unknown historical condition fields"))
    } else {
        Ok(())
    }
}

fn canonical_path(value: &str) -> bool {
    std::path::Path::new(value).is_absolute()
        && !value.contains('\0')
        && !value.split('/').any(|p| p == "." || p == "..")
}

fn path_selector(value: &Value) -> Result<(), Error> {
    fields(value, &["kind", "path"])?;
    if !matches!(value["kind"].as_str(), Some("exact" | "within"))
        || value["path"].as_str().is_none_or(|p| !canonical_path(p))
    {
        return Err(Error::Contract("invalid path selector"));
    }
    Ok(())
}

fn path_matches(selector: &Value, value: &str) -> bool {
    selector["path"].as_str().is_some_and(|p| {
        if selector["kind"] == "within" {
            std::path::Path::new(value).starts_with(p)
        } else {
            value == p
        }
    })
}

fn validate_domain(c: &Value) -> Result<(), Error> {
    match c["domain"].as_str() {
        Some("argv") => {
            if c["executable"].as_str().is_none_or(str::is_empty) {
                return Err(Error::Contract("executable required"));
            }
            fields(&c["arguments"], &["kind", "values"])?;
            if !matches!(
                c["arguments"]["kind"].as_str(),
                Some("exact" | "prefix" | "any")
            ) || (c["arguments"]["kind"] != "any"
                && serde_json::from_value::<Vec<String>>(c["arguments"]["values"].clone()).is_err())
            {
                return Err(Error::Contract("invalid arguments selector"));
            }
            if let Some(effects) = c.get("possible_effects") {
                let effects: Vec<crate::effects::Effect> = serde_json::from_value(effects.clone())
                    .map_err(|_| Error::Contract("unsupported effect predicate"))?;
                if effects.is_empty()
                    || effects.len() > 11
                    || effects.iter().collect::<BTreeSet<_>>().len() != effects.len()
                {
                    return Err(Error::Contract("effects must be nonempty and unique"));
                }
            }
        }
        Some("shell")
            if c["source"].is_string()
                && matches!(
                    c["dialect"].as_str(),
                    Some("bash" | "posix" | "powershell" | "cmd")
                ) => {}
        Some("file") => {
            if !matches!(
                c["action"].as_str(),
                Some("read" | "write" | "copy" | "move" | "remove")
            ) {
                return Err(Error::Contract("invalid file action"));
            }
            for selector in c["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(c.get("destination").filter(|v| !v.is_null()))
            {
                path_selector(selector)?;
            }
        }
        Some("web") if matches!(c["action"].as_str(), Some("fetch" | "search")) => {}
        Some("mcp") if c["server"].is_string() && c["tool"].is_string() => {}
        _ => return Err(Error::Contract("invalid historical condition")),
    }
    Ok(())
}

fn validate_exact(c: &Value) -> Result<(), Error> {
    let input: Input = serde_json::from_value(c["input"].clone())
        .map_err(|_| Error::Contract("invalid exact input"))?;
    let context: Context = serde_json::from_value(c["context"].clone())
        .map_err(|_| Error::Contract("invalid exact context"))?;
    // Current evidence-only MCP settings never belonged to a historical input.
    if c["input"].get("connection").is_some() {
        return Err(Error::Contract("invalid exact input"));
    }
    validate_input(&input, &context)?;
    if !canonical_path(&context.cwd) {
        return Err(Error::Contract("exact cwd must be canonical"));
    }
    if !c
        .get("executable")
        .is_some_and(|p| p.is_null() || p.as_str().is_some_and(canonical_path))
        || (matches!(input, Input::Argv { .. } | Input::Shell { .. }) && c["executable"].is_null())
    {
        return Err(Error::Contract(
            "exact launch requires canonical executable",
        ));
    }
    Ok(())
}

fn validate_input(input: &Input, context: &Context) -> Result<(), Error> {
    if !json!(input)["original_input"].is_object() || crate::json::contains_nul(&json!(input)) {
        return Err(Error::Contract("invalid original input"));
    }
    crate::policy::Request {
        executable: "context".into(),
        arguments: vec![],
        cwd: context.cwd.clone(),
        project: context.cwd.clone(),
    }
    .validate()?;
    match input {
        Input::Argv {
            executable,
            arguments,
            ..
        } => crate::policy::Request {
            executable: executable.clone(),
            arguments: arguments.clone(),
            cwd: context.cwd.clone(),
            project: context.cwd.clone(),
        }
        .validate(),
        Input::Shell {
            source, dialect, ..
        } if source.len() <= 65536
            && ["bash", "posix", "powershell", "cmd"].contains(&dialect.as_str()) =>
        {
            Ok(())
        }
        Input::File {
            action,
            sources,
            destination,
            ..
        } if ["read", "write", "copy", "move", "remove"].contains(&action.as_str())
            && sources.len() <= 256
            && (action == "write" || !sources.is_empty())
            && (!["write", "copy", "move"].contains(&action.as_str()) || destination.is_some()) =>
        {
            Ok(())
        }
        Input::Web {
            action,
            query,
            url,
            conditions,
            ..
        } if conditions.is_object()
            && ((action == "fetch" && url.is_some())
                || (action == "search" && query.is_some())) =>
        {
            Ok(())
        }
        Input::Mcp {
            server,
            tool,
            arguments,
            ..
        } if !server.is_empty() && !tool.is_empty() && arguments.is_object() => Ok(()),
        Input::Unknown { host_tool, .. } if !host_tool.is_empty() => Ok(()),
        _ => Err(Error::Contract("invalid normalized input")),
    }
}

fn timestamp(value: &str) -> bool {
    let b = value.as_bytes();
    if value.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return false;
    }
    let number = |a, b| {
        value
            .get(a..b)
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u32>().ok())
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        number(0, 4),
        number(5, 7),
        number(8, 10),
        number(11, 13),
        number(14, 16),
        number(17, 19),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    year > 0 && day > 0 && day <= days && hour < 24 && minute < 60 && second < 60
}

fn effect_matches(c: &Value, request: &crate::policy::Request) -> bool {
    let request = crate::policy::Request {
        project: request.cwd.clone(),
        ..request.clone()
    };
    c.get("possible_effects").is_none_or(|effects| {
        serde_json::from_value::<Vec<crate::effects::Effect>>(effects.clone()).is_ok_and(
            |effects| {
                crate::effects::classify(&request)
                    .is_some_and(|hints| effects.iter().all(|effect| hints.may(*effect)))
            },
        )
    })
}

fn domain_matches(c: &Value, input: &Input) -> bool {
    match input {
        Input::Argv {
            executable,
            arguments,
            ..
        } if c["domain"] == "argv" => {
            c["executable"] == *executable
                && match c["arguments"]["kind"].as_str() {
                    Some("any") => true,
                    Some("exact") => c["arguments"]["values"] == json!(arguments),
                    Some("prefix") => {
                        serde_json::from_value::<Vec<String>>(c["arguments"]["values"].clone())
                            .is_ok_and(|v| arguments.starts_with(&v))
                    }
                    _ => false,
                }
        }
        Input::Shell {
            source,
            dialect,
            dialect_version,
            ..
        } if c["domain"] == "shell" => {
            c["source"] == *source
                && c["dialect"] == *dialect
                && c["dialect_version"] == json!(dialect_version)
        }
        Input::Mcp {
            server,
            tool,
            arguments,
            ..
        } if c["domain"] == "mcp" => {
            c["server"] == *server
                && c["tool"] == *tool
                && c.get("arguments").is_none_or(|v| v == arguments)
        }
        // The original non-exact matcher never granted file or web effects.
        _ => false,
    }
}
