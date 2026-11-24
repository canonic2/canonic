//! Structured rule source, requirement branches and immutable preparation, with
//! the predicates they use: argv grammars, patterns, selectors, resource
//! evidence, directory options, portability and agent outputs.
use super::{
    Decision, Error, Match, Profile,
    model::Matcher,
    resolution::{
        diagnostics::{RuleReference, TextField},
        normalized::{Context, Input},
    },
};
use patterns::Provider;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) mod agent_outputs;
pub(super) mod argument_rules;
pub(crate) mod directories;
pub(crate) mod patterns;
pub(crate) mod portability;
pub(crate) mod resources;
pub(crate) mod selectors;

mod evidence;
mod preconditions;
mod predicates;
#[cfg(test)]
mod tests;
mod validation;
fn timestamp(value: &str) -> Option<canonic_registry_kit::timestamp::Parts> {
    canonic_registry_kit::timestamp::parse(value).filter(|parts| parts.0 > 0)
}
pub(crate) use preconditions::{MAX_ALTERNATIVES, Precondition};
use predicates::{Failures, checks, domain_matches};

/// Options grammars that pass over options they do not declare.
pub(crate) const UNKNOWN_OPTIONS: &str = "unknown-options-v1";
/// Option phases that accept unique prefixes of their long options.
pub(crate) const OPTION_ABBREVIATIONS: &str = "option-abbreviations-v1";
/// Text selectors of the form `{not: SELECTOR}`.
pub(crate) const NEGATED_SELECTORS: &str = "negated-selectors-v1";
/// Leading option phases also parsed between prefix words.
pub(crate) const INTERLEAVED_LEADING: &str = "interleaved-leading-v1";
pub(crate) const FIELD_NEGATION: &str = "field-negation-v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum Decisions {
    Scalar(Decision),
    Profiles(Profiles),
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profiles {
    loose: Decision,
    recommended: Decision,
    strict: Decision,
}
impl Decisions {
    pub(crate) fn selected(&self, profile: Profile) -> Decision {
        match self {
            Self::Scalar(v) => *v,
            Self::Profiles(p) => match profile {
                Profile::Loose => p.loose,
                Profile::Recommended => p.recommended,
                Profile::Strict => p.strict,
            },
        }
    }
    pub(crate) fn validate(&self, pack: bool, unmet: bool) -> Result<(), Error> {
        if matches!(self, Self::Profiles(_)) && !pack {
            return Err(Error::Contract("custom decision must be scalar"));
        }
        let [a, b, c] =
            [Profile::Loose, Profile::Recommended, Profile::Strict].map(|p| self.selected(p));
        if a > b || b > c || (unmet && a == Decision::Allow) {
            return Err(Error::Contract(
                "invalid decision profile ordering or unmet allowance",
            ));
        }
        Ok(())
    }
    fn allows(&self) -> bool {
        self.selected(Profile::Loose) == Decision::Allow
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Guidance {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) message: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) alternatives: Option<Vec<String>>,
}
impl Guidance {
    fn validate(&self) -> Result<(), Error> {
        if self.message.is_none() && self.alternatives.is_none() {
            return Err(Error::Contract("empty guidance"));
        }
        if self
            .message
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 512)
            || self.alternatives.as_ref().is_some_and(|v| {
                v.is_empty() || v.len() > 3 || v.iter().any(|s| s.is_empty() || s.len() > 256)
            })
        {
            return Err(Error::Contract("invalid guidance bounds"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Unmet {
    pub(crate) decision: Decisions,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) message: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) alternatives: Option<Vec<String>>,
}
impl Unmet {
    fn guidance(&self) -> Option<Guidance> {
        (self.message.is_some() || self.alternatives.is_some()).then(|| Guidance {
            message: self.message.clone(),
            alternatives: self.alternatives.clone(),
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    pub(crate) id: String,
    /// Lossless historical argv matching, with no additional authority tiers.
    #[serde(
        default,
        rename = "historical_matcher",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) legacy: Option<super::legacy::Authority>,
    #[serde(
        default,
        rename = "historical_normalized",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) legacy_normalized: Option<super::legacy::NormalizedAuthority>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) invocation: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) command_set: Option<String>,
    pub(crate) decision: Decisions,
    pub(crate) conditions: Value,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) requirements: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) on_unmet: Option<Unmet>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) reason: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) guidance: Option<Guidance>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) expires_at: Option<String>,
    /// A pack rule that applies only when no other pack decides with a regular rule.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) fallback: Option<bool>,
    /// Pack-only activation tests, removed when variables resolve.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) when: Option<Vec<super::variables::Test>>,
    /// Agent-specific outputs, such as a Codex prefix rule.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) agents: Option<agent_outputs::Agents>,
    #[serde(skip)]
    pub(super) matcher: Option<Matcher>,
}
pub(super) fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

