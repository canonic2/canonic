//! Frozen codecs for authenticated, retained prerelease policy sources.
//!
//! The historical documents are validated before translation. Their source bytes
//! remain the digest identity; these codecs do not acquire or rewrite policy.
use super::{Decision, Error, Scope, model, rules};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

mod dependencies;
mod normalized;
#[cfg(test)]
mod tests;
pub(super) use normalized::{NormalizedAuthority, validate_normalized_authority};

/// Matching semantics authenticated alongside a compiled historical rule.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Authority {
    pub(super) matcher: model::Matcher,
    pub(super) scope: Option<Scope>,
    pub(super) mandatory: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    schema_version: u8,
    id: String,
    version: String,
    description: String,
    license: String,
    maintainers: Vec<String>,
    engine: String,
    capabilities: Vec<String>,
    rules: Vec<PackRule>,
    #[serde(default, deserialize_with = "dependencies::declarations")]
    dependencies: Option<Vec<dependencies::Declaration>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackRule {
    id: String,
    matcher: model::Matcher,
    reason: String,
    profiles: rules::Profiles,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Custom {
    schema_version: u8,
    rules: Vec<CustomRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomRule {
    id: String,
    scope: Scope,
    #[serde(default)]
    mandatory: bool,
    decision: Decision,
    matcher: model::Matcher,
    reason: String,
}

fn source(input: &str) -> Result<(Value, bool), Error> {
    if input.len() > super::MAX_DOCUMENT_BYTES {
        return Err(Error::Limit);
    }
    if input.len() > 1_048_576 {
        // Inspect only the envelope before allocating the full rule tree. Modern
        // documents have a larger budget and may put `kind` after their rules.
        #[derive(Deserialize)]
        struct Header {
            kind: Option<String>,
        }
        let header: Header = serde_json::from_str(input)
            .map_err(|_| Error::Syntax)
            .or_else(|_| super::parse_yaml(input))?;
        if header.kind.is_none() {
            return Err(Error::Limit);
        }
    }
    let value: Value = super::parse(input)?;
    let historical = value.is_object() && value.get("kind").is_none();
    Ok((value, historical))
}

fn decode<T: serde::de::DeserializeOwned>(value: Value, input: &str) -> Result<T, Error> {
    // YAML's typed String decoder preserves plain scalar text (for example
    // `reason: true`). The common value path must retain that compatibility.
    serde_json::from_value(value)
        .map_err(|_| Error::Syntax)
        .or_else(|_| super::parse(input))
}

pub(super) fn pack(input: &str) -> Result<model::PackDocument, Error> {
    let (value, historical) = source(input)?;
    if !historical {
        return decode(value, input);
    }
    let source: Pack = decode(value, input)?;
    source.validate()?;
    let mut rules = Vec::with_capacity(source.rules.len());
    for row in source.rules {
        let decisions = rules::Decisions::Profiles(row.profiles);
        rules.push(translate(
            &row.id,
            row.matcher,
            &row.reason,
            &decisions,
            None,
        )?);
    }
    let mut capabilities = BTreeSet::from(["structured-rules-v1".to_owned()]);
    for row in &rules {
        row.capabilities(&mut capabilities);
    }
    if source.dependencies.is_some() {
        capabilities.insert("dependencies-v1".into());
    }
    // Metadata stays descriptive. Compatibility of the old engine requirement
    // was established when these exact source bytes entered retained state.
    let mut value = json!({
        "schema_version":1,"kind":"pack","id":source.id,"version":source.version,
        "description":source.description,"license":source.license,
        "maintainers":source.maintainers,"engines":{"shield":"*"},
        "capabilities":capabilities,"rules":[]
    });
    if let Some(dependencies) = source.dependencies {
        value["dependencies"] = serde_json::to_value(dependencies).map_err(|_| Error::Syntax)?;
    }
    let mut document: model::PackDocument =
        serde_json::from_value(value).map_err(|_| Error::Syntax)?;
    document.retained_legacy = true;
    document.rules = rules;
    document.validate_resolved()?;
    Ok(document)
}

/// Recheck historical descriptive header limits after authenticated compilation.
pub(super) fn validate_header(document: &model::PackDocument) -> Result<(), Error> {
    let current = serde_json::to_value(document).map_err(|_| Error::Syntax)?;
    let dependencies = current.get("dependencies");
    let mut old = json!({
        "schema_version":if dependencies.is_some() { 3 } else { 1 },
        "id":current["id"],"version":current["version"],
        "description":current["description"],"license":current["license"],
        "maintainers":current["maintainers"],"engine":current["engines"]["shield"],
        "capabilities":if dependencies.is_some() {
            vec!["argv-v1", "dependencies-v1"]
        } else { vec!["argv-v1"] },
        "rules":[]
    });
    if let Some(dependencies) = dependencies {
        old["dependencies"] = dependencies.clone();
    }
    let source: Pack = serde_json::from_value(old).map_err(|_| Error::Syntax)?;
    source.validate()
}

pub(super) fn custom(input: &str) -> Result<model::CustomDocument, Error> {
    let (value, historical) = source(input)?;
    if !historical {
        let mut document: model::CustomDocument = decode(value, input)?;
        document.validate()?;
        return Ok(document);
    }
    if value["rules"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row.get("conditions").is_some()))
    {
        return normalized::document(value, input);
    }
    let source: Custom = decode(value, input)?;
    if !matches!(source.schema_version, 1 | 2) {
        return Err(Error::Contract("unsupported historical custom schema"));
    }
    let mut ids = BTreeSet::new();
    rule_count(source.rules.len())?;
    let mut rules = Vec::with_capacity(source.rules.len());
    for row in source.rules {
        if row.mandatory || row.scope != Scope::Project {
            return Err(Error::Contract("unsupported retained custom authority"));
        }
        validate_rule(&row.id, &row.reason, &row.matcher, &mut ids)?;
        if source.schema_version == 1 && row.matcher.semantic() {
            return Err(Error::Contract(
                "effect predicates require schema version 2",
            ));
        }
        rules.push(translate(
            &row.id,
            row.matcher,
            &row.reason,
            &rules::Decisions::Scalar(row.decision),
            Some(row.scope),
        )?);
    }
    model::CustomDocument::from_rules(rules)
}

fn translate(
    id: &str,
    matcher: model::Matcher,
    reason: &str,
    decision: &rules::Decisions,
    scope: Option<Scope>,
) -> Result<rules::Rule, Error> {
    let value = serde_json::to_value(&matcher).map_err(|_| Error::Syntax)?;
    let mut conditions = json!({
        "domain":"argv","executable":value["executable"],"arguments":value["arguments"]
    });
    if let Some(effects) = value.get("possible_effects") {
        conditions["possible_effects"] = effects.clone();
    }
    let mut rule: rules::Rule = serde_json::from_value(json!({
        "id":id,"decision":decision,"conditions":conditions,"reason":reason
    }))
    .map_err(|_| Error::Syntax)?;
    rule.legacy = Some(Authority {
        matcher,
        scope,
        mandatory: false,
    });
    Ok(rule)
}

/// The source feature carries a matcher, never a caller-chosen rank or tier.
pub(super) fn validate_authority(rule: &rules::Rule, pack: bool) -> Result<(), Error> {
    let Some(authority) = &rule.legacy else {
        return Ok(());
    };
    if authority.mandatory || authority.scope != if pack { None } else { Some(Scope::Project) } {
        return Err(Error::Contract("unsupported historical matcher authority"));
    }
    rule.decision.validate(pack, false)?;
    let reason = rule.reason.as_deref().ok_or(Error::Contract(
        "historical matcher requires its original reason",
    ))?;
    let mut ids = BTreeSet::new();
    validate_rule(&rule.id, reason, &authority.matcher, &mut ids)?;
    let expected = translate(
        &rule.id,
        authority.matcher.clone(),
        reason,
        &rule.decision,
        authority.scope,
    )?;
    if serde_json::to_value(rule).map_err(|_| Error::Syntax)?
        != serde_json::to_value(expected).map_err(|_| Error::Syntax)?
    {
        return Err(Error::Contract(
            "historical matcher must preserve its exact rule",
        ));
    }
    Ok(())
}

impl Pack {
    fn validate(&self) -> Result<(), Error> {
        if !matches!(self.schema_version, 1..=3)
            || (self.schema_version == 3) != self.dependencies.is_some()
        {
            return Err(Error::Contract("unsupported historical pack schema"));
        }
        let expected = match self.schema_version {
            1 => vec!["argv-v1"],
            2 => vec!["argv-v1", "possible-effects-v1"],
            _ if self.capabilities.iter().any(|c| c == "possible-effects-v1") => {
                vec!["argv-v1", "possible-effects-v1", "dependencies-v1"]
            }
            _ => vec!["argv-v1", "dependencies-v1"],
        };
        if self.capabilities != expected {
            return Err(Error::Contract("unsupported historical capabilities"));
        }
        let segments: Vec<_> = self.id.split('/').collect();
        if segments.len() != 2 || !segments.iter().all(|s| identifier(s)) {
            return Err(Error::Contract("pack ID must be namespace/name"));
        }
        for value in [
            &self.version,
            &self.engine,
            &self.description,
            &self.license,
        ] {
            text(value)?;
        }
        semver::Version::parse(&self.version)
            .map_err(|_| Error::Contract("invalid pack version"))?;
        semver::VersionReq::parse(&self.engine)
            .map_err(|_| Error::Contract("invalid engine requirement"))?;
        if self.maintainers.is_empty() || self.maintainers.len() > 32 {
            return Err(Error::Contract("one to 32 maintainers required"));
        }
        for maintainer in &self.maintainers {
            text(maintainer)?;
        }
        validate_dependencies(self.dependencies.as_deref())?;
        let mut ids = BTreeSet::new();
        rule_count(self.rules.len())?;
        for row in &self.rules {
            validate_rule(&row.id, &row.reason, &row.matcher, &mut ids)?;
            rules::Decisions::Profiles(row.profiles.clone()).validate(true, false)?;
            if row.matcher.semantic()
                && !self.capabilities.iter().any(|c| c == "possible-effects-v1")
            {
                return Err(Error::Contract(
                    "effect predicates require possible-effects-v1",
                ));
            }
        }
        Ok(())
    }
}

fn validate_dependencies(dependencies: Option<&[dependencies::Declaration]>) -> Result<(), Error> {
    let mut seen = BTreeSet::new();
    if let Some(dependencies) = dependencies {
        if dependencies.len() > 64 {
            return Err(Error::Limit);
        }
        for dependency in dependencies {
            dependency
                .current()
                .parsed()
                .map_err(|_| Error::Contract("invalid dependency declaration"))?;
            if !seen.insert((&dependency.registry, &dependency.pack)) {
                return Err(Error::Contract("duplicate dependency identity"));
            }
        }
    }
    Ok(())
}

fn validate_rule(
    id: &str,
    reason: &str,
    matcher: &model::Matcher,
    ids: &mut BTreeSet<String>,
) -> Result<(), Error> {
    if !identifier(id) || !ids.insert(id.to_owned()) {
        return Err(Error::Contract(
            "rule IDs must be unique lowercase identifiers",
        ));
    }
    text(reason)?;
    matcher.validate()
}

fn rule_count(count: usize) -> Result<(), Error> {
    if count > 1024 {
        Err(Error::Limit)
    } else {
        Ok(())
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn text(value: &str) -> Result<(), Error> {
    if value.trim().is_empty() || value.contains('\0') {
        Err(Error::Contract("nonempty text required"))
    } else if value.len() > 4096 {
        Err(Error::Limit)
    } else {
        Ok(())
    }
}
