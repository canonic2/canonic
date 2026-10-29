//! Validates declarative project policy and composes decisions without I/O.

pub(crate) mod agent_outputs;
mod argument_rules;
pub(crate) mod compiled;
pub(crate) mod dependencies;
pub(crate) mod directories;
mod index;
mod invocations;
mod json_budget;
mod model;
pub(crate) mod normalized;
pub(crate) mod partition;
pub(crate) mod patterns;
pub(crate) mod portability;
pub(crate) mod profile_comparison;
pub(crate) mod resolution;
pub(crate) mod resources;
pub(crate) mod rules;
pub(crate) mod schema;
pub(crate) mod selectors;
pub(crate) mod symbolic;
mod templates;
#[cfg(test)]
mod tests;
pub(crate) mod variables;

pub(crate) fn validate_mcp_fields(fields: &serde_json::Value) -> Result<(), Error> {
    selectors::field_map(fields)?;
    let mut patterns = patterns::Store::default();
    patterns.collect(fields, "mcp")
}

pub(crate) fn matches_mcp_fields(
    fields: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<bool, Error> {
    validate_mcp_fields(fields)?;
    let mut patterns = patterns::Store::default();
    patterns.collect(fields, "mcp")?;
    Ok(selectors::fields_match(fields, value, &patterns)?.is_none())
}

pub use dependencies::{Declaration, Mapping};
pub use model::Scope;
use model::{CustomDocument, PackDocument};
pub use resolution::{Binding, Override};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum encoded size of one policy document.
pub const MAX_DOCUMENT_BYTES: usize = 256 * 1024 * 1024;

/// Every capability this engine implements. A document declaring any other
/// was written for a different Shield version.
pub(crate) const CAPABILITIES: &[&str] = &[
    "structured-rules-v1",
    "argument-rules-v1",
    "argument-paths-v1",
    "glob-v1",
    "regex-v1",
    "location-v1",
    "access-paths-v1",
    "requirements-v1",
    "possible-effects-v1",
    "fallback-rules-v1",
    "dependencies-v1",
    "invocation-groups-v1",
    "prefix-leading-v1",
    "command-sets-v1",
    "variables-v1",
    "preconditions-v1",
    "directory-options-v1",
    "templates-v1",
    "agent-outputs-v1",
    "mcp-integrations-v1",
    "mcp-connection-facts-v1",
    "mcp-command-fields-v1",
    "mcp-effective-fields-v1",
    rules::UNKNOWN_OPTIONS,
    rules::OPTION_ABBREVIATIONS,
    rules::NEGATED_SELECTORS,
];

/// Decision severity, ordered from least to most restrictive.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// The matching policy permits the request.
    Allow,
    /// Human approval is required.
    Ask,
    /// The request is blocked.
    Deny,
}

/// Registry-owned protection profile selection, ordered from least to most restrictive.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    Serialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Least restrictive profile.
    Loose,
    /// Default profile.
    #[default]
    Recommended,
    /// Most restrictive profile.
    Strict,
}

/// Invalid policy/request input or cooperative interruption. Messages omit document snippets.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Cooperative caller interruption; no completed decision is available.
    #[error("evaluation interrupted")]
    Interrupted,
    /// A document could not be parsed using the strict schema.
    #[error("invalid policy document")]
    Syntax,
    /// A supported resource limit was exceeded.
    #[error("policy or request exceeds a resource limit")]
    Limit,
    /// A document violates the versioned semantic contract.
    #[error("invalid policy: {0}")]
    Contract(&'static str),
    /// Two selected documents declare one directory option differently. The
    /// bounded message names the option and both declaring sources.
    #[error("invalid policy: {0}")]
    DirectoryOptionConflict(String),
    /// Invalid MCP default shape, naming only bounded integration and variable IDs.
    #[error("{}", mcp_defaults_message(integration, variables, reason))]
    McpDefaults {
        /// Declared integration identity.
        integration: String,
        /// Variables referenced by these defaults; no values are retained.
        variables: Vec<String>,
        /// Static diagnostic describing the invalid shape.
        reason: &'static str,
    },
    /// A selection's variable binding is refused. The bounded selection and
    /// variable names are validated identifiers; values are never included.
    /// The selection is empty until a caller that knows it fills it in.
    #[error("{}", binding_message(selection, variable, reason))]
    Binding {
        /// Selection identity, empty when unknown.
        selection: String,
        /// Declared or named variable.
        variable: String,
        /// Why the binding is refused.
        reason: &'static str,
    },
}

impl Error {
    pub(crate) fn mcp_defaults(
        integration: &str,
        variables: &BTreeSet<String>,
        reason: &'static str,
    ) -> Self {
        Self::McpDefaults {
            integration: directories::bounded(integration),
            variables: variables
                .iter()
                .filter(|name| self::variables::valid_name(name))
                .take(64)
                .map(|name| directories::bounded(name))
                .collect(),
            reason,
        }
    }