impl Rule {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.validate_for(false)
    }
    pub(super) fn validate_for(&self, pack: bool) -> Result<(), Error> {
        super::legacy::validate_authority(self, pack)?;
        super::legacy::validate_normalized_authority(self, pack)?;
        if self.legacy.is_some() || self.legacy_normalized.is_some() {
            return Ok(());
        }
        if pack
            && self.conditions["domain"] == "exact_request"
            && self.conditions["input"]["kind"] == "mcp"
        {
            return Err(Error::Contract(
                "pack MCP rules require integration-scoped mcp conditions",
            ));
        }
        if !pack && (self.invocation.is_some() || self.command_set.is_some()) {
            return Err(Error::Contract("shared invocation bindings are pack-only"));
        }
        if !valid_id(&self.id) {
            return Err(Error::Contract("invalid rule identity"));
        }
        if self.when.is_some() {
            return Err(Error::Contract("when is resolved from pack variables"));
        }
        if crate::json::encoded_len(self).map_err(|_| Error::Syntax)? > 2 * 1024 * 1024 {
            return Err(Error::Limit);
        }
        self.decision.validate(pack, false)?;
        if self
            .reason
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 4096)
        {
            return Err(Error::Contract("invalid rule reason"));
        }
        if let Some(guidance) = &self.guidance {
            guidance.validate()?;
        }
        if self.requirements.is_some() != self.on_unmet.is_some() {
            return Err(Error::Contract("requirements require on_unmet"));
        }
        if let Some(unmet) = &self.on_unmet {
            unmet.decision.validate(pack, true)?;
            if let Some(guidance) = unmet.guidance() {
                guidance.validate()?;
            }
        }
        if let Some(expiry) = &self.expires_at
            && (pack || timestamp(expiry).is_none())
        {
            return Err(Error::Contract("invalid custom expiration"));
        }
        if self.fallback.is_some_and(|fallback| !fallback || !pack) {
            return Err(Error::Contract("fallback is true on pack rules only"));
        }
        self.validate_conditions()?;
        if let Some(agents) = &self.agents {
            agents.validate(self)?;
        }
        let mut value = json!(self);
        if crate::json::contains_nul(&value) || crate::evidence::redact(&mut value) {
            return Err(Error::Contract("NUL or credentials in rule"));
        }
        Ok(())
    }

    fn validate_conditions(&self) -> Result<(), Error> {
        let domain = self.conditions["domain"]
            .as_str()
            .ok_or(Error::Contract("domain required"))?;
        if domain == "exact_request" {
            selectors::fields(
                &self.conditions,
                &["domain", "input", "context", "executable"],
            )?;
            if self.requirements.is_some() {
                return Err(Error::Contract("exact request has no requirements"));
            }
            let input: Input = serde_json::from_value(self.conditions["input"].clone())
                .map_err(|_| Error::Syntax)?;
            let context: Context = serde_json::from_value(self.conditions["context"].clone())
                .map_err(|_| Error::Syntax)?;
            input.validate(&context, &context.cwd)?;
            if json!(input) != self.conditions["input"]
                || json!(context) != self.conditions["context"]
            {
                return Err(Error::Contract(
                    "exact binding requires complete normalized input and context",
                ));
            }
            if !canonical(&context.cwd)
                || !self
                    .conditions
                    .get("executable")
                    .is_some_and(|p| p.is_null() || p.as_str().is_some_and(canonical))
            {
                return Err(Error::Contract("exact context must be canonical"));
            }
        } else {
            validation::validate_predicates(
                &self.conditions,
                domain,
                false,
                self.decision.allows(),
            )?;
            if let Some(requirements) = &self.requirements {
                if requirements
                    .as_object()
                    .is_none_or(serde_json::Map::is_empty)
                {
                    return Err(Error::Contract("requirements must be nonempty"));
                }
                validation::validate_predicates(
                    requirements,
                    domain,
                    true,
                    self.decision.allows(),
                )?;
            }
            let has = |key| {
                self.conditions.get(key).is_some()
                    || self
                        .requirements
                        .as_ref()
                        .is_some_and(|r| r.get(key).is_some())
            };
            if domain == "argv" && !has("arguments") {
                return Err(Error::Contract("argument selector required"));
            }
            if domain == "web"
                && ((self.conditions["action"] == "search" && !has("query"))
                    || (self.conditions["action"] == "fetch" && !has("url"))
                    || (self.conditions["action"] == "request" && (!has("url") || !has("method"))))
            {
                return Err(Error::Contract("web action selector required"));
            }
            if domain == "web" && !self.conditions["action"].is_string() {
                return Err(Error::Contract("web action must be literal"));
            }
            if self.decision.allows() && accepts_hostname_resolution(&self.conditions) {
                validation::validate_hostname_request(&self.conditions)?;
            }
            if domain == "shell" && self.decision.allows() {
                let exact = |field: &str| {
                    self.conditions[field].get("exact").is_some()
                        || self
                            .requirements
                            .as_ref()
                            .is_some_and(|r| r[field].get("exact").is_some())
                };
                if !exact("cwd") || !exact("environment") {
                    return Err(Error::Contract(
                        "whole-shell Allow requires complete exact context",
                    ));
                }
            }
            if domain == "file" && self.decision.allows() {
                self.validate_file_coverage()?;
            }
        }
        Ok(())
    }

    fn validate_file_coverage(&self) -> Result<(), Error> {
        let predicates = [
            &self.conditions,
            self.requirements.as_ref().unwrap_or(&Value::Null),
        ];
        let source = predicates.iter().any(|p| p["sources"].get("all").is_some());
        let destination = predicates.iter().any(|p| p.get("destination").is_some());
        let covers = |action: &str| {
            predicates.iter().any(|p| {
                let targets = &p["targets"];
                std::iter::once(targets)
                    .chain(targets.as_array().into_iter().flatten())
                    .any(|t| t["action"] == action && t.get("all").is_some())
            })
        };
        let action = &self.conditions["action"];
        let actions = action.as_str().map_or_else(
            || {
                action["one_of"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
            },
            |a| vec![a],
        );
        for action in actions {
            let sources = match action {
                "read" | "copy" => source || covers("read"),
                "remove" | "move" => source || covers("remove"),
                _ => true,
            };
            let destinations =
                !["write", "copy", "move"].contains(&action) || destination || covers("write");
            let replacements = action != "move" || destination || covers("remove");
            if !sources || !destinations || !replacements {
                return Err(Error::Contract(
                    "file Allow must cover every affected resource role",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn prepare(&mut self) {
        self.matcher = if self.legacy_normalized.is_some() {
            // Literal normalized rules have different effect and environment
            // semantics; they cannot use the matcher comparison projection.
            None
        } else {
            self.legacy
                .as_ref()
                .map(|legacy| legacy.matcher.clone())
                .or_else(|| self.projection())
        };
    }
    fn projection(&self) -> Option<Matcher> {
        if self.invocation.is_some()
            || self.command_set.is_some()
            || self.conditions["domain"] != "argv"
            || self.requirements.is_some()
            || self.expires_at.is_some()
            // Comparisons assume most-restrictive composition across packs.
            || self.fallback.is_some()
            || self.conditions.get("environment").is_some()
            || self.conditions.get("targets").is_some()
            || self.conditions.get("argument_paths").is_some()
            || self.conditions.get("graphql").is_some()
            || self.conditions.get("cwd").is_some()
        {
            return None;
        }
        self.comparison_example()
    }
    /// A written argv shape for incomplete witness search only. Requirements,
    /// activation and resource facts still come from full policy evaluation;
    /// this must never be used to declare a complete matching partition.
    pub(super) fn comparison_example(&self) -> Option<Matcher> {
        if self.conditions["domain"] != "argv" {
            return None;
        }
        let executable = selectors::exact(&self.conditions["executable"])?;
        let arguments = &self.conditions["arguments"];
        if !matches!(arguments["kind"].as_str(), Some("exact" | "prefix" | "any"))
            || arguments.get("leading").is_some()
            || arguments["values"]
                .as_array()
                .is_some_and(|v| v.iter().any(|s| !s.is_string()))
        {
            return None;
        }
        let mut projection = json!({"executable":executable,"arguments":arguments});
        if let Some(effects) = self.conditions.get("possible_effects") {
            projection["possible_effects"] = effects.clone();
        }
        let matcher: Matcher = serde_json::from_value(projection).ok()?;
        matcher.validate().ok()?;
        Some(matcher)
    }
    pub(crate) fn semantic(&self) -> bool {
        self.conditions.get("possible_effects").is_some()
            || self
                .requirements
                .as_ref()
                .is_some_and(|r| r.get("possible_effects").is_some())
    }
    pub(super) fn executable(&self) -> Option<&str> {
        if self.conditions["domain"] == "argv" {
            selectors::exact(&self.conditions["executable"])
        } else {
            None
        }
    }
    pub(super) fn capabilities(&self, result: &mut BTreeSet<String>) {
        if self.legacy.is_some() {
            result.insert("historical-matchers-v1".into());
        }
        if self.legacy_normalized.is_some() {
            result.insert("historical-normalized-v1".into());
        }
        if self.invocation.is_some() {
            result.insert("invocation-groups-v1".into());
        }
        if self.command_set.is_some() {
            result.insert("command-sets-v1".into());
        }
        let domain = self.conditions["domain"].as_str().unwrap_or_default();
        if domain == "argv" {
            result.insert("argument-rules-v1".into());
        }
        if domain == "file" {
            result.insert("location-v1".into());
        }
        if self.requirements.is_some() {
            result.insert("requirements-v1".into());
        }
        if self.fallback.is_some() {
            result.insert("fallback-rules-v1".into());
        }
        if self.agents.is_some() {
            result.insert("agent-outputs-v1".into());
        }
        for value in std::iter::once(&self.conditions).chain(self.requirements.as_ref()) {
            if value["url"].get("subdomains").is_some() {
                result.insert("url-subdomains-v1".into());
            }
            capabilities(value, domain, result);
        }
        if accepts_hostname_resolution(&self.conditions) {
            result.insert(crate::http_client::local::HOSTNAME_RESOLUTION_CAPABILITY.into());
        }
    }
}

fn accepts_hostname_resolution(conditions: &Value) -> bool {
    use crate::http_client::local::{HOSTNAME_RESOLUTION, SYSTEM_RESOLUTION};
    conditions["domain"] == "web"
        && conditions["action"] == "request"
        && conditions["request_fields"][HOSTNAME_RESOLUTION]["exact"] == SYSTEM_RESOLUTION
}

pub(crate) fn document(rows: &[Value]) -> Result<Value, Error> {
    document_with(rows, &[])
}

/// A custom rules document holding `rows` and the directory option
/// declarations of `directory_options`, with the capabilities they use.
pub(crate) fn document_with(rows: &[Value], directory_options: &[Value]) -> Result<Value, Error> {
    let mut capabilities = BTreeSet::from(["structured-rules-v1".to_owned()]);
    for row in rows {
        let rule: Rule = serde_json::from_value(row.clone()).map_err(|_| Error::Syntax)?;
        rule.validate()?;
        rule.capabilities(&mut capabilities);
    }
    let mut document =
        json!({"schema_version":1,"kind":"custom","capabilities":capabilities,"rules":rows});
    if !directory_options.is_empty() {
        let declarations: Vec<super::directories::DirectoryOption> =
            serde_json::from_value(json!(directory_options)).map_err(|_| Error::Syntax)?;
        super::directories::validate(&declarations)?;
        capabilities.insert(super::model::DIRECTORY_OPTIONS.into());
        document["capabilities"] = json!(capabilities);
        document["directory_options"] = json!(directory_options);
    }
    Ok(document)
}

pub(super) fn capabilities(value: &Value, domain: &str, result: &mut BTreeSet<String>) {
    match value {
        Value::Object(fields) => {
            if value["kind"] == "prefix" && value.get("leading").is_some() {
                result.insert("prefix-leading-v1".into());
            }
            if matches!(value["kind"].as_str(), Some("prefix" | "options"))
                && value["leading"]["interleaved"] == true
            {
                result.insert(INTERLEAVED_LEADING.into());
            }
            if value["kind"] == "options" && value["unknown_options"] == "skip" {
                result.insert(UNKNOWN_OPTIONS.into());
            }
            if value["long_abbreviations"] == true {
                result.insert(OPTION_ABBREVIATIONS.into());
            }
            for (key, v) in fields {
                if key == "graphql" {
                    result.insert("graphql-v1".into());
                    if v["operation"] == "mutation" {
                        result.insert("graphql-mutations-v1".into());
                    }
                    if crate::graphql::has_subtree(v) {
                        result.insert("graphql-subtree-v1".into());
                    }
                    for selector in crate::graphql::selectors(v) {
                        capabilities(selector, domain, result);
                    }
                    continue;
                }
                if [
                    "cwd",
                    "sources",
                    "destination",
                    "targets",
                    "argument_paths",
                    "field_paths",
                ]
                .contains(&key.as_str())
                    && access_reference(v)
                {
                    result.insert("access-paths-v1".into());
                }
                if key == "argument_paths" || key == "field_paths" {
                    result.insert("location-v1".into());
                }
                if key == "field_paths" && any_child_has_key(v, "each") {
                    result.insert("field-path-arrays-v1".into());
                }
                if ["fields", "request_fields", "connection"].contains(&key.as_str())
                    && any_child_has_key(v, "not")
                {
                    result.insert(FIELD_NEGATION.into());
                }
                if ["exact", "input", "context"].contains(&key.as_str()) {
                    continue;
                }
                if key == "environment" {
                    for selector in selectors::environment_selectors(v) {
                        capabilities(selector, domain, result);
                    }
                    continue;
                }
                let capability = match key.as_str() {
                    "action" if domain == "web" && v == "request" => Some("local-http-requests-v1"),
                    "connection" if domain == "mcp" => Some("mcp-connection-facts-v1"),
                    "argument_paths" => Some("argument-paths-v1"),
                    "docker_isolated_bind" => Some("docker-isolated-bind-v1"),
                    "field_paths" => Some("field-paths-v1"),
                    "glob" => Some("glob-v1"),
                    "regex" => Some("regex-v1"),
                    "not" => Some(NEGATED_SELECTORS),
                    "root" | "cwd" | "sources" | "destination" | "targets" | "within"
                    | "outside" => Some("location-v1"),
                    "possible_effects" => Some("possible-effects-v1"),
                    _ => None,
                };
                if let Some(capability) = capability {
                    result.insert(capability.into());
                }
                if key == "root" && matches!(v.as_str(), Some("read" | "write")) {
                    result.insert("access-paths-v1".into());
                }
                if !(domain == "mcp" && key == "arguments") {
                    capabilities(v, domain, result);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                capabilities(item, domain, result);
            }
        }
        _ => {}
    }
}

fn any_child_has_key(value: &Value, key: &str) -> bool {
    value
        .as_object()
        .is_some_and(|fields| fields.values().any(|child| child.get(key).is_some()))
}

fn access_reference(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            matches!(
                fields.get("root").and_then(Value::as_str),
                Some("read" | "write")
            ) || fields.values().any(access_reference)
        }
        Value::Array(items) => items.iter().any(access_reference),
        _ => false,
    }
}

fn canonical(value: &str) -> bool {
    std::path::Path::new(value).is_absolute()
        && !value.contains('\0')
        && !value.split('/').any(|p| p == "." || p == "..")
}
#[derive(Clone, Debug)]
pub(crate) struct Check {
    pub(crate) rule_ref: RuleReference,
    pub(crate) operation_ref: String,
    pub(crate) predicate_path: String,
    pub(crate) status: &'static str,
    pub(crate) code: &'static str,
    pub(crate) branch: &'static str,
    pub(crate) applied: bool,
    pub(crate) evidence: Option<Value>,
    pub(crate) private_input: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Facts<'a> {
    pub(crate) input: &'a Input,
    pub(crate) context: &'a Context,
    pub(crate) executable: Option<&'a str>,
    pub(crate) now: &'a str,
    pub(crate) known_cwd: bool,
    pub(crate) known_arguments: bool,
    pub(crate) locations: Option<&'a dyn super::resources::Resources>,
    pub(crate) container_read: Option<&'a dyn Fn(&str) -> bool>,
    pub(crate) effects: Option<&'a crate::effects::Hints>,
}

/// Historical SDK reads used this exact argv identity. Only the matching
/// historical rule sees argv; current file rules retain the original facts.
pub(super) fn legacy_read(request: &super::Request, facts: &Facts<'_>) -> bool {
    request.executable == "shield:read-text"
        && request.cwd == facts.context.cwd
        && matches!(facts.input, Input::File { action, sources, destination: None, recursive: false, .. }
            if action == "read" && sources.len() == 1 && request.arguments.first() == sources.first())
}

impl Rule {
    fn exact_matches(&self, facts: &Facts<'_>, profile: Profile) -> Result<bool, Error> {
        if self.decision.selected(profile) == Decision::Allow
            && (!facts.known_cwd || !facts.known_arguments)
        {
            return Ok(false);
        }
        // Connection facts are evaluator evidence, not submitted input.
        let mut submitted = json!(facts.input);
        if matches!(facts.input, Input::Mcp { .. }) {
            submitted
                .as_object_mut()
                .ok_or(Error::Syntax)?
                .remove("connection");
        }
        Ok(self.conditions["input"] == submitted
            && self.conditions["context"] == json!(facts.context)
            && self.conditions["executable"] == json!(facts.executable))
    }
    pub(super) fn matching(
        &self,
        request: &super::Request,
        facts: &Facts<'_>,
        patterns: &dyn Provider,
        source: &str,
        profile: Profile,
        tier: u8,
    ) -> Result<Option<Match>, Error> {
        if matches!(facts.input, Input::Web {action, conditions, ..}
            if action == "request" && crate::http_client::local::uses_hostname_resolution(conditions))
            && self.decision.selected(profile) == Decision::Allow
            && self.conditions["domain"] != "exact_request"
            && !accepts_hostname_resolution(&self.conditions)
        {
            return Ok(None);
        }
        let original;
        let bound;
        let historical_read = legacy_read(request, facts);
        let facts =
            if (self.legacy.is_some() || self.legacy_normalized.is_some()) && historical_read {
                original = Input::Argv {
                    executable: request.executable.clone(),
                    arguments: request.arguments.clone(),
                    original_input: json!({}),
                };
                bound = Facts {
                    input: &original,
                    ..*facts
                };
                &bound
            } else {
                facts
            };
        if let Some(legacy) = &self.legacy_normalized {
            if !facts.known_cwd || !facts.known_arguments {
                return Ok(None);
            }
            return Ok(legacy
                .matching(request, facts, historical_read)
                .map(|specificity| Match {
                    source: source.into(),
                    rule_id: self.id.clone(),
                    decision: self.decision.selected(profile),
                    reason: self.reason.clone().unwrap_or_default(),
                    specificity,
                    applied: false,
                    yielded_reason: None,
                    tier,
                    branch: "met",
                    requirement_checks: Vec::new(),
                    guidance: None,
                    exact_request: self.conditions["domain"] == "exact_request",
                    legacy: true,
                    fallback: false,
                    precondition: false,
                    decision_provenance: None,
                    operation_ref: None,
                    executable: TextField::Absent,
                    rule_ref: None,
                    private_input: false,
                }));
        }
        if let Some(legacy) = &self.legacy
            && (!matches!(facts.input, Input::Argv { .. })
                || !facts.known_cwd
                || !facts.known_arguments
                || !legacy.matcher.matches_evidence(request, facts.effects))
        {
            return Ok(None);
        }
        self.matching_current(facts, patterns, source, profile, tier)
    }

    fn matching_current(
        &self,
        facts: &Facts<'_>,
        patterns: &dyn Provider,
        source: &str,
        profile: Profile,
        tier: u8,
    ) -> Result<Option<Match>, Error> {
        if let Some(expiry) = &self.expires_at {
            let now = timestamp(facts.now).ok_or(Error::Contract("current clock unavailable"))?;
            if timestamp(expiry).ok_or(Error::Syntax)? <= now {
                return Ok(None);
            }
        }
        if self.conditions["domain"] == "exact_request" {
            if !self.exact_matches(facts, profile)? {
                return Ok(None);
            }
        } else if !domain_matches(&self.conditions, facts.input, patterns)?
            || !checks(&self.conditions, facts, patterns)?.is_empty()
        {
            return Ok(None);
        }
        let mut failures = self
            .requirements
            .as_ref()
            .map_or(Ok(vec![]), |r| checks(r, facts, patterns))?;
        if !facts.known_arguments
            && self
                .requirements
                .as_ref()
                .is_some_and(|r| r.get("arguments").is_some())
        {
            failures.push(("arguments".into(), "evidence_unavailable"));
        }
        if self.decision.allows()
            && self.conditions["domain"] == "argv"
            && (self.conditions.get("targets").is_some()
                || self
                    .requirements
                    .as_ref()
                    .is_some_and(|r| r.get("targets").is_some()))
        {
            let predicates = [
                &self.conditions,
                self.requirements.as_ref().unwrap_or(&Value::Null),
            ];
            if !facts.known_arguments || facts.locations.is_none_or(|l| !l.coverage(&predicates)) {
                if self.requirements.is_none() {
                    return Ok(None);
                }
                failures.push(("targets".into(), "evidence_unavailable"));
            }
        }
        let met = failures.is_empty();
        let branch = if met { "met" } else { "unmet" };
        let decision = if met {
            self.decision.selected(profile)
        } else {
            self.on_unmet
                .as_ref()
                .ok_or(Error::Syntax)?
                .decision
                .selected(profile)
        };
        if decision == Decision::Allow && !facts.known_arguments {
            return Ok(None);
        }
        let guidance = if met {
            self.guidance.clone()
        } else {
            self.on_unmet.as_ref().and_then(Unmet::guidance)
        };
        let rule_ref = RuleReference::for_rule(source, &self.id, tier);
        let requirement_checks =
            self.requirement_evidence(facts, patterns, &failures, &rule_ref, branch)?;
        let rank = if met {
            self.legacy.as_ref().map_or_else(
                || self.rank(),
                |legacy| legacy.matcher.historical_specificity(),
            )
        } else {
            rank(&self.conditions).max(2)
        };
        Ok(Some(Match {
            source: source.into(),
            rule_id: self.id.clone(),
            decision,
            reason: self.reason.clone().unwrap_or_default(),
            specificity: rank,
            applied: false,
            yielded_reason: None,
            tier,
            legacy: self.legacy.is_some(),
            branch,
            requirement_checks,
            guidance,
            exact_request: self.conditions["domain"] == "exact_request",
            fallback: self.fallback == Some(true),
            precondition: false,
            decision_provenance: None,
            operation_ref: None,
            executable: TextField::Absent,
            rule_ref: None,
            private_input: false,
        }))
    }
    fn requirement_evidence(
        &self,
        facts: &Facts<'_>,
        patterns: &dyn Provider,
        failures: &Failures,
        rule_ref: &RuleReference,
        branch: &'static str,
    ) -> Result<Vec<Check>, Error> {
        let mut requirement_checks = Vec::new();
        if let Some(requirements) = &self.requirements {
            let keys: BTreeSet<_> = requirements
                .as_object()
                .ok_or(Error::Syntax)?
                .keys()
                .chain(failures.iter().map(|(key, _)| key))
                .collect();
            for key in keys {
                let path = format!(
                    "/requirements/{}",
                    key.replace('~', "~0").replace('/', "~1")
                );
                let path = if requirements.get(key).is_some() {
                    path
                } else {
                    "/requirements".into()
                };
                let failed: Vec<_> = failures.iter().filter(|(field, _)| field == key).collect();
                let evidence = evidence::collect(
                    &requirements[key],
                    key,
                    facts,
                    patterns,
                    failed
                        .iter()
                        .any(|(_, code)| *code == "evidence_unavailable"),
                )?;
                if failed.is_empty() {
                    requirement_checks.push(Check {
                        rule_ref: rule_ref.clone(),
                        operation_ref: "request".into(),
                        predicate_path: path.clone(),
                        status: "met",
                        code: "requirement_met",
                        branch,
                        applied: false,
                        evidence: evidence.clone(),
                        private_input: false,
                    });
                }
                for (_, code) in failed {
                    requirement_checks.push(Check {
                        rule_ref: rule_ref.clone(),
                        operation_ref: "request".into(),
                        predicate_path: path.clone(),
                        status: if *code == "evidence_unavailable" {
                            "unknown"
                        } else {
                            "failed"
                        },
                        code,
                        branch,
                        applied: false,
                        evidence: evidence.clone(),
                        private_input: false,
                    });
                }
            }
        }
        Ok(requirement_checks)
    }
    /// The met branch ranks conditions and requirements together, since both
    /// were verified; each side's predicates fill keys the other lacks.
    fn rank(&self) -> u8 {
        let Some(requirements) = &self.requirements else {
            return rank(&self.conditions);
        };
        let merged = |first: &Value, second: &Value| {
            let mut merged = first.clone();
            if let (Some(merged), Some(second)) = (merged.as_object_mut(), second.as_object()) {
                for (key, value) in second {
                    merged.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
            merged
        };
        rank(&merged(&self.conditions, requirements))
            .max(rank(&merged(requirements, &self.conditions)))
    }
}

fn rank(value: &Value) -> u8 {
    if value["domain"] == "exact_request" {
        return 4;
    }
    let mut rank = 0;
    let exact =
        value["arguments"]["kind"] == "exact" || selectors::exact(&value["source"]).is_some();
    if exact {
        rank = 3;
    }
    if value["domain"] == "mcp" && value.get("arguments").is_some() {
        rank = 3;
    }
    if value["arguments"]["kind"] == "risk_tokens" {
        rank = 3;
    }
    if exact && value["cwd"].get("exact").is_some() && value["environment"].get("exact").is_some() {
        return 4;
    }
    for key in [
        "cwd",
        "environment",
        "targets",
        "argument_paths",
        "docker_isolated_bind",
        "graphql",
        "field_paths",
        "sources",
        "destination",
        "fields",
        "request_fields",
        "url",
    ] {
        if value.get(key).is_some() {
            rank = rank.max(2);
        }
    }
    if value["executable"].get("root").is_some() {
        rank = rank.max(2);
    }
    match value["arguments"]["kind"].as_str() {
        Some("options") => rank = rank.max(2),
        Some("sequence")
            if !(value["arguments"]["prefix"]
                .as_array()
                .is_some_and(Vec::is_empty)
                && value["arguments"]["rest"]["kind"] == "any") =>
        {
            rank = rank.max(2);
        }
        Some("prefix") => rank = rank.max(1),
        _ => {}
    }
    if value.get("query").is_some()
        || value.get("method").is_some()
        || value.get("dialect_version").is_some()
        || (value.get("source").is_some() && !exact)
    {
        rank = rank.max(1);
    }
    rank
}
