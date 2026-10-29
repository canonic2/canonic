//! Version-one policy documents; literal projections belong only to bounded comparison.
#[cfg(test)]
use super::Request;
use super::{
    Error,
    directories::{self, DirectoryOption},
    patterns, resolution,
    rules::{Precondition, Rule},
    variables::Declaration,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PackDocument {
    schema_version: u8,
    kind: String,
    pub(super) id: String,
    pub(super) version: String,
    description: String,
    license: String,
    maintainers: Vec<String>,
    engines: Engines,
    capabilities: Vec<String>,
    pub(super) rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) invocations: BTreeMap<String, super::invocations::Invocation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) command_sets: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(
        default,
        deserialize_with = "super::dependencies::declarations",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) dependencies: Option<Vec<super::Declaration>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) variables: BTreeMap<String, Declaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) preconditions: Vec<Precondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) directory_options: Vec<DirectoryOption>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) mcp_integrations: BTreeMap<String, crate::mcp_connections::Integration>,
}

/// The program versions a pack requires, keyed by program name. Shield is the
/// only program it can check, so any other name refuses the pack.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Engines {
    shield: String,
}

impl PackDocument {
    pub(super) fn descriptor(&self) -> Self {
        Self {
            schema_version: self.schema_version,
            kind: self.kind.clone(),
            id: self.id.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            license: self.license.clone(),
            maintainers: self.maintainers.clone(),
            engines: self.engines.clone(),
            capabilities: self.capabilities.clone(),
            rules: vec![],
            invocations: BTreeMap::new(),
            command_sets: BTreeMap::new(),
            dependencies: self.dependencies.clone(),
            variables: BTreeMap::new(),
            preconditions: self.preconditions.clone(),
            directory_options: self.directory_options.clone(),
            mcp_integrations: self.mcp_integrations.clone(),
        }
    }
    pub(super) fn templates_enabled(&self) -> bool {
        self.capabilities.iter().any(|c| c == "templates-v1")
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CustomDocument {
    schema_version: u8,
    kind: String,
    capabilities: Vec<String>,
    pub(super) rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) directory_options: Vec<DirectoryOption>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Matcher {
    executable: String,
    arguments: Arguments,
    #[serde(
        default,
        deserialize_with = "optional_context",
        skip_serializing_if = "Option::is_none"
    )]
    cwd: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_context",
        skip_serializing_if = "Option::is_none"
    )]
    project: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_effects",
        skip_serializing_if = "Option::is_none"
    )]
    possible_effects: Option<Vec<crate::effects::Effect>>,
}

fn optional_effects<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<Vec<crate::effects::Effect>>, D::Error> {
    Vec::deserialize(input).map(Some)
}

fn optional_context<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(input).map(Some)
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Arguments {
    Exact { values: Vec<String> },
    Prefix { values: Vec<String> },
    Any,
}

impl Matcher {
    #[cfg(test)]
    pub(super) fn matches(&self, request: &Request) -> bool {
        self.matches_evidence(request, crate::effects::classify(request).as_ref())
    }

    #[cfg(test)]
    fn matches_evidence(&self, request: &Request, hints: Option<&crate::effects::Hints>) -> bool {
        self.executable == request.executable
            && self.context_matches(&request.cwd, &request.project)
            && self.possible_effects.as_ref().is_none_or(|effects| {
                hints.is_some_and(|hints| effects.iter().any(|effect| hints.may(*effect)))
            })
            && match &self.arguments {
                Arguments::Exact { values } => values == &request.arguments,
                Arguments::Prefix { values } => request.arguments.starts_with(values),
                Arguments::Any => true,
            }
    }

    pub(super) fn semantic(&self) -> bool {
        self.possible_effects.is_some()
    }

    pub(super) fn effects(&self) -> &[crate::effects::Effect] {
        self.possible_effects.as_deref().unwrap_or_default()
    }

    // An exact argv leaf is a singleton: classification cannot vary within it.
    // Prefix/any effects require a classifier-aware partition of their suffixes.
    pub(super) fn partitionable(&self) -> bool {
        !self.semantic() || matches!(self.arguments, Arguments::Exact { .. })
    }

    pub(super) fn context_matches(&self, cwd: &str, project: &str) -> bool {
        self.cwd.as_ref().is_none_or(|value| value == cwd)
            && self.project.as_ref().is_none_or(|value| value == project)
    }

    pub(super) fn executable(&self) -> &str {
        &self.executable
    }

    pub(super) fn contexts(&self) -> [Option<&str>; 2] {
        [self.cwd.as_deref(), self.project.as_deref()]
    }

