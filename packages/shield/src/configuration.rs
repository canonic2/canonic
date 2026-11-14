//! Validates project configuration and reproducible retained artifact locks.

use crate::policy::{self, Binding, CustomRules, Error, Pack, Profile};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

mod composition;
mod connections;
pub(crate) mod layers;
pub(crate) mod settings;
pub use composition::{Effective, compose};

/// Render an immutable retained bundle's canonical lock without constructing
/// policy matchers. The caller verifies its source object; this grants no trust.
pub(crate) fn recorded_effective(
    candidate: &serde_json::Value,
    expected: &str,
) -> Result<Effective, Error> {
    recorded_composition(candidate)?.for_revision(expected)
}

pub(crate) fn recorded_revision(candidate: &serde_json::Value) -> Result<String, Error> {
    recorded_composition(candidate)?.revision()
}

/// Verify only the source fields present in a partial display against the
/// canonical recorded lock. This never verifies an omitted bundle field.
pub(crate) fn verify_recorded_fields(
    candidate: &serde_json::Value,
    expected: &str,
) -> Result<Effective, Error> {
    let effective = recorded_effective(candidate, expected)?;
    let lock: LockDocument =
        serde_json::from_value(candidate["lock"].clone()).map_err(|_| Error::Syntax)?;
    for (field, artifact) in [("rules", &lock.rules), ("local_rules", &lock.local_rules)] {
        if let Some(value) = candidate.get(field) {
            match (value.as_str(), artifact) {
                (Some(source), Some(artifact))
                    if policy::fingerprint(source.as_bytes()) == artifact.digest => {}
                (None, None) if value.is_null() => {}
                _ => return Err(Error::ArtifactMismatch),
            }
        }
    }
    if let Some(config) = candidate.get("config") {
        let config = config.as_str().ok_or(Error::Syntax)?;
        let local = match candidate.get("local_config") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => Some(value.as_str().ok_or(Error::Syntax)?),
        };
        let merged = layers::merge(config, local)?;
        if settings::config_digest(&merged)? != lock.config_digest {
            return Err(Error::ArtifactMismatch);
        }
        let selection = Candidate::from_yaml(&merged)?;
        if let Some(packs) = candidate.get("packs") {
            let packs = packs.as_array().ok_or(Error::Syntax)?;
            if packs.len() != selection.packs.len() {
                return Err(Error::ArtifactMismatch);
            }
            for (source, selected) in packs.iter().zip(&selection.packs) {
                if source.is_null() {
                    continue;
                }
                let selected =
                    serde_json::to_value(selected.locked(source.as_str().ok_or(Error::Syntax)?))
                        .map_err(|_| Error::Syntax)?;
                if !lock
                    .packs
                    .iter()
                    .any(|pack| serde_json::to_value(pack).ok().as_ref() == Some(&selected))
                {
                    return Err(Error::ArtifactMismatch);
                }
            }
        }
    } else if candidate.get("packs").is_some() {
        return Err(Error::ArtifactMismatch);
    }
    Ok(effective)
}

fn recorded_composition(candidate: &serde_json::Value) -> Result<Effective, Error> {
    if candidate["schema_version"] != 1
        || candidate["kind"] != "candidate-bundle"
        || candidate["trust"] != "unverified"
    {
        return Err(Error::Contract("invalid retained bundle metadata"));
    }
    let document: LockDocument =
        serde_json::from_value(candidate["lock"].clone()).map_err(|_| Error::Syntax)?;
    if document.schema_version != 1 || document.kind != "local-candidate" {
        return Err(Error::Contract("invalid retained lock metadata"));
    }
    compose(&[(policy::Scope::Project, Lock { document })])
}
mod evaluation;
pub(crate) use evaluation::NormalizedEvaluation;
pub use evaluation::{CandidateEvaluation, Evaluator, evaluate};
mod bundle;
pub use bundle::{Bundle, MAX_BUNDLE_BYTES};
mod comparison;
pub use comparison::{Change, Comparison, compare};
mod domain_comparison;
pub use domain_comparison::{DomainComparison, Relation, compare_argv, compare_policy};

/// Validated artifacts retained in memory with their exact candidate lock.
/// Construction captures no trust, approval, or atomic filesystem revision.
#[derive(Clone)]
pub struct Snapshot {
    lock: Lock,
    packs: Vec<Pack>,
    custom: Option<CustomRules>,
}

