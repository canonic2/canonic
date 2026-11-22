//! Validates declarative project policy and composes decisions without I/O.
//!
//! The pure policy core is grouped by family, each a self-named module:
//! - `model`: version-1 documents, schema, templates, variables, legacy forms.
//! - `rules`: structured predicates, argv grammars, patterns, selectors,
//!   directory options and portability.
//! - `resolution`: compiled plans, executable index, partitions, provenance
//!   and resolver evidence feeding the Allow/Ask/Deny decision.
//! - `dependencies`: pack dependencies and profiles as ordered levels.

pub(crate) mod dependencies;
#[cfg(test)]
mod historical_tests;
pub(crate) mod model;
pub(crate) use model::feedback;
pub(crate) mod resolution;
pub(crate) mod rules;
#[cfg(test)]
mod tests;

use model::{json_budget, legacy, variables};
use resolution::{diagnostics, index, invocations, normalized, provenance};
use rules::{directories, patterns, resources, selectors};

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
    "url-subdomains-v1",
    "local-http-requests-v1",
    "docker-isolated-bind-v1",
    "historical-matchers-v1",
    "historical-normalized-v1",
    "argument-rules-v1",
    "argument-paths-v1",
    "graphql-v1",
    "graphql-mutations-v1",
    "graphql-subtree-v1",
    "field-paths-v1",
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
    "variable-parts-v1",
    "variable-prefixes-v1",
    "variable-list-branches-v1",
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
    rules::INTERLEAVED_LEADING,
    rules::FIELD_NEGATION,
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
    /// Candidate selections and their supplied pack artifacts do not agree.
    #[error("candidate pack identity or version mismatch")]
    ArtifactMismatch,
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
    /// A variable or template refusal located in one source entry and field.
    /// Only bounded identifiers and a static reason are retained, never values.
    #[error("{}", resolution_message(selection, entry, field, variables, reason))]
    Resolution {
        /// Selection identity, empty until supplied by the composition owner.
        selection: String,
        /// Source rule, precondition or MCP integration identity.
        entry: String,
        /// Authored field that failed preparation.
        field: String,
        /// Referenced variable names; invalid names are omitted.
        variables: Vec<String>,
        /// Fixed diagnostic reason.
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
            Self::Resolution {
                entry,
                field,
                variables,
                reason,
                ..
            } => Self::Resolution {
                selection: directories::bounded(id),
                entry,
                field,
                variables,
                reason,
            },
            other => other,
        }
    }

    /// Locate a semantic resolution refusal without changing syntax, limit or
    /// interruption errors. An exact offending binding takes precedence over
    /// the enclosing field's referenced names.
    pub(crate) fn in_resolution(self, entry: &str, field: &str, names: &BTreeSet<String>) -> Self {
        let (selection, variables, reason) = match self {
            Self::Contract(reason) => (String::new(), names.iter().cloned().collect(), reason),
            Self::Binding {
                selection,
                variable,
                reason,
            } => (selection, vec![variable], reason),
            other => return other,
        };
        Self::Resolution {
            selection,
            entry: if rules::valid_id(entry) || crate::mcp_connections::valid_name(entry) {
                directories::bounded(entry)
            } else {
                "<invalid>".into()
            },
            field: field
                .chars()
                .take(256)
                .filter(|c| !c.is_control())
                .collect(),
            variables: variables
                .into_iter()
                .filter(|name| self::variables::valid_name(name))
                .take(64)
                .collect(),
            reason,
        }
    }
}