    /// A binding refusal naming one variable; the selection is filled in later.
    pub(crate) fn binding(variable: &str, reason: &'static str) -> Self {
        Self::Binding {
            selection: String::new(),
            variable: directories::bounded(variable),
            reason,
        }
    }

    /// Name the selection of a binding refusal; other errors pass through.
    #[must_use]
    pub(crate) fn in_selection(self, id: &str) -> Self {
        match self {
            Self::Binding {
                variable, reason, ..
            } => Self::Binding {
                selection: directories::bounded(id),
                variable,
                reason,
            },
            other => other,
        }
    }
}

fn binding_message(selection: &str, variable: &str, reason: &str) -> String {
    if selection.is_empty() {
        format!("invalid policy: variable {variable}: {reason}")
    } else {
        format!("invalid policy: pack {selection} variable {variable}: {reason}")
    }
}

fn mcp_defaults_message(integration: &str, variables: &[String], reason: &str) -> String {
    let variables = if variables.is_empty() {
        String::new()
    } else {
        format!(" (variables: {})", variables.join(", "))
    };
    format!("invalid policy: MCP integration {integration} defaults{variables}: {reason}")
}

/// A validated permission pack. Loading it does not establish trust or activate it.
pub struct Pack {
    document: PackDocument,
    digest: String,
    index: index::Index,
    semantic: bool,
    patterns: std::sync::Arc<patterns::Store>,
    resolution: Vec<resolution::Entry>,
}

/// Validated custom rules for a local preview; declared ownership is not authenticated.
pub struct CustomRules {
    document: CustomDocument,
    digest: String,
    index: index::Index,
    semantic: bool,
    patterns: std::sync::Arc<patterns::Store>,
}

pub(crate) fn has_expiration(rules: &CustomRules) -> bool {
    rules
        .document
        .rules
        .iter()
        .any(|rule| rule.expires_at.is_some())
}

/// Literal argv and context supplied for evaluation. No shell interpretation occurs.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Literal executable selector; no basename or PATH resolution is performed.
    pub executable: String,
    /// Arguments after the executable, preserving their original boundaries.
    pub arguments: Vec<String>,
    /// Absolute working-directory context supplied by the caller.
    pub cwd: String,
    /// Absolute project context supplied by the caller.
    pub project: String,
}

/// A matching rule and whether it contributed to the resolved decision.
#[derive(Debug, Serialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent evidence flags, each serialized under its own name"
)]
pub struct Match {
    source: String,
    rule_id: String,
    decision: Decision,
    reason: String,
    specificity: u8,
    applied: bool,
    #[serde(skip)]
    tier: u8,
    branch: &'static str,
    requirement_checks: Vec<rules::Check>,
    #[serde(skip)]
    guidance: Option<rules::Guidance>,
    #[serde(skip)]
    exact_request: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    fallback: bool,
    /// A failed pack precondition, deciding its pack before rules compete.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    precondition: bool,
}