    pub(super) fn argument_values(&self) -> Option<&[String]> {
        match &self.arguments {
            Arguments::Exact { values } | Arguments::Prefix { values } => Some(values),
            Arguments::Any => None,
        }
    }

    pub(super) fn validate(&self) -> Result<(), Error> {
        if let Some(effects) = &self.possible_effects
            && (effects.is_empty()
                || effects.len() > 11
                || effects.iter().collect::<BTreeSet<_>>().len() != effects.len())
        {
            return Err(Error::Contract(
                "possible effects must be nonempty and unique",
            ));
        }
        text(&self.executable)?;
        for context in [&self.cwd, &self.project].into_iter().flatten() {
            text(context)?;
            if !std::path::Path::new(context).is_absolute() {
                return Err(Error::Contract("matcher context must be absolute"));
            }
        }
        if let Arguments::Exact { values } | Arguments::Prefix { values } = &self.arguments {
            if values.len() > 256 {
                return Err(Error::Limit);
            }
            for value in values {
                if value.len() > 4096 {
                    return Err(Error::Limit);
                }
                if value.contains('\0') {
                    return Err(Error::Contract("NUL in matcher"));
                }
            }
            if matches!(self.arguments, Arguments::Prefix { .. }) && values.is_empty() {
                return Err(Error::Contract("prefix must contain an argument"));
            }
        }
        Ok(())
    }
}

/// Project policy scope. A label does not establish ownership or enrollment.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Nearest ordinary project policy.
    Project,
}

impl PackDocument {
    pub(super) fn validate_schema(&self) -> Result<(), Error> {
        schema(self.schema_version, &self.kind, "pack")
    }
    /// Validate a source document. A dynamic one is validated through a sample
    /// resolution that keeps every entry, with its features counted as used.
    pub(super) fn validate(&mut self) -> Result<(), Error> {
        self.validate_header()?;
        if !resolution::dynamic(self) {
            return self.validate_body(Some(BTreeSet::new()));
        }
        let (mut sample, features) = resolution::sample(self)?;
        let mut extra = features.patterns;
        if features.variables {
            extra.insert("variables-v1".to_owned());
        }
        if features.templates {
            extra.insert("templates-v1".to_owned());
        }
        if !self.preconditions.is_empty() {
            extra.insert("preconditions-v1".to_owned());
        }
        sample.validate_body(Some(extra))
    }
    /// Validate a resolved document; capabilities were checked on its source.
    pub(super) fn validate_resolved(&mut self) -> Result<(), Error> {
        self.validate_body(None)
    }
    fn validate_body(&mut self, extra: Option<BTreeSet<String>>) -> Result<(), Error> {
        if self.preconditions.len() > 256 {
            return Err(Error::Limit);
        }
        let mut expected = validate_rules(&mut self.rules, true)?;
        if self.mcp_integrations.len() > 64 {
            return Err(Error::Limit);
        }
        for (name, integration) in &self.mcp_integrations {
            if !crate::mcp_connections::valid_name(name) {
                return Err(Error::Contract("invalid pack MCP integration name"));
            }
            integration.validate(name)?;
            if !integration.command_fields.is_empty() {
                expected.insert("mcp-command-fields-v1".into());
            }
            if !integration.effective_fields.is_empty() {
                expected.insert("mcp-effective-fields-v1".into());
            }
            super::rules::capabilities(
                &serde_json::json!({"fields":integration.fields}),
                "mcp",
                &mut expected,
            );
        }
        if !self.mcp_integrations.is_empty() {
            expected.insert("mcp-integrations-v1".into());
        }
        let mut ids: BTreeSet<&str> = self.rules.iter().map(|r| r.id.as_str()).collect();
        for precondition in &self.preconditions {
            precondition.validate()?;
            if !ids.insert(&precondition.id) {
                return Err(Error::Contract("duplicate rule identity"));
            }
            precondition.capabilities(&mut expected);
        }
        directories::validate(&self.directory_options)?;
        if !self.directory_options.is_empty() {
            expected.insert(DIRECTORY_OPTIONS.into());
        }
        if let Some(extra) = extra {
            if self.dependencies.is_some() {
                expected.insert("dependencies-v1".into());
            }
            expected.extend(extra);
            declared(&self.capabilities, &expected)?;
        }
        super::invocations::validate(self)
    }
    fn validate_header(&self) -> Result<(), Error> {
        self.validate_schema()?;
        let parts: Vec<_> = self.id.split('/').collect();
        if parts.len() != 2 || !parts.iter().all(|s| identifier(s)) || self.id.len() > 128 {
            return Err(Error::Contract("pack ID must be publisher/name"));
        }
        for value in [
            &self.description,
            &self.license,
            &self.engines.shield,
            &self.version,
        ] {
            text(value)?;
        }
        if self.version.len() > 128 || self.engines.shield.len() > 1024 {
            return Err(Error::Limit);
        }
        semver::Version::parse(&self.version)
            .map_err(|_| Error::Contract("invalid pack version"))?;
        let requirement = semver::VersionReq::parse(&self.engines.shield)
            .map_err(|_| Error::Contract("invalid Shield version requirement"))?;
        let version =
            semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_| Error::Syntax)?;
        if !requirement.matches(&version) {
            return Err(Error::Contract("pack requires a different Shield version"));
        }
        if self.maintainers.is_empty() || self.maintainers.len() > 256 {
            return Err(Error::Limit);
        }
        let mut maintainers = BTreeSet::new();
        for value in &self.maintainers {
            text(value)?;
            if value.len() > 128 || !maintainers.insert(value) {
                return Err(Error::Contract("invalid maintainers"));
            }
        }
        if let Some(dependencies) = &self.dependencies {
            if dependencies.len() > 64 {
                return Err(Error::Limit);
            }
            let mut seen = BTreeSet::new();
            for dependency in dependencies {
                dependency
                    .parsed()
                    .map_err(|_| Error::Contract("invalid dependency declaration"))?;
                if !seen.insert((&dependency.registry, &dependency.pack)) {
                    return Err(Error::Contract("duplicate dependency identity"));
                }
            }
        }
        Ok(())
    }
}
impl CustomDocument {
    pub(super) fn validate_schema(&self) -> Result<(), Error> {
        schema(self.schema_version, &self.kind, "custom")
    }
    pub(super) fn empty() -> Self {
        Self {
            schema_version: 1,
            kind: "custom".into(),
            capabilities: vec!["structured-rules-v1".into()],
            rules: vec![],
            directory_options: vec![],
        }
    }
    pub(super) fn validate(&mut self) -> Result<(), Error> {
        self.validate_schema()?;
        let mut expected = validate_rules(&mut self.rules, false)?;
        directories::validate(&self.directory_options)?;
        if !self.directory_options.is_empty() {
            expected.insert(DIRECTORY_OPTIONS.into());
        }
        declared(&self.capabilities, &expected)
    }
}