fn resolution_message(
    selection: &str,
    entry: &str,
    field: &str,
    variables: &[String],
    reason: &str,
) -> String {
    let selection = if selection.is_empty() {
        String::new()
    } else {
        format!("pack {selection} ")
    };
    let variables = if variables.is_empty() {
        String::new()
    } else {
        format!(" (variables: {})", variables.join(", "))
    };
    format!("invalid policy: {selection}entry {entry} field {field}{variables}: {reason}")
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
#[derive(Clone)]
pub struct Pack {
    comparison_source: Option<feedback::SourceIdentity>,
    document: PackDocument,
    digest: String,
    index: index::Index,
    semantic: bool,
    historical_semantic: bool,
    patterns: std::sync::Arc<patterns::Store>,
    resolution: Vec<resolution::Entry>,
    origins: BTreeMap<String, provenance::Original>,
}

/// Validated custom rules for a local preview; declared ownership is not authenticated.
#[derive(Clone)]
pub struct CustomRules {
    pub(crate) comparison_sources: Option<Vec<feedback::SourceIdentity>>,
    document: CustomDocument,
    digest: String,
    index: index::Index,
    semantic: bool,
    historical_semantic: bool,
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
#[derive(Clone, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent evidence flags, each serialized under its own name"
)]
pub struct Match {
    pub(crate) source: String,
    pub(crate) rule_id: String,
    pub(crate) decision: Decision,
    pub(crate) reason: String,
    specificity: u8,
    pub(crate) applied: bool,
    pub(crate) yielded_reason: Option<&'static str>,
    tier: u8,
    branch: &'static str,
    requirement_checks: Vec<rules::Check>,
    guidance: Option<rules::Guidance>,
    exact_request: bool,
    legacy: bool,
    fallback: bool,
    /// A failed pack precondition, deciding its pack before rules compete.
    precondition: bool,
    decision_provenance: Option<provenance::Evidence>,
    pub(crate) operation_ref: Option<String>,
    pub(crate) executable: diagnostics::TextField,
    rule_ref: Option<diagnostics::RuleReference>,
    private_input: bool,
}

impl Match {
    fn pack_default(&self, approved: &impl Fn(&str) -> bool) -> bool {
        self.tier == 1
            && approved(&self.source)
            && self
                .decision_provenance
                .as_ref()
                .is_some_and(provenance::Evidence::unchanged_default)
    }
}

/// A read-only preview result, never an execution or activation grant.
#[derive(Debug)]
pub struct Evaluation {
    schema_version: u8,
    mode: &'static str,
    /// Resolved policy decision.
    pub decision: Decision,
    /// Fingerprint of the selected document bytes and profile.
    pub revision: String,
    /// All matching rules, including overridden matches.
    pub matches: Vec<Match>,
    mapped_commands: Vec<Request>,
    pub(crate) requirement_checks: Vec<rules::Check>,
    pub(crate) guidance: Vec<diagnostics::GuidanceRecord>,
    pub(crate) reason: &'static str,
    cwd: String,
    project: String,
    packs: Vec<SelectedPack>,
    effect_evidence: Option<crate::effects::Hints>,
    semantic_coverage: Option<&'static str>,
    private_input: bool,
}

#[derive(Debug, Serialize)]
struct SelectedPack {
    id: String,
    version: String,
    digest: String,
    profile: Profile,
}