/// A read-only preview result, never an execution or activation grant.
#[derive(Debug, Serialize)]
pub struct Evaluation {
    schema_version: u8,
    mode: &'static str,
    /// Resolved policy decision.
    pub decision: Decision,
    /// Fingerprint of the selected document bytes and profile.
    pub revision: String,
    /// All matching rules, including overridden matches.
    pub matches: Vec<Match>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mapped_commands: Vec<Request>,
    requirement_checks: Vec<rules::Check>,
    guidance: Vec<serde_json::Value>,
    reason: &'static str,
    cwd: String,
    project: String,
    packs: Vec<SelectedPack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effect_evidence: Option<crate::effects::Hints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_coverage: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct SelectedPack {
    id: String,
    version: String,
    digest: String,
    profile: Profile,
}

impl Evaluation {
    pub(crate) fn constrain(&mut self, mut other: Self, request: Request) {
        self.decision = self.decision.max(other.decision);
        self.mapped_commands.push(request);
        for check in &mut other.requirement_checks {
            check.operation_ref = "mcp-command";
        }
        for matched in &mut other.matches {
            for check in &mut matched.requirement_checks {
                check.operation_ref = "mcp-command";
            }
        }
        self.matches.extend(other.matches);
        self.requirement_checks.extend(other.requirement_checks);
        self.guidance.extend(other.guidance);
        self.reason = "mapped-mcp-command";
    }
    pub(crate) fn require_review(&mut self) {
        self.decision = self.decision.max(Decision::Ask);
        self.reason = "unsupported-mcp-command-field";
    }
    pub(crate) fn fallback_only_ask(&self) -> bool {
        self.decision == Decision::Ask
            && self
                .matches
                .iter()
                .all(|matched| matched.fallback && !matched.precondition)
    }
}

impl Pack {
    pub(crate) fn mcp_effective_fields(
        &self,
        integration: &str,
    ) -> Result<&BTreeMap<String, crate::mcp_connections::EffectiveField>, Error> {
        self.document
            .mcp_integrations
            .get(integration)
            .map(|d| &d.effective_fields)
            .ok_or(Error::Contract("undeclared MCP integration"))
    }
    pub(crate) fn mcp_default_variables(
        &self,
        integration: &str,
    ) -> Result<&BTreeSet<String>, Error> {
        self.document
            .mcp_integrations
            .get(integration)
            .map(|d| &d.default_variables)
            .ok_or(Error::Contract("undeclared MCP defaults"))
    }
    pub(crate) fn mcp_commands(
        &self,
        integration: &str,
    ) -> Result<&std::collections::BTreeMap<String, crate::mcp_connections::CommandField>, Error>
    {
        self.document
            .mcp_integrations
            .get(integration)
            .map(|d| &d.command_fields)
            .ok_or(Error::Contract("undeclared MCP integration"))
    }
    pub(crate) fn mcp_default(
        &self,
        integration: &str,
    ) -> Result<crate::mcp_connections::Transport, Error> {
        let defaults = self
            .document
            .mcp_integrations
            .get(integration)
            .and_then(|d| d.defaults.as_ref())
            .ok_or(Error::Contract(
                "MCP default reference names no declared defaults",
            ))?;
        serde_json::from_value(defaults.clone())
            .map_err(|_| Error::Contract("unresolved MCP defaults"))
    }
    /// Bind MCP scopes to explicitly associated project connections. The raw
    /// artifact remains unchanged; the configuration revision owns this image.
    pub(crate) fn bind_mcp_connections(
        &mut self,
        connections: &crate::mcp_connections::Connections,
    ) -> Result<(), Error> {
        let mut bindings = std::collections::BTreeMap::<String, Vec<String>>::new();
        for (id, connection) in connections {
            let Some(name) = connection.packs.get(self.id()) else {
                continue;
            };
            let definition = self
                .document
                .mcp_integrations
                .get(name)
                .ok_or(Error::Contract(
                    "connection names an undeclared pack MCP integration",
                ))?;
            if !definition.matches(connection)? {
                return Err(Error::Contract(
                    "project MCP connection does not match its pack integration",
                ));
            }
            if connection.enabled && !connection.agents.is_empty() {
                bindings.entry(name.clone()).or_default().push(id.clone());
            }
        }
        let patterns = self.patterns.clone();
        if !self
            .document
            .rules
            .iter()
            .any(|r| r.conditions["domain"] == "mcp")
            && !self
                .document
                .preconditions
                .iter()
                .any(|p| p.conditions["domain"] == "mcp")
        {
            return Ok(());
        }
        let bind = |conditions: &mut serde_json::Value| -> Result<bool, Error> {
            if conditions["domain"] != "mcp" {
                return Ok(true);
            }
            let mut names = std::collections::BTreeSet::new();
            for (integration, ids) in &bindings {
                if selectors::matches(&conditions["server"], integration, patterns.as_ref())? {
                    names.extend(ids.iter().cloned());
                }
            }
            if names.is_empty() {
                return Ok(false);
            }
            conditions["server"] = serde_json::json!({"one_of":names});
            Ok(true)
        };
        let mut rules = Vec::new();
        for mut rule in std::mem::take(&mut self.document.rules) {
            if bind(&mut rule.conditions)? {
                rules.push(rule);
            }
        }
        self.document.rules = rules;
        let mut preconditions = Vec::new();
        for mut gate in std::mem::take(&mut self.document.preconditions) {
            if bind(&mut gate.conditions)? {
                if gate.conditions["domain"] == "mcp" {
                    let mut alternatives = Vec::new();
                    for mut alternative in std::mem::take(&mut gate.requires_any) {
                        if alternative.get("server").is_some() {
                            let mut scoped =
                                serde_json::json!({"domain":"mcp","server":alternative["server"]});
                            if bind(&mut scoped)? {
                                alternative["server"] = scoped["server"].clone();
                            } else {
                                continue;
                            }
                        }
                        alternatives.push(alternative);
                    }
                    // No remaining alternative means the active gate is unmet;
                    // it must not disappear and expose an ordinary allowance.
                    gate.requires_any = alternatives;
                }
                preconditions.push(gate);
            }
        }
        self.document.preconditions = preconditions;
        self.index = index::Index::new(self.document.rules.iter().map(rules::Rule::executable));
        self.patterns =
            model::prepare_patterns(&self.document.rules, &self.document.preconditions)?.into();
        Ok(())
    }
    /// Parse and validate a supported declarative pack document.
    ///
    /// # Errors
    /// Returns an error for malformed, incompatible, oversized, or invalid packs.
    pub fn from_yaml(input: &str) -> Result<Self, Error> {
        Self::from_yaml_bound(input, &Binding::default(), false)
    }

