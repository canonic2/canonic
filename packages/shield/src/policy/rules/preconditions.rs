//! Unranked pack gates: a failed precondition decides its pack before rules compete.
use super::{
    Check, Error, Facts, Match, Profile, Unmet, capabilities, checks, domain_matches, valid_id,
    validation::validate_predicates,
};
use crate::policy::{normalized::Input, patterns::Provider, selectors, variables::Test};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

const MAX_ALTERNATIVES: usize = 16;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Precondition {
    pub(crate) id: String,
    pub(crate) conditions: Value,
    #[serde(
        default,
        deserialize_with = "super::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) when: Option<Vec<Test>>,
    pub(crate) requires_any: Vec<Value>,
    pub(crate) on_unmet: Unmet,
    #[serde(
        default,
        deserialize_with = "super::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) reason: Option<String>,
}

fn identities(domain: &str) -> &'static [&'static str] {
    match domain {
        "argv" => &["executable"],
        "shell" => &["dialect"],
        "file" | "web" => &["action"],
        "mcp" => &["server", "tool"],
        _ => &[],
    }
}

/// Split one requirement object into identity fields and ordinary predicates.
fn split(alternative: &Value, domain: &str) -> Result<(Map<String, Value>, Value), Error> {
    let fields = alternative.as_object().ok_or(Error::Syntax)?;
    let mut identity = Map::new();
    let mut rest = Map::new();
    for (key, value) in fields {
        if identities(domain).contains(&key.as_str()) {
            identity.insert(key.clone(), value.clone());
        } else {
            rest.insert(key.clone(), value.clone());
        }
    }
    Ok((identity, Value::Object(rest)))
}

impl Precondition {
    pub(crate) fn domain(&self) -> &str {
        self.conditions["domain"].as_str().unwrap_or_default()
    }