impl Evaluation {
    pub(crate) fn mark_private_input(&mut self) {
        self.private_input = true;
        for matched in &mut self.matches {
            matched.private_input = true;
            for check in &mut matched.requirement_checks {
                check.private_input = true;
            }
        }
        for check in &mut self.requirement_checks {
            check.private_input = true;
        }
        for guidance in &mut self.guidance {
            guidance.private_input = true;
        }
    }
    pub(crate) fn annotate_rule_references(&mut self, selectors: &BTreeMap<String, String>) {
        for matched in &mut self.matches {
            let mut reference = diagnostics::RuleReference::for_rule(
                &matched.source,
                &matched.rule_id,
                matched.tier,
            );
            reference.annotate(selectors);
            matched.rule_ref = Some(reference);
            for check in &mut matched.requirement_checks {
                check.rule_ref.annotate(selectors);
            }
        }
        for check in &mut self.requirement_checks {
            check.rule_ref.annotate(selectors);
        }
        for guidance in &mut self.guidance {
            if let Some(reference) = &mut guidance.rule_ref {
                reference.annotate(selectors);
            }
        }
    }
    pub(crate) fn constrain(&mut self, mut other: Self, request: Request) {
        self.decision = self.decision.max(other.decision);
        self.mapped_commands.push(request);
        for check in &mut other.requirement_checks {
            check.operation_ref = "mcp-command".into();
        }
        for matched in &mut other.matches {
            for check in &mut matched.requirement_checks {
                check.operation_ref = "mcp-command".into();
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

    pub(crate) fn allows_docker_context_runtime(&self, approved: impl Fn(&str) -> bool) -> bool {
        self.decision == Decision::Allow
            && self.matches.iter().any(|matched| {
                matched.applied
                    && matched.branch == "met"
                    && matched.decision == Decision::Allow
                    && matched.pack_default(&approved)
                    && matches!(
                        matched.rule_id.as_str(),
                        "exec-context"
                            | "container-exec-context"
                            | "run-context"
                            | "container-run-context"
                            | "create-context"
                            | "container-create-context"
                    )
            })
    }

    pub(crate) fn stock_fetch_review(&self, approved: impl Fn(&str) -> bool) -> bool {
        self.semantic_coverage.is_none()
            && !self.matches.is_empty()
            && self.fallback_only_ask()
            && self.matches.iter().all(|matched| {
                matched.rule_id == "shared-review-curl"
                    && matched.decision == Decision::Ask
                    && matched.pack_default(&approved)
            })
    }

    pub(crate) fn allow_container_client_help(&mut self) {
        self.decision = Decision::Allow;
        self.reason = "checked-docker-client-help";
        self.yield_matches("container-client-help");
        self.requirement_checks.clear();
        self.guidance.clear();
    }

    pub(crate) fn retain_container_fetch_review(&mut self, mut argv: Self) {
        argv.yield_matches("container-web-policy");
        self.matches.extend(argv.matches);
        if self.decision == Decision::Allow {
            self.reason = "container-web-policy";
        }
    }

    fn yield_matches(&mut self, reason: &'static str) {
        for matched in &mut self.matches {
            matched.applied = false;
            matched.yielded_reason = Some(reason);
            for check in &mut matched.requirement_checks {
                check.applied = false;
            }
        }
    }
}

impl Pack {
    /// Estimated cache charge, including the resolved document, resolution
    /// evidence, bound values and prepared matcher allocation.
    pub(crate) fn cache_bytes(
        &self,
        binding: &Binding,
        source: usize,
        limit: usize,
    ) -> Option<usize> {
        let base = source
            .saturating_add(std::mem::size_of::<Self>() + std::mem::size_of::<Binding>())
            .saturating_add(self.patterns.prepared_bytes());
        let data = crate::json::retained_len(
            &(&self.document, &self.resolution, &self.origins, binding),
            limit.checked_sub(base)?,
        )?;
        Some(base + data)
    }
    pub(crate) fn has_mcp_integrations(&self) -> bool {
        !self.document.mcp_integrations.is_empty()
    }
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
    /// Validate explicit constraints without discarding rules needed by servers
    /// configured after activation.
    pub(crate) fn validate_mcp_connections(
        &self,
        connections: &crate::mcp_connections::Connections,
    ) -> Result<(), Error> {
        for connection in connections.values() {
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
        }
        Ok(())
    }

    /// Materialize only selected MCP postings using request-owned matches and
    /// the compiled pattern provider. No parsing or pattern compilation occurs.
    pub(crate) fn bind_mcp_scopes(
        &mut self,
        bindings: &BTreeMap<String, Vec<String>>,
        patterns: &dyn patterns::Provider,
    ) -> Result<(), Error> {
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
            for (integration, ids) in bindings {
                if selectors::matches(&conditions["server"], integration, patterns)? {
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
        Self::from_yaml_reusing(input, binding, required, None)
    }

    /// Reuse only exact matcher programs; all source/binding validation still runs.
    pub(crate) fn from_yaml_reusing(
        input: &str,
        binding: &Binding,
        required: bool,
        previous: Option<&Self>,
    ) -> Result<Self, Error> {
        Self::prepare_yaml(input, binding, required, previous, false)
    }

    pub(crate) fn from_retained_yaml(input: &str) -> Result<Self, Error> {
        Self::from_retained_yaml_reusing(input, &Binding::default(), false, None)
    }

    pub(crate) fn from_retained_yaml_reusing(
        input: &str,
        binding: &Binding,
        required: bool,
        previous: Option<&Self>,
    ) -> Result<Self, Error> {
        Self::prepare_yaml(input, binding, required, previous, true)
    }

    fn prepare_yaml(
        input: &str,
        binding: &Binding,
        required: bool,
        previous: Option<&Self>,
        retained: bool,
    ) -> Result<Self, Error> {
        let _trace = crate::diagnostics::trace::span("pack.prepare");
        let mut document: PackDocument = if retained {
            legacy::pack(input)?
        } else {
            let _trace = crate::diagnostics::trace::span("pack.decode");
            parse(input)?
        };
        if !document.retained_legacy {
            let _trace = crate::diagnostics::trace::span("pack.validate_source");
            document.validate()?;
        }
        if document.variables.values().any(|d| d.setup.is_some()) {
            variables::validate_order(&document.variables, &variables::order::read(input)?)?;
        }
        let comparison_source = feedback::source_pack(&document);
        let mut digest = fingerprint(input.as_bytes());
        let origins = provenance::capture(&document, &binding.overrides);
        let mut entries = Vec::new();
        if resolution::dynamic(&document) || !binding.is_empty() {
            let _trace = crate::diagnostics::trace::span("pack.resolve");
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
        let patterns = {
            let _trace = crate::diagnostics::trace::span("pack.patterns");
            let mut store = model::prepare_patterns_reusing(
                &document.rules,
                &document.preconditions,
                previous.map(|pack| pack.patterns.as_ref()),
            )?;
            for integration in document.mcp_integrations.values() {
                store.collect_reusing(
                    &integration.fields,
                    "mcp",
                    previous.map(|pack| pack.patterns.as_ref()),
                )?;
            }
            store.into()
        };
        let historical_semantic = historical_semantic_rules(&document.rules);
        Ok(Self {
            comparison_source,
            document,
            digest,
            index,
            semantic,
            historical_semantic,
            patterns,
            resolution: entries,
            origins,
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
    #[cfg(test)]
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

    pub(crate) fn bind_override_layers(
        &mut self,
        local_selection: bool,
        project: &BTreeMap<String, Override>,
        local: Option<&BTreeMap<String, Override>>,
    ) {
        provenance::layers(&mut self.origins, local_selection, project, local);
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

fn retained_pack_document(source: &str) -> Result<PackDocument, Error> {
    legacy::pack(source)
}

fn retained_pack_display_document(source: &str) -> Result<PackDocument, Error> {
    let envelope: serde_json::Value = parse(source)?;
    if envelope["schema_version"] != 1 || envelope["kind"] != "pack" {
        return Err(Error::Contract(
            "historical pack display is unsupported without preparing policy; inspect or edit the pack file explicitly",
        ));
    }
    let document: PackDocument = parse(source)?;
    document.validate_schema()?;
    Ok(document)
}

/// Materialize a retained custom source for an ordinary reviewed policy edit.
/// Automatic checkpoint loading keeps the original source bytes instead.
pub(crate) fn normalize_retained_custom(source: &str) -> Result<serde_json::Value, Error> {
    let document = legacy::custom(source)?;
    serde_json::to_value(document).map_err(|_| Error::Syntax)
}

/// Display already captured current source without validating/preparing its
/// matchers again. Historical formats report unsupported display explicitly.
pub(crate) fn inspect_retained_custom(source: &str) -> Result<serde_json::Value, Error> {
    let value: serde_json::Value = parse(source)?;
    if value["schema_version"] == 1 && value["kind"] == "custom" {
        let document: model::CustomDocument =
            serde_json::from_value(value).map_err(|_| Error::Syntax)?;
        document.validate_schema()?;
        serde_json::to_value(document).map_err(|_| Error::Syntax)
    } else {
        Err(Error::Contract(
            "historical custom display is unsupported without preparing policy; inspect or edit the rules file explicitly",
        ))
    }
}

pub(crate) fn inspect_pack_rule(
    source: &str,
    id: &str,
) -> Result<Option<serde_json::Value>, Error> {
    let document = retained_pack_display_document(source)?;
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
    let document = retained_pack_display_document(source)?;
    document.validate_schema()?;
    if !resolution::dynamic(&document) && binding.is_empty() {
        return Ok(BTreeMap::new());
    }
    // These exact source bytes were already reviewed. Display resolution is
    // descriptive: resolving variables must not build or validate matchers.
    let _trace = crate::diagnostics::trace::span("pack.inspect_resolution");
    crate::diagnostics::trace::count("pack.inspection_resolutions", 1);
    let (document, entries) = resolution::resolve(document, binding)?;
    let mut report = BTreeMap::new();
    for entry in entries {
        let mut row = serde_json::to_value(&entry).map_err(|_| Error::Syntax)?;
        if entry.state == "active" {
            let resolved = if entry.kind == "precondition" {
                document
                    .preconditions
                    .iter()
                    .find(|p| p.id == entry.id)
                    .map(serde_json::to_value)
            } else {
                document
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
    let document = retained_pack_document(source)?;
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
    let document = retained_pack_document(source)?;
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
    // Reactivation restores a ranked contribution. An absent entry has no
    // decision, so treating `off` as Allow misses Ask/Allow replacing a broader
    // restriction. Only the composed comparison can establish its actual effect.
    if matches!(before, Some(Override::Off)) && !matches!(after, Some(Override::Off)) {
        return true;
    }
    let Ok(document) = retained_pack_document(before_source) else {
        return false;
    };
    let Ok(updated) = (before_source != after_source)
        .then(|| retained_pack_document(after_source))
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
    let document = retained_pack_document(source)?;
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
    pub(crate) fn normalized_document(&self) -> Result<serde_json::Value, Error> {
        serde_json::to_value(&self.document).map_err(|_| Error::Syntax)
    }

    pub(crate) fn from_rule_values(values: &[serde_json::Value]) -> Result<Self, Error> {
        let rules = values
            .iter()
            .cloned()
            .map(serde_json::from_value)
            .collect::<Result<Vec<rules::Rule>, _>>()
            .map_err(|_| Error::Contract("invalid session selector"))?;
        let document = CustomDocument::from_rules(rules)?;
        let index = index::Index::new(document.rules.iter().map(rules::Rule::executable));
        let semantic = document.rules.iter().any(rules::Rule::semantic);
        let historical_semantic = historical_semantic_rules(&document.rules);
        let patterns = model::prepare_patterns(&document.rules, &[])?.into();
        let comparison_sources = feedback::source_custom(&document).map(|source| vec![source]);
        Ok(Self {
            comparison_sources,
            document,
            digest: fingerprint(
                serde_json::to_string(values)
                    .map_err(|_| Error::Contract("invalid selector encoding"))?
                    .as_bytes(),
            ),
            index,
            semantic,
            historical_semantic,
            patterns,
        })
    }
    /// Evaluate retained session selectors through the ordinary prepared resolver.
    pub(crate) fn evaluate_observed(&self, request: &Request) -> Result<Evaluation, Error> {
        let input = normalized::Input::Argv {
            executable: request.executable.clone(),
            arguments: request.arguments.clone(),
            original_input: serde_json::json!({}),
        };
        let context = normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        };
        let observer = crate::location::Observer::new(&input, &request.project, &request.cwd, None);
        let now = crate::evidence::now();
        let facts = rules::Facts {
            input: &input,
            context: &context,
            executable: None,
            now: &now,
            known_cwd: true,
            known_arguments: true,
            locations: Some(&observer),
            container_read: None,
            effects: None,
        };
        evaluate_facts(&[], &[self], request, &facts, true, &|| Ok(()), None, None)
    }

    /// Parse and validate custom rules for preview.
    ///
    /// # Errors
    /// Returns an error for malformed rules, unsupported fields, or invalid ownership.
    pub fn from_yaml(input: &str) -> Result<Self, Error> {
        let mut document: CustomDocument = parse(input)?;
        document.validate()?;
        Self::prepare_yaml(input, document)
    }

    pub(crate) fn from_retained_yaml(input: &str) -> Result<Self, Error> {
        Self::prepare_yaml(input, legacy::custom(input)?)
    }

    fn prepare_yaml(input: &str, document: CustomDocument) -> Result<Self, Error> {
        let index = index::Index::new(document.rules.iter().map(rules::Rule::executable));
        let semantic = document.rules.iter().any(rules::Rule::semantic);
        let historical_semantic = historical_semantic_rules(&document.rules);
        let patterns = model::prepare_patterns(&document.rules, &[])?.into();
        let comparison_sources = feedback::source_custom(&document).map(|source| vec![source]);
        Ok(Self {
            comparison_sources,
            document,
            digest: fingerprint(input.as_bytes()),
            index,
            semantic,
            historical_semantic,
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
    // Every explicit YAML tag starts with `!`. Without it, the ordinary
    // decoder can validate the document without a second tokenization pass.
    if !input.contains('!') {
        return false;
    }
    crate::diagnostics::trace::count("policy.tag_scans", 1);
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
pub(crate) use canonic_registry_kit::digest::sha256_hex as fingerprint;

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
        container_read: None,
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
    let unused = patterns::Store::default();
    let shared = patterns::Memo::new(prepared.unwrap_or(&unused), checkpoint);
    let prepared = prepared.map(|_| &shared as &dyn patterns::Provider);
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
            prepared,
            &shared,
            &mut matches,
            &mut materialization,
        )?;
    }
    for custom in customs {
        revision_parts.push(format!("custom:{}", custom.digest));
        let memo = patterns::Memo::sharing(custom.patterns.as_ref(), checkpoint, &shared);
        for position in
            custom
                .index
                .candidates(&request.executable, custom.document.rules.len(), indexed)
        {
            checkpoint()?;
            if let Some(matched) = custom.document.rules[position].matching(
                request,
                facts,
                prepared.unwrap_or(&memo),
                "custom:project",
                Profile::Recommended,
                0,
            )? {
                matches.push(matched);
            }
        }
    }
    let mut decision = resolve(&mut matches, checkpoint)?;
    if rules::legacy_read(request, facts) {
        decision = constrain_current_read(&mut matches, decision, checkpoint)?;
    }
    let historical_semantic = historical_semantic_selection(packs, customs);
    let semantic = semantic_selection(packs, customs)
        && matches!(facts.input, normalized::Input::Argv { .. })
        || historical_semantic && rules::legacy_read(request, facts);
    let exact = matches
        .iter()
        .any(|m| m.exact_request && m.applied && m.decision == Decision::Allow);
    if semantic && (!exact || rules::legacy_read(request, facts)) {
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
        private_input: false,
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
    shared: &dyn patterns::Provider,
    matches: &mut Vec<Match>,
    budget: &mut invocations::Budget,
) -> Result<(), Error> {
    let (owner, pack, profile) = selection;
    let memo = patterns::Memo::sharing(pack.patterns.as_ref(), checkpoint, shared);
    let provider = prepared.unwrap_or(&memo);
    // A failed precondition decides the pack; its ordinary rules do not compete.
    let before = matches.len();
    for precondition in &pack.document.preconditions {
        checkpoint()?;
        if let Some(failed) = precondition.failure(facts, provider, pack.id(), profile)? {
            let mut failed = failed;
            failed.decision_provenance = Some(provenance::evidence(
                pack.origins.get(&precondition.id),
                profile,
                true,
                failed.decision,
            ));
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
                if let Some(mut matched) =
                    candidate.matching(request, facts, provider, pack.id(), profile, 1)?
                {
                    matched.decision_provenance = Some(provenance::evidence(
                        pack.origins.get(&rule.id),
                        profile,
                        matched.branch == "unmet",
                        matched.decision,
                    ));
                    matches.push(matched);
                }
            }
            continue;
        }
        let read;
        let facts = if rule.id.starts_with("read-")
            && !pack.origins.contains_key(&rule.id)
            && facts
                .container_read
                .is_some_and(|approved| approved(pack.id()))
        {
            read = rules::Facts {
                locations: Some(&rules::resources::ContainerRead),
                ..*facts
            };
            &read
        } else {
            facts
        };
        if let Some(mut matched) = rule.matching(request, facts, provider, pack.id(), profile, 1)? {
            matched.decision_provenance = Some(provenance::evidence(
                pack.origins.get(&rule.id),
                profile,
                matched.branch == "unmet",
                matched.decision,
            ));
            matches.push(matched);
        }
    }
    Ok(())
}

fn applied_feedback(
    matches: &mut [Match],
    decision: Decision,
) -> (Vec<rules::Check>, Vec<diagnostics::GuidanceRecord>) {
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
            guidance.push(diagnostics::GuidanceRecord {
                alternatives: authored.alternatives.clone().unwrap_or_default(),
                branch: Some(matched.branch),
                candidate_only: true,
                message: authored.message.clone(),
                operation_ref: "request".into(),
                rule_ref: Some(diagnostics::RuleReference::for_rule(
                    &matched.source,
                    &matched.rule_id,
                    matched.tier,
                )),
                source: None,
                request: None,
                fetch_url: diagnostics::TextField::Absent,
                private_input: false,
            });
        }
    }
    (requirement_checks, guidance)
}

pub(crate) fn semantic_selection(packs: &[(&Pack, Profile)], customs: &[&CustomRules]) -> bool {
    packs.iter().any(|(pack, _)| pack.semantic) || customs.iter().any(|custom| custom.semantic)
}

fn historical_semantic_selection(packs: &[(&Pack, Profile)], customs: &[&CustomRules]) -> bool {
    packs.iter().any(|(pack, _)| pack.historical_semantic)
        || customs.iter().any(|custom| custom.historical_semantic)
}

fn historical_semantic_rules(rules: &[rules::Rule]) -> bool {
    rules.iter().any(|rule| {
        rule.legacy
            .as_ref()
            .is_some_and(|legacy| legacy.matcher.semantic())
            || rule.legacy_normalized.is_some() && rule.semantic()
    })
}

/// Current typed file restrictions still constrain a retained argv read allow.
/// An empty current selection supplies no policy decision of its own.
fn constrain_current_read(
    matches: &mut [Match],
    decision: Decision,
    checkpoint: &dyn Fn() -> Result<(), Error>,
) -> Result<Decision, Error> {
    let mut current: Vec<_> = matches
        .iter()
        .filter(|matched| !matched.legacy)
        .cloned()
        .collect();
    if current.is_empty() {
        return Ok(decision);
    }
    let restriction = resolve(&mut current, checkpoint)?;
    for selected in current.iter().filter(|matched| matched.applied) {
        if let Some(original) = matches.iter_mut().find(|matched| {
            !matched.legacy
                && matched.source == selected.source
                && matched.rule_id == selected.rule_id
        }) {
            original.applied = true;
        }
    }
    Ok(decision.max(restriction))
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