    /// Parse, validate and resolve a pack with one selection's variable values and
    /// overrides. With `required`, an unset required variable refuses; otherwise
    /// it is reported by [`Pack::unset_required`] for activation to refuse.
    ///
    /// # Errors
    /// Returns an error for invalid packs, bindings or overrides, and resource limits.
    pub fn from_yaml_bound(input: &str, binding: &Binding, required: bool) -> Result<Self, Error> {
        let mut document: PackDocument = parse(input)?;
        document.validate()?;
        if document.variables.values().any(|d| d.setup.is_some()) {
            variables::validate_order(&document.variables, &variables::order::read(input)?)?;
        }
        let mut digest = fingerprint(input.as_bytes());
        let mut entries = Vec::new();
        if resolution::dynamic(&document) || !binding.is_empty() {
            let resolved;
            (resolved, entries) = resolution::resolve(document, binding)?;
            if required && let Some(unset) = entries.iter().find(|e| e.kind == "variable") {
                return Err(Error::binding(
                    &unset.id,
                    "a required pack variable is unset",
                ));
            }
            document = resolved;
            document.validate_resolved()?;
            if !binding.is_empty() {
                let bound = serde_json::to_vec(binding).map_err(|_| Error::Syntax)?;
                digest = fingerprint(&[digest.as_bytes(), b"\n", &bound].concat());
            }
        }
        let index = index::Index::new(document.rules.iter().map(rules::Rule::executable));
        let semantic = document.rules.iter().any(rules::Rule::semantic)
            || document
                .preconditions
                .iter()
                .any(rules::Precondition::semantic);
        let patterns = model::prepare_patterns(&document.rules, &document.preconditions)?.into();
        Ok(Self {
            document,
            digest,
            index,
            semantic,
            patterns,
            resolution: entries,
        })
    }

    /// Required variables of this selection that have no value.
    pub fn unset_required(&self) -> impl Iterator<Item = &str> {
        self.resolution
            .iter()
            .filter(|e| e.kind == "variable")
            .map(|e| e.id.as_str())
    }

    /// How each variable-dependent or overridden entry resolved.
    pub(crate) fn resolution(&self) -> &[resolution::Entry] {
        &self.resolution
    }

    /// Validated namespaced pack identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.document.id
    }

    pub(crate) fn bind_identity(&mut self, identity: &str) {
        self.document.id = identity.into();
    }

    /// Exact validated pack version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.document.version
    }

    /// Exact artifact dependency declarations, when the pack enables dependencies-v1.
    #[must_use]
    pub fn dependencies(&self) -> &[Declaration] {
        self.document.dependencies.as_deref().unwrap_or_default()
    }
}

pub(crate) fn inspect_pack_rule(
    source: &str,
    id: &str,
) -> Result<Option<serde_json::Value>, Error> {
    let document: PackDocument = parse(source)?;
    document.validate_schema()?;
    if let Some(rule) = invocations::inspect(&document, id)? {
        return serde_json::to_value(rule)
            .map(Some)
            .map_err(|_| Error::Syntax);
    }
    document
        .preconditions
        .iter()
        .find(|p| p.id == id)
        .map(|gate| {
            let mut row = serde_json::to_value(gate).map_err(|_| Error::Syntax)?;
            row["kind"] = serde_json::json!("precondition");
            Ok(row)
        })
        .transpose()
}

/// How each variable-dependent, gated or overridden entry of one selection
/// resolves, keyed by source ID; empty for packs that resolve to themselves.
pub(crate) fn inspect_resolution(
    source: &str,
    binding: &Binding,
) -> Result<BTreeMap<String, serde_json::Value>, Error> {
    let document: PackDocument = parse(source)?;
    if !resolution::dynamic(&document) && binding.is_empty() {
        return Ok(BTreeMap::new());
    }
    let pack = Pack::from_yaml_bound(source, binding, false)?;
    let mut report = BTreeMap::new();
    for entry in pack.resolution() {
        let mut row = serde_json::to_value(entry).map_err(|_| Error::Syntax)?;
        if entry.state == "active" {
            let resolved = if entry.kind == "precondition" {
                pack.document
                    .preconditions
                    .iter()
                    .find(|p| p.id == entry.id)
                    .map(serde_json::to_value)
            } else {
                pack.document
                    .rules
                    .iter()
                    .find(|r| r.id == entry.id)
                    .map(serde_json::to_value)
            };
            if let Some(resolved) = resolved {
                row["resolved"] = resolved.map_err(|_| Error::Syntax)?;
            }
        }
        report.insert(entry.id.clone(), row);
    }
    Ok(report)
}

/// The rules, preconditions and command members a selection can override.
///
/// # Errors
/// Returns an error for documents that are not version-1 packs.
pub(crate) fn override_targets(source: &str) -> Result<Vec<resolution::targets::Target>, Error> {
    let document: PackDocument = parse(source)?;
    document.validate_schema()?;
    Ok(resolution::targets::list(&document))
}

/// A pack's variable declarations keyed by name.
///
/// # Errors
/// Returns an error for documents that are not version-1 packs.
pub(crate) fn declarations(
    source: &str,
) -> Result<BTreeMap<String, variables::Declaration>, Error> {
    let document: PackDocument = parse(source)?;
    document.validate_schema()?;
    Ok(document.variables)
}

/// A pack's variable declarations in the order its source declares them,
/// which is the order setup asks its questions.
///
/// # Errors
/// Returns an error for documents that are not version-1 packs.
pub(crate) fn ordered_declarations(
    source: &str,
) -> Result<Vec<(String, variables::Declaration)>, Error> {
    let mut declarations = declarations(source)?;
    let mut ordered: Vec<_> = variables::order::read(source)?
        .into_iter()
        .filter_map(|name| declarations.remove_entry(&name))
        .collect();
    ordered.extend(declarations);
    Ok(ordered)
}