    /// Validate a resolved precondition; `{var}` and `when` must already be gone.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.validate_for(false)
    }

    /// Bound MCP alternatives can all disappear; that gate remains unmet.
    pub(crate) fn validate_bound(&self) -> Result<(), Error> {
        self.validate_for(true)
    }

    fn validate_for(&self, bound: bool) -> Result<(), Error> {
        if !valid_id(&self.id) {
            return Err(Error::Contract("invalid precondition identity"));
        }
        if self.when.is_some() {
            return Err(Error::Contract("unresolved precondition activation"));
        }
        let domain = self.conditions["domain"]
            .as_str()
            .ok_or(Error::Contract("domain required"))?;
        if domain == "exact_request" {
            return Err(Error::Contract("preconditions cannot bind exact requests"));
        }
        validate_predicates(&self.conditions, domain, false, false)?;
        if ((!bound || domain != "mcp") && self.requires_any.is_empty())
            || self.requires_any.len() > MAX_ALTERNATIVES
        {
            return Err(Error::Contract(
                "requires_any lists 1-16 requirement objects",
            ));
        }
        for alternative in &self.requires_any {
            let (identity, rest) = split(alternative, domain)?;
            if identity.is_empty() && rest.as_object().is_none_or(Map::is_empty) {
                return Err(Error::Contract("requirement objects must be nonempty"));
            }
            if !rest.as_object().is_none_or(Map::is_empty) {
                validate_predicates(&rest, domain, true, false)?;
            }
            if !identity.is_empty() {
                let mut whole = self.conditions.clone();
                let fields = whole.as_object_mut().ok_or(Error::Syntax)?;
                fields
                    .retain(|key, _| key == "domain" || identities(domain).contains(&key.as_str()));
                fields.extend(identity);
                validate_predicates(&whole, domain, false, false)?;
            }
        }
        self.on_unmet.decision.validate(true, true)?;
        if let Some(guidance) = self.on_unmet.guidance() {
            guidance.validate()?;
        }
        if self
            .reason
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 4096)
        {
            return Err(Error::Contract("invalid precondition reason"));
        }
        let mut value = json!(self);
        if crate::json::contains_nul(&value) || crate::evidence::redact(&mut value) {
            return Err(Error::Contract("NUL or credentials in precondition"));
        }
        Ok(())
    }

    pub(crate) fn capabilities(&self, result: &mut BTreeSet<String>) {
        let domain = self.domain();
        if domain == "argv" {
            result.insert("argument-rules-v1".into());
        }
        if domain == "file" {
            result.insert("location-v1".into());
        }
        for value in std::iter::once(&self.conditions).chain(&self.requires_any) {
            capabilities(value, domain, result);
        }
    }

    pub(crate) fn semantic(&self) -> bool {
        std::iter::once(&self.conditions)
            .chain(&self.requires_any)
            .any(|v| v.get("possible_effects").is_some())
    }

    /// The failed gate for this request, or None when it does not apply or holds.
    pub(crate) fn failure(
        &self,
        facts: &Facts<'_>,
        patterns: &dyn Provider,
        source: &str,
        profile: Profile,
    ) -> Result<Option<Match>, Error> {
        if !domain_matches(&self.conditions, facts.input, patterns)?
            || !checks(&self.conditions, facts, patterns)?.is_empty()
        {
            return Ok(None);
        }
        let rule_ref = json!({"scope":"pack","pack_selector":source,"rule_id":self.id});
        let mut requirement_checks = Vec::new();
        for (position, alternative) in self.requires_any.iter().enumerate() {
            let failures = self.alternative(alternative, facts, patterns)?;
            if failures.is_empty() {
                return Ok(None);
            }
            for (key, code) in failures {
                requirement_checks.push(Check {
                    rule_ref: rule_ref.clone(),
                    operation_ref: "request",
                    predicate_path: format!(
                        "/requires_any/{position}/{}",
                        key.replace('~', "~0").replace('/', "~1")
                    ),
                    status: if code == "evidence_unavailable" {
                        "unknown"
                    } else {
                        "failed"
                    },
                    code,
                    branch: "unmet",
                    applied: false,
                    evidence: super::evidence::collect(&alternative[&key], &key, facts, patterns)?,
                });
            }
        }
        Ok(Some(Match {
            source: source.into(),
            rule_id: self.id.clone(),
            decision: self.on_unmet.decision.selected(profile),
            reason: self.reason.clone().unwrap_or_default(),
            specificity: 2,
            applied: false,
            tier: 1,
            branch: "unmet",
            requirement_checks,
            guidance: self.on_unmet.guidance(),
            exact_request: false,
            fallback: false,
            precondition: true,
        }))
    }

    fn alternative(
        &self,
        alternative: &Value,
        facts: &Facts<'_>,
        patterns: &dyn Provider,
    ) -> Result<Vec<(String, &'static str)>, Error> {
        let (identity, rest) = split(alternative, self.domain())?;
        let mut failures = Vec::new();
        for (key, selector) in &identity {
            let actual = match (key.as_str(), facts.input) {
                ("executable", Input::Argv { executable, .. }) => Some(executable.as_str()),
                ("dialect", Input::Shell { dialect, .. }) => Some(dialect.as_str()),
                ("action", Input::File { action, .. } | Input::Web { action, .. }) => {
                    Some(action.as_str())
                }
                ("server", Input::Mcp { server, .. }) => Some(server.as_str()),
                ("tool", Input::Mcp { tool, .. }) => Some(tool.as_str()),
                _ => None,
            };
            let met = if selector.get("root").is_some() {
                match facts.locations.map(|l| l.executable(selector)) {
                    Some(None) => true,
                    Some(Some(code)) => {
                        failures.push((key.clone(), code));
                        continue;
                    }
                    None => {
                        failures.push((key.clone(), "evidence_unavailable"));
                        continue;
                    }
                }
            } else {
                actual.map_or(Ok(false), |v| selectors::matches(selector, v, patterns))?
            };
            if !met {
                failures.push((key.clone(), "predicate_mismatch"));
            }
        }
        if !rest.as_object().is_none_or(Map::is_empty) {
            failures.extend(checks(&rest, facts, patterns)?);
            if !facts.known_arguments && rest.get("arguments").is_some() {
                failures.push(("arguments".into(), "evidence_unavailable"));
            }
        }
        Ok(failures)
    }
}
