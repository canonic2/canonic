//! An untrusted project can request an exact new enrollment without loading its authority.
use super::{Context, Error, integrations::project_sources, storage};
use crate::{configuration, policy};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

/// Startup diagnostics establish no policy, grant or enrollment authority.
pub(crate) struct Probe {
    root: PathBuf,
    previous_root: Option<String>,
}

pub(crate) fn inspect(project: &Path) -> Result<Probe, Error> {
    let (root, previous_root) = storage::bootstrap::probe(project)?;
    Ok(Probe {
        root,
        previous_root,
    })
}

impl Probe {
    pub(crate) fn project_root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn bootstrap_required(&self) -> bool {
        self.previous_root.is_none()
    }

    pub(crate) fn report(&self) -> Value {
        let required = self.bootstrap_required();
        let mut report = json!({"bootstrap_required":required,"untrusted_policy":true,
            "policy_validated":false,"project_root":self.root,"previous_project_root":self.previous_root,
            "classification":classification(self.previous_root.as_deref(),&self.root),"classification_is_advisory":true,
            "next_action":if required { "Call project_enroll on this MCP connection to capture, review and confirm a new enrollment. Policy files are not evaluated by status." }
                else { "Use ordinary project diagnostics. This location probe does not validate policy or saved approval authority." }});
        crate::evidence::redact(&mut report);
        report
    }
}

fn classification(previous: Option<&str>, root: &Path) -> &'static str {
    match previous {
        None => "unenrolled",
        Some(path) if Some(path) == root.to_str() => "enrolled",
        Some(path) => match Path::new(path).try_exists() {
            Ok(true) => "copy",
            Ok(false) => "relocation",
            Err(_) => "unverified",
        },
    }
}

/// Immutable source and destination review. Constructing it writes nothing.
pub(crate) struct Plan {
    pub(in crate::review) root: PathBuf,
    pub(in crate::review) previous: storage::bootstrap::Previous,
    pub(in crate::review) candidate: Value,
    pub(in crate::review) metadata: Value,
    pub(in crate::review) revision: String,
    pub(in crate::review) fingerprint: String,
    pub(in crate::review) requested_launch: Option<PathBuf>,
    pub(in crate::review) requested_cache: Option<PathBuf>,
    classification: &'static str,
}

#[cfg(test)]
pub(crate) fn prepare(project: &Path) -> Result<Plan, Error> {
    prepare_options(project, None, None)
}

#[cfg(test)]
pub(crate) fn prepare_launch(project: &Path, requested: &Path) -> Result<Plan, Error> {
    prepare_options(project, Some(requested), None)
}