/// The capability a document declares when it holds directory options.
pub(super) const DIRECTORY_OPTIONS: &str = "directory-options-v1";
fn schema(version: u8, kind: &str, expected: &str) -> Result<(), Error> {
    if version != 1 || kind != expected {
        return Err(Error::Contract("unsupported policy schema or kind"));
    }
    Ok(())
}
fn validate_rules(rules: &mut [Rule], pack: bool) -> Result<BTreeSet<String>, Error> {
    if rules.len() > 500_000 {
        return Err(Error::Limit);
    }
    let mut expected = BTreeSet::from(["structured-rules-v1".to_owned()]);
    let mut ids = BTreeSet::new();
    for rule in rules {
        rule.validate_for(pack)?;
        if !ids.insert(rule.id.clone()) {
            return Err(Error::Contract("duplicate rule identity"));
        }
        rule.capabilities(&mut expected);
        rule.prepare();
    }
    Ok(expected)
}
fn declared(declared: &[String], expected: &BTreeSet<String>) -> Result<(), Error> {
    if declared.len() != expected.len()
        || declared.iter().collect::<BTreeSet<_>>().len() != declared.len()
        || declared.iter().any(|v| !expected.contains(v))
    {
        return Err(Error::Contract(
            "capabilities must declare exactly the features used",
        ));
    }
    Ok(())
}
pub(super) fn prepare_patterns(
    rules: &[Rule],
    preconditions: &[Precondition],
) -> Result<patterns::Store, Error> {
    let mut store = patterns::Store::default();
    for precondition in preconditions {
        let domain = precondition.domain();
        store.collect(&precondition.conditions, domain)?;
        for alternative in &precondition.requires_any {
            store.collect(alternative, domain)?;
        }
    }
    for rule in rules {
        let domain = rule.conditions["domain"].as_str().unwrap_or_default();
        if domain == "exact_request" {
            continue;
        }
        store.collect(&rule.conditions, domain)?;
        if let Some(requirements) = &rule.requirements {
            store.collect(requirements, domain)?;
        }
    }
    Ok(store)
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
        return Err(Error::Contract("nonempty text required"));
    }
    if value.len() > 4096 {
        return Err(Error::Limit);
    }
    Ok(())
}