/// Whether an entry's before/after source, profile and override relax a branch.
/// This compares entry outcomes, not the complete composed permission.
pub(crate) fn override_widens(
    before_source: &str,
    after_source: &str,
    id: &str,
    before_profile: Profile,
    after_profile: Profile,
    before: Option<&Override>,
    after: Option<&Override>,
) -> bool {
    let Ok(document) = parse::<PackDocument>(before_source) else {
        return false;
    };
    let Ok(updated) = (before_source != after_source)
        .then(|| parse::<PackDocument>(after_source))
        .transpose()
    else {
        return false;
    };
    let effect = |document: &PackDocument, profile, change: Option<&Override>| {
        let body = |rule: &rules::Rule| {
            (
                rule.decision.selected(profile),
                rule.on_unmet
                    .as_ref()
                    .map_or(Decision::Allow, |u| u.decision.selected(profile)),
            )
        };
        let original = if let Some(rule) = document.rules.iter().find(|r| r.id == id) {
            body(rule)
        } else if let Some(gate) = document.preconditions.iter().find(|p| p.id == id) {
            (Decision::Allow, gate.on_unmet.decision.selected(profile))
        } else if let Some(rule) = document.rules.iter().find(|r| {
            r.command_set.as_ref().is_some_and(|set| {
                document
                    .command_sets
                    .get(set)
                    .is_some_and(|members| members.values().any(|member| member == id))
            })
        }) {
            body(rule)
        } else {
            return (Decision::Allow, Decision::Allow);
        };
        match change {
            None => original,
            Some(Override::Off) => (Decision::Allow, Decision::Allow),
            Some(Override::Decision(decision)) => (*decision, original.1),
            Some(Override::Branches { decision, on_unmet }) => (
                decision.unwrap_or(original.0),
                on_unmet.unwrap_or(original.1),
            ),
        }
    };
    let was = effect(&document, before_profile, before);
    let now = effect(updated.as_ref().unwrap_or(&document), after_profile, after);
    now.0 < was.0 || now.1 < was.1
}

/// Drop override IDs and bound names that a pack artifact does not declare,
/// and values its declarations no longer accept.
pub(crate) fn retain_declared(source: &str, binding: &mut Binding) -> Result<(), Error> {
    let document: PackDocument = parse(source)?;
    document.validate_schema()?;
    binding.variables.retain(|name, value| {
        document
            .variables
            .get(name)
            .is_some_and(|d| d.check(value).is_ok())
    });
    binding.overrides.retain(|id, _| {
        document.rules.iter().any(|r| &r.id == id)
            || document.preconditions.iter().any(|p| &p.id == id)
            || document
                .command_sets
                .values()
                .any(|members| members.values().any(|m| m == id))
    });
    Ok(())
}

impl CustomRules {
    /// Parse and validate custom rules for preview.
    ///
    /// # Errors
    /// Returns an error for malformed rules, unsupported fields, or invalid ownership.
    pub fn from_yaml(input: &str) -> Result<Self, Error> {
        let mut document: CustomDocument = parse(input)?;
        document.validate()?;
        let index = index::Index::new(document.rules.iter().map(rules::Rule::executable));
        let semantic = document.rules.iter().any(rules::Rule::semantic);
        let patterns = model::prepare_patterns(&document.rules, &[])?.into();
        Ok(Self {
            document,
            digest: fingerprint(input.as_bytes()),
            index,
            semantic,
            patterns,
        })
    }
}

pub(crate) fn has_normalized(custom: &CustomRules) -> bool {
    custom.document.rules.iter().any(|r| r.matcher.is_none())
}

pub(crate) fn parse<T: DeserializeOwned>(input: &str) -> Result<T, Error> {
    if input.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::Limit);
    }
    // JSON artifacts need neither YAML tokenization nor its scalar inference.
    // The shared decoder rejects duplicate keys before typed deserialization.
    match crate::json::parse(input.as_bytes()) {
        Ok(value) => {
            json_budget::check(&value)?;
            return serde_json::from_value(value).map_err(|_| Error::Syntax);
        }
        Err(crate::json::Error::Trailing) if input.trim_start().starts_with(['{', '[']) => {
            return Err(Error::Syntax);
        }
        Err(crate::json::Error::Syntax | crate::json::Error::Trailing) => {}
    }
    parse_yaml(input)
}

fn parse_yaml<T: DeserializeOwned>(input: &str) -> Result<T, Error> {
    if tagged(input) {
        return Err(Error::Syntax);
    }
    let options = serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        merge_keys: serde_saphyr::MergeKeyPolicy::Error,
        strict_booleans: true,
        reject_unsupported_tags: true,
        with_snippet: false,
        budget: serde_saphyr::budget! {
            max_depth: 64,
            max_documents: 1,
            max_nodes: 16_000_000,
            max_events: 32_000_000,
            max_aliases: 0,
            max_anchors: 0,
            max_total_scalar_bytes: MAX_DOCUMENT_BYTES,
        },
    };
    serde_saphyr::from_str_with_options(input, options).map_err(|error| match error {
        serde_saphyr::Error::Budget { .. } => Error::Limit,
        _ => Error::Syntax,
    })
}