pub(crate) fn prepare_options(
    project: &Path,
    requested: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Result<Plan, Error> {
    for path in [requested, cache_dir].into_iter().flatten() {
        if !path.is_absolute()
            || path
                .to_str()
                .is_none_or(|value| value.len() > 4096 || value.contains('\0'))
        {
            return Err(Error::Invalid(
                "enrollment paths must be absolute UTF-8 paths of at most 4096 bytes",
            ));
        }
    }
    let root = project.canonicalize()?;
    let previous = storage::bootstrap::inspect(&root)?;
    if previous.root.as_deref() == root.to_str() {
        return Err(Error::Invalid(
            "project is already enrolled at this location",
        ));
    }
    let shield = root.join(".shield").canonicalize()?;
    let config = shield.join("config.yml").canonicalize()?;
    if config.parent() != Some(shield.as_path()) {
        return Err(Error::Invalid(
            "project configuration must remain inside .shield",
        ));
    }
    let local = shield.join("config.local.yml");
    if local.exists() && !local.canonicalize()?.starts_with(&shield) {
        return Err(Error::Invalid(
            "local configuration must remain inside .shield",
        ));
    }
    let candidate = json!(
        project_sources::capture(&config).map_err(|_| Error::Invalid(
            "invalid project configuration or contained policy source"
        ))?
    );
    let merged = configuration::layers::merged(&candidate)?;
    let mut installed = project_sources::installed(&merged, &candidate)?;
    // Registry labels are source identities, not enrolled signing trust. Keep
    // selection identity so enrollment does not rewrite the project's files.
    for (row, selection) in installed
        .iter_mut()
        .zip(merged["packs"].as_array().into_iter().flatten())
    {
        if let Some(identity) = selection["registry_identity"].as_str() {
            row["source_identity"] = json!(identity);
            row["source"]["identity"] = json!(identity);
        }
    }
    let mut metadata = json!({"sources":{},"default":null,"installed":installed,"projection":null});
    let disabled = project_sources::disabled(&merged, &candidate)?;
    if !disabled.is_empty() {
        metadata["disabled"] = json!(disabled);
    }
    super::packs::refresh_shared_metadata(&candidate, &mut metadata)?;
    if let Some(cache_dir) = cache_dir {
        metadata["compiled_cache_root"] = json!(cache_dir);
    }
    let evaluator = configuration::Evaluator::new(super::snapshots(&BTreeMap::from([(
        "project".into(),
        candidate.clone(),
    )]))?)?;
    let classification = classification(previous.root.as_deref(), &root);
    let fingerprint = crate::json::fingerprint(&json!({"kind":"project-enrollment-v1",
        "root":root,"previous":previous.digest,"candidate":candidate,"metadata":metadata,"requested_launch":requested,"requested_cache":cache_dir}))
    .map_err(|_| Error::Invalid("enrollment review encoding"))?;
    Ok(Plan {
        root,
        previous,
        candidate,
        metadata,
        revision: evaluator.revision().into(),
        fingerprint,
        requested_launch: requested.map(Path::to_path_buf),
        requested_cache: cache_dir.map(Path::to_path_buf),
        classification,
    })
}

impl Plan {
    pub(in crate::review) fn launch_roots(&self) -> Vec<String> {
        if self.requested_launch.is_none() && !self.previous.confirmed_launch_roots.is_empty() {
            return self.previous.confirmed_launch_roots.clone();
        }
        let mut paths = Vec::new();
        for path in self.previous.root.as_deref().into_iter().chain(
            self.requested_launch
                .as_ref()
                .and_then(|path| path.to_str()),
        ) {
            if !paths.iter().any(|previous| previous == path) {
                paths.push(path.to_owned());
            }
        }
        paths
    }

    pub(crate) fn committed(&self) -> Result<Option<Context>, Error> {
        storage::bootstrap::committed(self)
    }

    pub(crate) fn report(&self) -> Value {
        let config =
            configuration::layers::merged(&self.candidate).expect("validated configuration");
        let packs: Vec<_> = config["packs"].as_array().into_iter().flatten().enumerate()
            .map(|(index, selection)| json!({"id":selection["id"],"registry_identity":selection["registry_identity"],
                "version":selection["version"],"profile":selection["profile"],"path":selection["path"],
                "digest":policy::fingerprint(self.candidate["packs"][index].as_str().unwrap_or_default().as_bytes())})).collect();
        let rules: Vec<_> = ["rules", "local_rules"].into_iter().filter_map(|field| {
            let source = self.candidate[field].as_str()?;
            let normalized = policy::normalize_retained_custom(source).ok()?;
            Some(json!({"layer":field,"rule_count":normalized["rules"].as_array().map_or(0, Vec::len),
                "digest":policy::fingerprint(source.as_bytes())}))
        }).collect();
        let mut report = json!({"bootstrap_required":true,"untrusted_policy":true,
            "project_root":self.root,"previous_project_root":self.previous.root,"classification":self.classification,
            "classification_is_advisory":true,"review_fingerprint":self.fingerprint,"policy_revision":self.revision,
            "launch_roots":self.launch_roots(),
            "cache_dir":self.requested_cache,
            "recovery_scope":"The two latest predecessor checkpoints and history generations remain private recovery data; older inactive recovery data may be reclaimed after commit.",
            "policy":{"packs":packs,"custom_rules":rules,"access_paths":config["access_paths"],
                "local_access_paths":config["local_access_paths"]},
            "next_action":"Call project_enroll on this MCP connection to review and confirm a new enrollment at this location. Declining leaves all files unchanged.",
            "scope":"New project and activation identities. Existing approvals, sessions, registry trust, disabled protection and agent registrations are not imported. Source files and the previous checkpoint are preserved."});
        crate::evidence::redact(&mut report);
        report
    }

    pub(crate) fn message(&self) -> String {
        let report = self.report();
        let mut text = format!(
            "Enroll Shield at {}?\nPreviously recorded location: {}\nClassification: {} (advisory only).\n\nThis confirms the local policy files as a new enrollment. Review .shield/config.yml and its referenced packs and rules. Registry provenance has not been imported.\nA new project identity and policy activation will be created. No old approvals, conversation grants, registry trust, disabled protection or agent registrations will be reused. Existing source files and recovery evidence are preserved.\nThe reviewed MCP launch paths below may connect to this new project when launched from this project.\n\nSelected packs:\n",
            display(self.root.to_str().unwrap_or("invalid"), usize::MAX),
            display(self.previous.root.as_deref().unwrap_or("none"), usize::MAX),
            self.classification
        );
        for row in report["policy"]["packs"].as_array().into_iter().flatten() {
            writeln!(
                &mut text,
                "- {} @ {} ({}) SHA256 {}",
                short(&row["id"]),
                short(&row["version"]),
                short(&row["profile"]),
                short(&row["digest"])
            )
            .expect("writing to a String cannot fail");
        }
        for row in report["policy"]["custom_rules"]
            .as_array()
            .into_iter()
            .flatten()
        {
            writeln!(
                &mut text,
                "- {}: {} rules; SHA256 {}",
                short(&row["layer"]),
                row["rule_count"],
                short(&row["digest"])
            )
            .expect("writing to a String cannot fail");
        }
        writeln!(
            &mut text,
            "\nReviewed MCP launch paths: {}",
            self.launch_roots()
                .iter()
                .map(|path| display(path, usize::MAX))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .expect("writing to a String cannot fail");
        writeln!(&mut text,"\nCompiled cache: {}. This selects disposable compiled storage and grants no execution or policy permissions.",
            self.requested_cache.as_ref().map_or_else(|| "the system default (no copied cache override)".into(),|path| display(path.to_str().unwrap_or("invalid"),usize::MAX))).expect("writing to a String cannot fail");
        write!(&mut text,"\nThe two latest predecessor checkpoints and history generations remain private recovery data. Older inactive recovery data may be reclaimed after commit.\nAccess roots: {}\nLocal access roots: {}\nReview fingerprint: {}\nConfirm only if you intend to trust this policy at this exact location.",
            access_summary(&report["policy"]["access_paths"]),access_summary(&report["policy"]["local_access_paths"]),self.fingerprint).expect("writing to a String cannot fail");
        text
    }

    /// Called only by a transport after the host delivers the person's acceptance.
    #[cfg(test)]
    pub(crate) fn commit(&self) -> Result<Context, Error> {
        self.commit_checked(&|| Ok(()))
    }

    pub(crate) fn commit_checked(
        &self,
        checkpoint: super::Checkpoint<'_>,
    ) -> Result<Context, Error> {
        storage::bootstrap::commit(self, "host-elicitation", checkpoint)
    }

    /// The CLI owns its separate terminal confirmation before entering this method.
    pub(crate) fn commit_cli(&self) -> Result<Context, Error> {
        storage::bootstrap::commit(self, "operator-cli", &|| Ok(()))
    }
}

fn short(value: &Value) -> String {
    display(value.as_str().unwrap_or("unknown"), 200)
}

fn display(value: &str, limit: usize) -> String {
    let shortened: String = value.chars().take(limit).collect();
    let encoded = serde_json::to_string(&shortened).expect("text encodes");
    let escaped: String = encoded.chars().map(|character| {
        if character.is_control() || matches!(character,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {
            format!("\\u{:04x}",u32::from(character))
        } else { character.to_string() }
    }).collect();
    if shortened.len() == value.len() {
        escaped
    } else {
        format!(
            "{escaped}… (shortened; full SHA256 {})",
            policy::fingerprint(value.as_bytes())
        )
    }
}

fn access_summary(value: &Value) -> String {
    let parts: Vec<_> = ["read", "write"]
        .into_iter()
        .map(|access| {
            let roots: Vec<_> = value[access]
                .as_array()
                .into_iter()
                .flatten()
                .take(4)
                .filter_map(Value::as_str)
                .map(|path| display(path, 128))
                .collect();
            format!(
                "{access}: {} roots [{}]; source SHA256 {}",
                value[access].as_array().map_or(0, Vec::len),
                roots.join(", "),
                policy::fingerprint(value[access].to_string().as_bytes())
            )
        })
        .collect();
    parts.join("; ")
}

/// Select a retained reviewed launch association for the current project.
/// Portable enrollment evidence authenticates this route; resource grants and
/// operation epochs still require their separate current-checkout validation.
pub(crate) fn accepts_launch(project: &Path, requested: &Path) -> bool {
    let Ok(state) = storage::bootstrap::current_enrollment(project) else {
        return false;
    };
    let receipt = &state.operator["bootstrap"];
    receipt["launch_roots"]
        .as_array()
        .into_iter()
        .flatten()
        .take(2)
        .filter_map(Value::as_str)
        .any(|path| Path::new(path) == requested)
        && receipt["project_root"] == state.project_root
        && storage::bootstrap::enrollment(&state, receipt)
}

/// Copied metadata may select only restricted enrollment review, never authority.
pub(crate) fn launch_matches_metadata(project: &Path, requested: &Path) -> bool {
    storage::bootstrap::launch_matches_metadata(project, requested)
}

/// Continue the reviewed host's form capability after reconnecting or relocation.
/// This metadata supplies no native agent identity or operation authority.
pub(crate) fn forms_reviewed_at(project: &Path) -> bool {
    storage::bootstrap::current_enrollment(project).is_ok_and(|state| {
        storage::bootstrap::enrollment(&state, &state.operator["bootstrap"])
            || integration_forms_reviewed(&state)
            || super::registry_trust::inactive_reviewed(&state)
            || state.authorization_events.iter().any(|event| {
                event["kind"] == "policy_reused"
                    && event["operator"]["mechanism"] == "reviewed-policy-reuse"
                    && event["provenance"]["project_id"] == state.project_id
                    && event["provenance"]["project_root"] == state.project_root
                    && event["details"]["receipt"] == state.operator["policy_reuse"]
            })
    })
}

fn integration_forms_reviewed(state: &super::model::State) -> bool {
    let receipt = &state.operator["integration_review"];
    receipt.is_object()
        && receipt["project_id"] == state.project_id
        && receipt["project_root"] == state.project_root
        && receipt["review_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        && state.authorization_events.iter().any(|event| {
            event["kind"] == "integration_review_confirmed"
                && event["provenance"]["project_id"] == state.project_id
                && event["provenance"]["project_root"] == state.project_root
                && event["details"]["review_channel"] == *receipt
                && ["host-elicitation", "operator-cli"]
                    .contains(&event["operator"]["mechanism"].as_str().unwrap_or(""))
        })
}