impl Snapshot {
    /// Metadata bound to the same artifact contents retained for evaluation.
    #[must_use]
    pub fn lock(&self) -> &Lock {
        &self.lock
    }
}

/// Untrusted project configuration, without discovery or authorization.
pub struct Candidate {
    packs: Vec<Selection>,
    rules: Option<String>,
    local_rules: Option<String>,
    disabled: Vec<Selection>,
    digest: String,
    access_paths: AccessPaths,
    mcp: crate::mcp_connections::Connections,
}

/// Operator-owned additional roots; the project is always readable and writable.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AccessPaths {
    #[serde(default, deserialize_with = "sequence")]
    pub(crate) read: Vec<String>,
    #[serde(default, deserialize_with = "sequence")]
    pub(crate) write: Vec<String>,
}

impl AccessPaths {
    fn is_empty(&self) -> bool {
        self.read.is_empty() && self.write.is_empty()
    }

    fn validate(&self) -> Result<(), Error> {
        if self.read.len() + self.write.len() > 64 {
            return Err(Error::Limit);
        }
        for paths in [&self.read, &self.write] {
            let mut seen = BTreeSet::new();
            for path in paths {
                if path.len() > 4096 {
                    return Err(Error::Limit);
                }
                if !std::path::Path::new(path).is_absolute()
                    || path.contains('\0')
                    || path
                        .split(std::path::is_separator)
                        .any(|part| matches!(part, "." | ".."))
                    || !seen.insert(path)
                {
                    return Err(Error::Contract(
                        "access paths require unique absolute roots without traversal",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u8,
    #[serde(deserialize_with = "sequence")]
    packs: Vec<Selection>,
    #[serde(default, deserialize_with = "optional_path")]
    rules: Option<String>,
    #[serde(default)]
    access_paths: AccessPaths,
    #[serde(default, deserialize_with = "optional_path")]
    local_rules: Option<String>,
    #[serde(default)]
    local_access_paths: AccessPaths,
    #[serde(default, deserialize_with = "sequence")]
    disabled: Vec<Selection>,
    #[serde(default)]
    mcp: crate::mcp_connections::Connections,
    #[serde(default)]
    local_mcp: crate::mcp_connections::Connections,
    #[serde(default)]
    enforcement: Option<settings::Enforcement>,
    #[serde(default)]
    protection: Option<settings::Protection>,
    #[serde(default)]
    ask_fallback: BTreeMap<String, settings::Fallback>,
    #[serde(default)]
    history: Option<settings::History>,
    #[serde(default)]
    registries: Option<canonic_registry_kit::registrations::Registrations>,
    /// The local layer's settings, which the layer merge carries here.
    #[serde(default)]
    local_settings: Option<settings::Layer>,
}

/// Marks a selection the local layer adds.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Layer {
    Local,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    id: String,
    #[serde(default)]
    registry_identity: Option<String>,
    version: String,
    profile: Profile,
    path: String,
    #[serde(default)]
    variables: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    overrides: BTreeMap<String, policy::Override>,
    #[serde(default)]
    layer: Option<Layer>,
    #[serde(default)]
    local: Option<layers::Overlay>,
}

impl Selection {
    /// Shared values and overrides with the local overlay applied.
    fn binding(&self) -> Binding {
        let mut binding = Binding {
            variables: self.variables.clone(),
            overrides: self.overrides.clone(),
        };
        if let Some(overlay) = &self.local {
            overlay.apply(&mut binding);
        }
        binding
    }

    fn validate(&self) -> Result<(), Error> {
        let segments: Vec<_> = self.id.split('/').collect();
        if segments.len() != 2 || !segments.iter().all(|part| identifier(part)) {
            return Err(Error::Contract("pack ID must be namespace/name"));
        }
        if self.registry_identity.as_ref().is_some_and(|id| {
            id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        }) {
            return Err(Error::Contract("invalid registry identity"));
        }
        if self.layer.is_some() && self.local.is_some() {
            return Err(Error::Contract(
                "local-only selections hold their own variables and overrides",
            ));
        }
        if self.version.len() > 4096 {
            return Err(Error::Limit);
        }
        semver::Version::parse(&self.version)
            .map_err(|_| Error::Contract("configuration requires an exact pack version"))?;
        portable_path(&self.path)
    }

    fn locked(&self, bytes: &str) -> LockedPack {
        LockedPack {
            id: self.registry_identity.as_ref().map_or_else(
                || self.id.clone(),
                |registry| format!("{registry}/{}", self.id),
            ),
            version: self.version.clone(),
            profile: self.profile,
            artifact: Artifact {
                path: self.path.clone(),
                digest: policy::fingerprint(bytes.as_bytes()),
            },
        }
    }
}

fn optional_path<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    String::deserialize(input).map(Some)
}

// deserialize_seq in the YAML adapter accepts null as empty; use deserialize_any
// to require a real sequence at the candidate boundary.
fn sequence<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Vec<T>, D::Error> {
    struct Sequence<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Sequence<T> {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a sequence")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut input: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = input.next_element()? {
                values.push(value);
            }
            Ok(values)
        }
    }
    input.deserialize_any(Sequence(std::marker::PhantomData))
}

/// Exact candidate artifact identities. This is not an active or approved policy.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Lock {
    document: LockDocument,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LockDocument {
    schema_version: u8,
    kind: String,
    config_digest: String,
    #[serde(deserialize_with = "sequence")]
    packs: Vec<LockedPack>,
    rules: Option<Artifact>,
    #[serde(default, skip_serializing_if = "AccessPaths::is_empty")]
    access_paths: AccessPaths,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_rules: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    disabled: Vec<LockedPack>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    mcp: crate::mcp_connections::Connections,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LockedPack {
    id: String,
    version: String,
    profile: Profile,
    artifact: Artifact,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: String,
    digest: String,
}

impl Candidate {
    /// Validate a strict version-1 local configuration and its portable paths.
    ///
    /// # Errors
    /// Rejects invalid syntax, unsupported fields/version, duplicate IDs, invalid
    /// exact versions, nonportable paths, and resource limits.
    pub fn from_yaml(input: &str) -> Result<Self, Error> {
        let document: Document = policy::parse(input)?;
        if document.schema_version != 1 {
            return Err(Error::Contract("unsupported configuration schema version"));
        }
        document.access_paths.validate()?;
        document.local_access_paths.validate()?;
        let shared_settings = settings::Layer {
            enforcement: document.enforcement,
            protection: document.protection,
            ask_fallback: document.ask_fallback,
            history: document.history,
            registries: document.registries,
        };
        shared_settings.validate()?;
        if let Some(local) = &document.local_settings {
            local.validate()?;
        }
        // The digest verification recomputes: a settings key with an empty or
        // null value is still left out, so empty values cannot stand in for
        // absent keys.
        let digest = settings::config_digest(input)?;
        let mut access_paths = document.access_paths;
        for (paths, local) in [
            (&mut access_paths.read, document.local_access_paths.read),
            (&mut access_paths.write, document.local_access_paths.write),
        ] {
            for path in local {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        let mut mcp = document.mcp;
        // Source provenance comes from the owning layer, never a supplied flag.
        for connection in mcp.values_mut() {
            connection.local = false;
        }
        for (id, mut connection) in document.local_mcp {
            connection.local = true;
            mcp.insert(id, connection);
        }
        crate::mcp_connections::validate(&mcp)?;
        let candidate = Self {
            packs: document.packs,
            rules: document.rules,
            local_rules: document.local_rules,
            disabled: document.disabled,
            digest,
            access_paths,
            mcp,
        };
        candidate.access_paths.validate()?;
        if candidate.packs.len() + candidate.disabled.len() > 64 {
            return Err(Error::Limit);
        }
        let mut ids = BTreeSet::new();
        for selection in &candidate.packs {
            selection.validate()?;
            if !ids.insert((&selection.registry_identity, &selection.id)) {
                return Err(Error::Contract("duplicate configuration pack identity"));
            }
        }
        let mut disabled_ids = BTreeSet::new();
        for selection in &candidate.disabled {
            selection.validate()?;
            if !disabled_ids.insert((&selection.registry_identity, &selection.id))
                || candidate.packs.iter().any(|active| {
                    active.registry_identity == selection.registry_identity
                        && active.id == selection.id
                        && active.layer != Some(Layer::Local)
                })
            {
                return Err(Error::Contract("duplicate configuration pack identity"));
            }
        }
        if candidate
            .disabled
            .iter()
            .any(|s| s.layer.is_some() || s.local.is_some())
        {
            return Err(Error::Contract("disabled selections are shared selections"));
        }
        for path in candidate.rules.iter().chain(&candidate.local_rules) {
            portable_path(path)?;
        }
        Ok(candidate)
    }

    /// Artifact paths relative to the configuration's owning directory, in input order.
    pub fn pack_paths(&self) -> impl Iterator<Item = &str> {
        self.packs.iter().map(|selection| selection.path.as_str())
    }

    /// Explicit custom-rule path; absent rules never trigger implicit file loading.
    #[must_use]
    pub fn rules_path(&self) -> Option<&str> {
        self.rules.as_deref()
    }

    /// The local layer's custom-rule path, when it has one.
    #[must_use]
    pub fn local_rules_path(&self) -> Option<&str> {
        self.local_rules.as_deref()
    }

    /// Artifact paths of shared selections the local layer disables.
    pub fn disabled_paths(&self) -> impl Iterator<Item = &str> {
        self.disabled
            .iter()
            .map(|selection| selection.path.as_str())
    }

    /// Validate and retain artifacts alongside a deterministic candidate lock.
    ///
    /// # Errors
    /// Rejects incomplete, invalid, incompatible, or identity/version-mismatched artifacts.
    pub fn capture(&self, packs: &[String], rules: Option<&str>) -> Result<Snapshot, Error> {
        self.capture_layers(packs, rules, None, &[])
    }

    /// Validate and retain artifacts of both layers: effective selections, shared
    /// and local custom rules, and the retained bytes of disabled selections.
    ///
    /// # Errors
    /// Rejects incomplete, invalid, incompatible, or identity/version-mismatched artifacts.
    pub fn capture_layers(
        &self,
        packs: &[String],
        rules: Option<&str>,
        local_rules: Option<&str>,
        disabled: &[String],
    ) -> Result<Snapshot, Error> {
        self.capture_reusing(packs, rules, local_rules, disabled, None)
    }

    fn capture_reusing(
        &self,
        packs: &[String],
        rules: Option<&str>,
        local_rules: Option<&str>,
        disabled: &[String],
        previous: Option<&Snapshot>,
    ) -> Result<Snapshot, Error> {
        let _trace = crate::diagnostics::trace::span("policy.capture_sources");
        crate::diagnostics::trace::count("policy.source_captures", 1);
        if packs.len() != self.packs.len()
            || rules.is_some() != self.rules.is_some()
            || local_rules.is_some() != self.local_rules.is_some()
            || disabled.len() != self.disabled.len()
        {
            return Err(Error::Contract("candidate artifact set is incomplete"));
        }
        let mut retained = Vec::new();
        for (selection, bytes) in self.disabled.iter().zip(disabled) {
            let pack = Pack::from_retained_yaml(bytes)?;
            if selection.id != pack.id() || selection.version != pack.version() {
                return Err(Error::ArtifactMismatch);
            }
            retained.push(selection.locked(bytes));
        }
        let mut locked = Vec::new();
        let mut captured = Vec::new();
        for (selection, bytes) in self.packs.iter().zip(packs) {
            let entry = selection.locked(bytes);
            let previous = previous
                .and_then(|snapshot| snapshot.packs.iter().find(|pack| pack.id() == entry.id));
            // Candidate capture is an untrusted source preview. Acquisition
            // establishes pack trust separately using the strict current codec.
            let mut pack =
                Pack::from_retained_yaml_reusing(bytes, &selection.binding(), false, previous)
                    .map_err(|error| error.in_selection(&selection.id))?;
            if selection.id != pack.id() || selection.version != pack.version() {
                return Err(Error::ArtifactMismatch);
            }
            pack.bind_identity(&entry.id);
            pack.bind_override_layers(
                selection.layer == Some(Layer::Local),
                &selection.overrides,
                selection.local.as_ref().map(|overlay| &overlay.overrides),
            );
            locked.push(entry);
            captured.push(pack);
        }
        let mcp = connections::resolve(&self.mcp, &self.packs, &captured)?;
        for pack in &captured {
            pack.validate_mcp_connections(&mcp)?;
        }
        policy::dependencies::validate_packs(
            &captured
                .iter()
                .zip(&self.packs)
                .map(|(pack, selection)| (pack, selection.profile))
                .collect::<Vec<_>>(),
        )?;
        for connection in mcp.values() {
            if connection
                .packs
                .keys()
                .any(|id| !captured.iter().any(|pack| pack.id() == id))
            {
                return Err(Error::Contract(
                    "MCP association names an unselected or ambiguous pack identity",
                ));
            }
        }
        locked.sort_by(|a, b| a.id.cmp(&b.id));
        let artifact = |path: &Option<String>, bytes: Option<&str>| {
            path.as_ref().zip(bytes).map(|(path, bytes)| Artifact {
                path: path.clone(),
                digest: policy::fingerprint(bytes.as_bytes()),
            })
        };
        let captured_custom = custom_rules(rules, local_rules)?;
        let local_artifact = artifact(&self.local_rules, local_rules);
        let rules = artifact(&self.rules, rules);
        let lock = Lock {
            document: LockDocument {
                schema_version: 1,
                kind: "local-candidate".into(),
                config_digest: self.digest.clone(),
                packs: locked,
                rules,
                access_paths: self.access_paths.clone(),
                local_rules: local_artifact,
                disabled: retained,
                mcp,
            },
        };
        Ok(Snapshot {
            lock,
            packs: captured,
            custom: captured_custom,
        })
    }
}

/// Shared custom rules with local rules joined; a local rule replaces the
/// shared rule with its ID.
fn custom_rules(shared: Option<&str>, local: Option<&str>) -> Result<Option<CustomRules>, Error> {
    let Some(local) = local else {
        return shared.map(CustomRules::from_retained_yaml).transpose();
    };
    let normalized = |source: &str| {
        let custom = CustomRules::from_retained_yaml(source)?;
        Ok::<_, Error>((custom.normalized_document()?, custom.comparison_sources))
    };
    let (local, local_sources) = normalized(local)?;
    let shared = shared.map(normalized).transpose()?;
    let comparison_sources = match (&shared, local_sources) {
        (Some((_, Some(shared))), Some(mut local)) => {
            let mut sources = shared.clone();
            sources.append(&mut local);
            Some(sources)
        }
        (None, local) => local,
        _ => None,
    };
    let shared = shared.map(|(document, _)| document);
    let field = |document: &serde_json::Value, name: &str| -> Vec<serde_json::Value> {
        document[name].as_array().cloned().unwrap_or_default()
    };
    let (mut merged, directory_options) = match shared.as_ref() {
        Some(shared) => (field(shared, "rules"), field(shared, "directory_options")),
        None => (Vec::new(), Vec::new()),
    };
    for rule in field(&local, "rules") {
        match merged.iter_mut().find(|r| r["id"] == rule["id"]) {
            Some(existing) => *existing = rule,
            None => merged.push(rule),
        }
    }
    // Both documents validated above, so their declarations read back.
    let declarations = |values: Vec<serde_json::Value>| {
        serde_json::from_value::<Vec<policy::directories::DirectoryOption>>(values.into())
            .map_err(|_| Error::Syntax)
    };
    let directory_options = policy::directories::join(
        &declarations(directory_options)?,
        &declarations(field(&local, "directory_options"))?,
    )?;
    let directory_options = serde_json::to_value(directory_options)
        .ok()
        .and_then(|values| values.as_array().cloned())
        .ok_or(Error::Syntax)?;
    let mut custom = CustomRules::from_yaml(
        &policy::rules::document_with(&merged, &directory_options)?.to_string(),
    )?;
    custom.comparison_sources = comparison_sources;
    Ok(Some(custom))
}

impl Lock {
    /// Fingerprint of the canonical JSON lock, independent of machine paths.
    ///
    /// # Errors
    /// Returns an error if the lock cannot be serialized.
    pub fn revision(&self) -> Result<String, Error> {
        let bytes = serde_json::to_vec(self)
            .map_err(|_| Error::Contract("candidate lock serialization failed"))?;
        Ok(policy::fingerprint(&bytes))
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(crate) fn portable_path(path: &str) -> Result<(), Error> {
    if path.len() > 4096 {
        return Err(Error::Limit);
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
        || path
            .chars()
            .any(|ch| ch.is_control() || "\\:<>\"|?*".contains(ch))
        || path.split('/').any(|part| {
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            part.ends_with(['.', ' '])
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
    {
        return Err(Error::Contract(
            "artifact paths must be portable relative paths without traversal",
        ));
    }
    Ok(())
}