/// Whether a YAML stream tags any node. Policy refuses every tag, while the
/// decoder's unsupported-tag check still accepts core-schema tags such as
/// `!!str`. Parser errors are left to the decoder, which reports them.
fn tagged(input: &str) -> bool {
    use granit_parser::{Event, Parser};
    for next in Parser::new_from_str(input) {
        let Ok((event, _)) = next else {
            return false;
        };
        if matches!(
            event,
            Event::Scalar(_, _, _, Some(_))
                | Event::SequenceStart(_, _, Some(_))
                | Event::MappingStart(_, _, Some(_))
        ) {
            return true;
        }
    }
    false
}

/// Lowercase hexadecimal SHA-256, the digest of artifacts, sources and revisions.
pub(crate) use canonic_registry::digest::sha256_hex as fingerprint;

/// Compose validated packs and custom rules against a literal argv request.
///
/// # Errors
/// Returns an error for invalid request context, duplicate pack IDs, or input limits.
pub fn evaluate(
    packs: &[Pack],
    custom: Option<&CustomRules>,
    profile: Profile,
    request: &Request,
) -> Result<Evaluation, Error> {
    if packs.len() > 64 {
        return Err(Error::Limit);
    }
    let selected: Vec<_> = packs.iter().map(|pack| (pack, profile)).collect();
    let customs: Vec<_> = custom.into_iter().collect();
    evaluate_selected(&selected, &customs, request)
}

/// Evaluate individually selected pack profiles and one optional project custom document.
/// Uses the same authority and specificity resolver as uniform-profile previews.
/// Declared custom scopes remain unauthenticated.
///
/// # Errors
/// Rejects invalid requests, duplicate pack IDs or scoped custom-rule IDs, and limits.
pub fn evaluate_selected(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    request: &Request,
) -> Result<Evaluation, Error> {
    evaluate_selected_with(packs, customs, request, true)
}

fn evaluate_selected_with(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    request: &Request,
    indexed: bool,
) -> Result<Evaluation, Error> {
    evaluate_selected_controlled(packs, customs, request, indexed, &|| Ok(()))
}

pub(crate) fn evaluate_selected_controlled(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    request: &Request,
    indexed: bool,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Evaluation, Error> {
    evaluate_selected_context(packs, customs, request, indexed, checkpoint, true)
}

pub(crate) fn evaluate_selected_context(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    request: &Request,
    indexed: bool,
    checkpoint: &dyn Fn() -> Result<(), Error>,
    known_cwd: bool,
) -> Result<Evaluation, Error> {
    checkpoint()?;
    request.validate()?;
    if packs.len() > 64 || customs.len() > 1 {
        return Err(Error::Limit);
    }
    let evidence = semantic_selection(packs, customs)
        .then(|| crate::effects::classify(request))
        .flatten();
    evaluate_evidence(
        packs,
        customs,
        request,
        indexed,
        checkpoint,
        Knowledge {
            cwd: known_cwd,
            arguments: true,
        },
        evidence,
    )
}

#[derive(Clone, Copy)]
struct Knowledge {
    cwd: bool,
    arguments: bool,
}

fn evaluate_evidence(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    request: &Request,
    indexed: bool,
    checkpoint: &dyn Fn() -> Result<(), Error>,
    known: Knowledge,
    effect_evidence: Option<crate::effects::Hints>,
) -> Result<Evaluation, Error> {
    let input = normalized::Input::Argv {
        executable: request.executable.clone(),
        arguments: request.arguments.clone(),
        original_input: serde_json::json!({}),
    };
    let context = normalized::Context {
        working_files: None,
        cwd: request.cwd.clone(),
        environment: None,
    };
    let now = crate::evidence::now();
    let facts = rules::Facts {
        input: &input,
        context: &context,
        executable: None,
        now: &now,
        known_cwd: known.cwd,
        known_arguments: known.arguments,
        locations: Some(&resources::Unavailable),
        effects: None,
    };
    evaluate_facts(
        packs,
        customs,
        request,
        &facts,
        indexed,
        checkpoint,
        effect_evidence,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared pure resolver accepts explicit evidence and prepared pattern ownership"
)]
fn evaluate_facts(
    packs: &[(&Pack, Profile)],
    customs: &[&CustomRules],
    request: &Request,
    facts: &rules::Facts<'_>,
    indexed: bool,
    checkpoint: &dyn Fn() -> Result<(), Error>,
    effect_evidence: Option<crate::effects::Hints>,
    prepared: Option<&dyn patterns::Provider>,
) -> Result<Evaluation, Error> {
    checkpoint()?;
    dependencies::validate_packs(packs)?;
    request.validate()?;
    if packs.len() > 64 || customs.len() > 1 {
        return Err(Error::Limit);
    }
    let mut identities = BTreeSet::new();
    let facts = &rules::Facts {
        effects: effect_evidence.as_ref(),
        ..*facts
    };
    let mut revision_parts = Vec::new();
    let mut matches = Vec::new();
    let mut materialization = invocations::Budget::default();
    let prepared = prepared.map(|p| patterns::Memo::new(p, checkpoint));
    for (owner, (pack, profile)) in packs.iter().enumerate() {
        if !identities.insert(pack.id()) {
            return Err(Error::Contract("duplicate pack identity"));
        }
        revision_parts.push(format!("{}:{}:{profile:?}", pack.id(), pack.digest));
        collect_pack_matches(
            (owner, pack, *profile),
            request,
            facts,
            indexed,
            checkpoint,
            prepared.as_ref().map(|p| p as &dyn patterns::Provider),
            &mut matches,
            &mut materialization,
        )?;
    }
    for custom in customs {
        revision_parts.push(format!("custom:{}", custom.digest));
        let memo = patterns::Memo::new(custom.patterns.as_ref(), checkpoint);
        for position in
            custom
                .index
                .candidates(&request.executable, custom.document.rules.len(), indexed)
        {
            checkpoint()?;
            if let Some(matched) = custom.document.rules[position].matching(
                facts,
                prepared
                    .as_ref()
                    .map_or(&memo as &dyn patterns::Provider, |p| p),
                "custom:project",
                Profile::Recommended,
                0,
            )? {
                matches.push(matched);
            }
        }
    }
    let mut decision = resolve(&mut matches, checkpoint)?;
    let semantic =
        semantic_selection(packs, customs) && matches!(facts.input, normalized::Input::Argv { .. });
    let exact = matches
        .iter()
        .any(|m| m.exact_request && m.applied && m.decision == Decision::Allow);
    if semantic && !exact {
        decision = decision.max(Decision::Ask);
    }
    let (requirement_checks, guidance) = applied_feedback(&mut matches, decision);
    revision_parts.sort();
    Ok(Evaluation {
        schema_version: 2,
        mode: "preview",
        decision,
        revision: fingerprint(revision_parts.join("\n").as_bytes()),
        reason: if exact {
            "matched-exact-request"
        } else if semantic && decision == Decision::Ask {
            "incomplete-effect-evidence"
        } else if matches.is_empty() {
            "unmatched-normalized-request"
        } else {
            "matched-policy"
        },
        matches,
        mapped_commands: Vec::new(),
        requirement_checks,
        guidance,
        cwd: request.cwd.clone(),
        project: request.project.clone(),
        packs: selected_metadata(packs),
        effect_evidence,
        semantic_coverage: semantic.then_some("incomplete-possible-effects-v1"),
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "source pack evaluation shares request facts, provider, output and a cross-pack materialization budget"
)]
fn collect_pack_matches(
    selection: (usize, &Pack, Profile),
    request: &Request,
    facts: &rules::Facts<'_>,
    indexed: bool,
    checkpoint: &dyn Fn() -> Result<(), Error>,
    prepared: Option<&dyn patterns::Provider>,
    matches: &mut Vec<Match>,
    budget: &mut invocations::Budget,
) -> Result<(), Error> {
    let (owner, pack, profile) = selection;
    let memo = patterns::Memo::new(pack.patterns.as_ref(), checkpoint);
    let provider = prepared.unwrap_or(&memo);
    // A failed precondition decides the pack; its ordinary rules do not compete.
    let before = matches.len();
    for precondition in &pack.document.preconditions {
        checkpoint()?;
        if let Some(failed) = precondition.failure(facts, provider, pack.id(), profile)? {
            matches.push(failed);
        }
    }
    if matches.len() > before {
        return Ok(());
    }
    let mut routes = BTreeMap::new();
    for position in pack
        .index
        .candidates(&request.executable, pack.document.rules.len(), indexed)
    {
        checkpoint()?;
        let rule = &pack.document.rules[position];
        if let Some(name) = &rule.invocation {
            if rule.executable() != Some(request.executable.as_str())
                || !matches!(facts.input, normalized::Input::Argv { .. })
            {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = routes.entry(name.as_str()) {
                entry.insert(invocations::routes(
                    &pack.document.invocations[name].segments,
                    &request.arguments,
                    checkpoint,
                )?);
            }
            for candidate in invocations::matching(
                &pack.document,
                rule,
                request,
                &routes,
                budget,
                (owner, position),
            )? {
                checkpoint()?;
                let candidate = candidate?;
                if let Some(matched) = candidate.matching(facts, provider, pack.id(), profile, 1)? {
                    matches.push(matched);
                }
            }
        } else if let Some(matched) = rule.matching(facts, provider, pack.id(), profile, 1)? {
            matches.push(matched);
        }
    }
    Ok(())
}

fn applied_feedback(
    matches: &mut [Match],
    decision: Decision,
) -> (Vec<rules::Check>, Vec<serde_json::Value>) {
    let mut requirement_checks = Vec::new();
    let mut guidance = Vec::new();
    let mut ordered: Vec<_> = matches.iter_mut().collect();
    ordered.sort_by(|a, b| {
        (
            a.tier,
            std::cmp::Reverse(a.specificity),
            &a.source,
            &a.rule_id,
        )
            .cmp(&(
                b.tier,
                std::cmp::Reverse(b.specificity),
                &b.source,
                &b.rule_id,
            ))
    });
    for matched in ordered {
        for check in &mut matched.requirement_checks {
            check.applied = matched.applied;
            if check.status != "met" && matched.applied && matched.decision == decision {
                requirement_checks.push(check.clone());
            }
        }
        if matched.applied
            && matched.decision == decision
            && let Some(authored) = &matched.guidance
        {
            let rule_ref = if matched.tier == 0 {
                serde_json::json!({"scope":"custom","rule_id":matched.rule_id})
            } else {
                serde_json::json!({"scope":"pack","pack_selector":matched.source,"rule_id":matched.rule_id})
            };
            guidance.push(serde_json::json!({"rule_ref":rule_ref,"operation_ref":"request","branch":matched.branch,"message":authored.message,"alternatives":authored.alternatives.clone().unwrap_or_default(),"candidate_only":true}));
        }
    }
    (requirement_checks, guidance)
}

pub(crate) fn semantic_selection(packs: &[(&Pack, Profile)], customs: &[&CustomRules]) -> bool {
    packs.iter().any(|(pack, _)| pack.semantic) || customs.iter().any(|custom| custom.semantic)
}

fn selected_metadata(packs: &[(&Pack, Profile)]) -> Vec<SelectedPack> {
    let mut selected: Vec<_> = packs
        .iter()
        .map(|(pack, profile)| SelectedPack {
            id: pack.document.id.clone(),
            version: pack.document.version.clone(),
            digest: pack.digest.clone(),
            profile: *profile,
        })
        .collect();
    selected.sort_by(|a, b| a.id.cmp(&b.id));
    selected
}

fn resolve(
    matches: &mut [Match],
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Decision, Error> {
    let tier = matches.iter().map(|m| m.tier).min();
    // Per source (each pack, or the custom tier): the most specific rank of all
    // its matches, and of its regular (non-fallback) matches.
    let mut ranks: BTreeMap<String, (u8, Option<u8>)> = BTreeMap::new();
    for candidate in matches.iter().filter(|m| Some(m.tier) == tier) {
        checkpoint()?;
        let regular = (!candidate.fallback).then_some(candidate.specificity);
        ranks
            .entry(source(candidate))
            .and_modify(|(all, own)| {
                *all = (*all).max(candidate.specificity);
                *own = (*own).max(regular);
            })
            .or_insert((candidate.specificity, regular));
    }
    // A pack led by a fallback rule (no regular match at its top rank) yields to
    // any pack that decides with a regular rule, falling back to its own regular rules.
    let fallback_led: BTreeSet<String> = ranks
        .iter()
        .filter(|(source, (all, _))| {
            !matches.iter().any(|m| {
                Some(m.tier) == tier
                    && self::source(m) == **source
                    && !m.fallback
                    && m.specificity == *all
            })
        })
        .map(|(source, _)| source.clone())
        .collect();
    let yielding = fallback_led.len() < ranks.len();
    let mut decision = None;
    for candidate in matches.iter_mut() {
        checkpoint()?;
        let source = source(candidate);
        let applied = Some(candidate.tier) == tier
            && ranks.get(&source).is_some_and(|(all, regular)| {
                if yielding && fallback_led.contains(&source) {
                    !candidate.fallback && Some(candidate.specificity) == *regular
                } else {
                    candidate.specificity == *all
                }
            });
        if applied {
            decision = Some(decision.map_or(candidate.decision, |current: Decision| {
                current.max(candidate.decision)
            }));
        }
        candidate.applied = applied;
    }
    matches.sort_by(|a, b| (&a.source, &a.rule_id).cmp(&(&b.source, &b.rule_id)));
    checkpoint()?;
    Ok(decision.unwrap_or(Decision::Ask))
}

fn source(candidate: &Match) -> String {
    if candidate.tier == 1 {
        candidate.source.clone()
    } else {
        String::new()
    }
}

impl Request {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.arguments.len() > 256 {
            return Err(Error::Limit);
        }
        let values = std::iter::once(&self.executable)
            .chain(&self.arguments)
            .chain([&self.cwd, &self.project]);
        let mut size = 0;
        for value in values {
            size += value.len();
            if value.len() > 4096 || size > 65_536 {
                return Err(Error::Limit);
            }
            if value.contains('\0') {
                return Err(Error::Contract("NUL in request"));
            }
        }
        if self.executable.is_empty()
            || !std::path::Path::new(&self.cwd).is_absolute()
            || !std::path::Path::new(&self.project).is_absolute()
        {
            return Err(Error::Contract(
                "executable and absolute context are required",
            ));
        }
        Ok(())
    }
}
