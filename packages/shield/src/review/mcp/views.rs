//! Immutable connection-local snapshot handles and stateless list views.
use super::{Context, Error};
use crate::{
    configuration,
    review::{snapshots, storage::Store},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, OnceLock},
};
// `Store` and `Path` serve the active handle below.
mod paging;
#[cfg(test)]
mod reclamation_tests;
mod retention;

/// Source and compiled bytes all of a connection's snapshot handles may pin.
/// It admits the largest bundle and plan the policy limits allow together.
const HANDLE_BYTES: usize = 1024 * 1024 * 1024;

pub(super) struct Captured {
    pub(super) candidate: Value,
    pub(super) metadata: Value,
    evaluator: OnceLock<configuration::Evaluator>,
    revision: String,
}
impl Captured {
    pub(super) fn revision(&self) -> &str {
        &self.revision
    }
    pub(super) fn evaluator(&self) -> Result<&configuration::Evaluator, Error> {
        self.evaluator.get().ok_or(Error::CandidateUnavailable)
    }
    fn load_evaluator(&self) -> Result<configuration::Evaluator, Error> {
        // Rebuild only this exact immutable capture, never current project files.
        configuration::Evaluator::new_for_revision(
            snapshots(&BTreeMap::from([(
                "project".into(),
                self.candidate.clone(),
            )]))?,
            &self.revision,
        )
        .map_err(Error::from)
    }
    fn with_evaluator(&self, evaluator: configuration::Evaluator) -> Self {
        Self {
            candidate: self.candidate.clone(),
            metadata: self.metadata.clone(),
            revision: self.revision.clone(),
            evaluator: OnceLock::from(evaluator),
        }
    }
}
#[derive(Clone)]
struct Handle {
    captured: Arc<Captured>,
    key: String,
    bytes: usize,
    used: u64,
    resolutions: BTreeMap<usize, Arc<BTreeMap<String, Value>>>,
}
#[derive(Clone)]
pub(super) struct Views {
    seed: String,
    sequence: u64,
    bytes: usize,
    handles: BTreeMap<String, Handle>,
    identities: BTreeMap<String, String>,
    approvals: BTreeMap<String, (Arc<Value>, bool)>,
    approval_bytes: usize,
}
impl Views {
    pub(super) fn new(seed: String) -> Self {
        Self {
            seed,
            sequence: 0,
            bytes: 0,
            handles: BTreeMap::new(),
            identities: BTreeMap::new(),
            approvals: BTreeMap::new(),
            approval_bytes: 0,
        }
    }
    fn id(&mut self, kind: &str) -> String {
        self.sequence += 1;
        format!("{kind}-{}-{}", self.seed, self.sequence)
    }
    pub(super) fn retain(
        &mut self,
        context: &Context,
        candidate: &Value,
        metadata: &Value,
    ) -> Result<String, Error> {
        self.retain_prepared(context, candidate, metadata, None, None, None)
    }
    /// Reserve an exact capture with the plan already prepared for its review.
    fn retain_prepared(
        &mut self,
        context: &Context,
        candidate: &Value,
        metadata: &Value,
        prepared: Option<configuration::Evaluator>,
        expected: Option<&str>,
        keep: Option<&str>,
    ) -> Result<String, Error> {
        let key = identity(candidate, metadata);
        self.retain_keyed(
            context,
            (candidate, metadata),
            key,
            prepared,
            expected,
            keep,
        )
    }
    /// [`Self::retain_prepared`] for a capture whose handle key is known.
    fn retain_keyed(
        &mut self,
        context: &Context,
        (candidate, metadata): (&Value, &Value),
        key: String,
        prepared: Option<configuration::Evaluator>,
        expected: Option<&str>,
        keep: Option<&str>,
    ) -> Result<String, Error> {
        let _trace = crate::diagnostics::trace::span("handle.retain");
        if candidate.is_null() {
            return Err(Error::CandidateUnavailable);
        }
        if let Some(id) = self.identities.get(&key).cloned() {
            let captured = self.get(&id)?;
            if expected.is_some_and(|expected| expected != captured.revision)
                || prepared
                    .as_ref()
                    .is_some_and(|plan| plan.revision() != captured.revision)
            {
                return Err(Error::CandidateUnavailable);
            }
            self.retain_existing_plan(context, &id, &captured, prepared, keep)?;
            return Ok(id);
        }
        crate::diagnostics::trace::count("handle.admission_encodings", 1);
        let bytes = crate::json::encoded_len(&[candidate, metadata])
            .map_err(|_| Error::Invalid("candidate encoding"))?;
        let revision = if let Some(prepared) = &prepared {
            prepared.revision().to_owned()
        } else if let Some(expected) = expected {
            configuration::recorded_effective(candidate, expected)?;
            expected.into()
        } else {
            configuration::recorded_revision(candidate)?
        };
        let length = bytes.saturating_add(
            prepared
                .as_ref()
                .map_or(0, |plan| plan.compiled_bytes().len()),
        );
        self.handle_room(context, length, 1, &[keep])?;
        let id = self.id("candidate");
        self.bytes += length;
        self.handles.insert(
            id.clone(),
            Handle {
                key: key.clone(),
                bytes: length,
                used: self.sequence,
                resolutions: BTreeMap::new(),
                captured: Arc::new(Captured {
                    candidate: candidate.clone(),
                    metadata: metadata.clone(),
                    evaluator: prepared.map_or_else(OnceLock::new, OnceLock::from),
                    revision,
                }),
            },
        );
        self.identities.insert(key, id.clone());
        Ok(id)
    }
    /// Preserve an already prepared review plan after admitting only the bytes
    /// beyond the existing reservation; rollback clones retain their old Arc.
    fn retain_existing_plan(
        &mut self,
        context: &Context,
        id: &str,
        captured: &Captured,
        prepared: Option<configuration::Evaluator>,
        keep: Option<&str>,
    ) -> Result<(), Error> {
        if captured.evaluator.get().is_none()
            && let Some(prepared) = prepared
        {
            let extra = prepared.compiled_bytes().len();
            self.handle_room(context, extra, 0, &[Some(id), keep])?;
            self.bytes += extra;
            let handle = self
                .handles
                .get_mut(id)
                .ok_or(Error::CandidateUnavailable)?;
            handle.bytes += extra;
            handle.captured = Arc::new(captured.with_evaluator(prepared));
        }
        Ok(())
    }

    pub(super) fn active(&mut self, context: &Context) -> Result<String, Error> {
        self.active_with(context, false, true)
    }
    pub(super) fn active_readonly(&mut self, context: &Context) -> Result<String, Error> {
        self.active_with(context, true, true)
    }
    /// Status exposes an exact reference without loading a second plan.
    pub(super) fn active_reference(&mut self, context: &Context) -> Result<String, Error> {
        self.active_with(context, true, false)
    }
    /// The handle of the policy the files hold now, which is the policy
    /// requests are decided by. A read-only view reuses a cached plan but never
    /// compiles one; evaluation prepares it on demand.
    fn active_with(
        &mut self,
        context: &Context,
        readonly: bool,
        prepare: bool,
    ) -> Result<String, Error> {
        let root = Path::new(&context.project_root);
        let state = if readonly {
            Store::inspect(root)?.read_inspection_following_files()?
        } else {
            Store::open(root)?.read_following_files()?
        };
        state.check(context)?;
        let candidate = state
            .candidates
            .get("project")
            .ok_or(Error::Invalid("project has no policy"))?;
        let cache = crate::review::files::cache_for(&state.operator).ok();
        let plan = if !prepare {
            None
        } else if readonly {
            cache.and_then(|cache| crate::review::files::cached_in(root, &cache))
        } else {
            Some(crate::review::files::load_in(root, cache.as_deref())?.evaluator)
        }
        .filter(|plan| plan.revision() == state.policy_revision);
        self.retain_prepared(
            context,
            candidate,
            &state.operator,
            plan,
            Some(&state.policy_revision),
            None,
        )
    }
    pub(super) fn get(&mut self, id: &str) -> Result<Arc<Captured>, Error> {
        self.sequence += 1;
        let handle = self
            .handles
            .get_mut(id)
            .ok_or(Error::CandidateUnavailable)?;
        handle.used = self.sequence;
        Ok(handle.captured.clone())
    }
    /// Cache descriptive resolution only within the exact immutable handle.
    /// Optional memo pressure falls back to extraction without retaining bytes.
    pub(super) fn resolution(
        &mut self,
        id: &str,
        candidate: &Value,
        index: usize,
        artifact: &str,
    ) -> Result<Arc<BTreeMap<String, Value>>, Error> {
        let handle = self.handles.get(id).ok_or(Error::CandidateUnavailable)?;
        if let Some(report) = handle.resolutions.get(&index) {
            return Ok(report.clone());
        }
        let report = Arc::new(crate::review::inspection::resolution(
            artifact, candidate, index,
        )?);
        let bytes = crate::json::encoded_len(&(index, report.as_ref()))
            .map_err(|_| Error::Invalid("resolution encoding"))?;
        if self.bytes.saturating_add(bytes) <= HANDLE_BYTES {
            let handle = self
                .handles
                .get_mut(id)
                .ok_or(Error::CandidateUnavailable)?;
            self.bytes += bytes;
            handle.bytes += bytes;
            handle.resolutions.insert(index, report.clone());
        }
        Ok(report)
    }
    /// Explicit evaluation prepares one exact capture and admits its plan bytes
    /// before retaining it. Read-only handle exposure never calls this method.
    pub(super) fn prepare(&mut self, context: &Context, id: &str) -> Result<Arc<Captured>, Error> {
        let captured = self.get(id)?;
        if captured.evaluator.get().is_some() {
            return Ok(captured);
        }
        let evaluator = captured.load_evaluator()?;
        let extra = evaluator.compiled_bytes().len();
        self.handle_room(context, extra, 0, &[Some(id)])?;
        let prepared = Arc::new(captured.with_evaluator(evaluator));
        self.bytes += extra;
        let handle = self
            .handles
            .get_mut(id)
            .ok_or(Error::CandidateUnavailable)?;
        handle.bytes += extra;
        handle.captured = prepared.clone();
        Ok(prepared)
    }
    /// Reuse only a plan already prepared for these exact immutable bytes and metadata.
    /// The caller must still validate current authority and the retained source.
    pub(super) fn prepared(&self, candidate: &Value, metadata: &Value) -> Option<Arc<Captured>> {
        let id = self.identities.get(&identity(candidate, metadata))?;
        let captured = &self.handles.get(id)?.captured;
        captured.evaluator.get()?;
        Some(captured.clone())
    }
    pub(super) fn approval(&self, binding: &str) -> Option<&Value> {
        self.approvals.get(binding).map(|(args, _)| args.as_ref())
    }
    /// Only a hint for form routing. Apply always verifies the retained proposal
    /// and refuses connection changes without a trusted form answer.
    pub(super) fn connection_review(&self, arguments: &Value) -> Option<bool> {
        self.approvals
            .values()
            .find_map(|(args, changed)| (args.as_ref() == arguments).then_some(*changed))
    }
    /// Presentation is disposable: reaching its bound falls back to regeneration.
    pub(super) fn remember_approval(
        &mut self,
        binding: String,
        arguments: Value,
        connection_review: bool,
    ) {
        if self.approvals.contains_key(&binding) || self.approvals.len() >= 256 {
            return;
        }
        let Ok(length) = crate::json::encoded_len(&arguments) else {
            return;
        };
        if self.approval_bytes.saturating_add(length) <= 8 * 1024 * 1024 {
            self.approval_bytes += length;
            self.approvals
                .insert(binding, (Arc::new(arguments), connection_review));
        }
    }
}
fn identity(candidate: &Value, metadata: &Value) -> String {
    identity_of(&crate::review::candidate_identity(candidate), metadata)
}

/// [`identity`] of a candidate known by its identity alone, such as a
/// proposal's, without rebuilding it.
fn identity_of(candidate: &str, metadata: &Value) -> String {
    crate::policy::fingerprint(
        json!([
            env!("CARGO_PKG_VERSION"),
            crate::policy::resolution::compiled::FORMAT,
            candidate,
            metadata
        ])
        .to_string()
        .as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_memos_are_selection_bound_admitted_and_disposable() {
        let root = assert_fs::TempDir::new().unwrap();
        let source = json!({
            "schema_version":1,"kind":"pack","id":"acme/inspection","version":"0.1.0",
            "description":"Acme inspection","license":"MIT","maintainers":["Acme"],
            "engines":{"shield":"*"},
            "capabilities":["structured-rules-v1","argument-rules-v1","variables-v1"],
            "variables":{"target":{"type":"string","description":"Selected target"}},
            "rules":[{"id":"inspect","decision":"allow","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[{"var":"target"}]}}}]
        }).to_string();
        let config = json!({"schema_version":1,"packs":[
            {"id":"acme/inspection","version":"0.1.0","profile":"recommended","path":"pack.json","variables":{"target":"status"}}
        ]}).to_string();
        let bundle =
            configuration::Bundle::capture(&config, std::slice::from_ref(&source), None).unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let state = Store::open(root.path())
            .unwrap()
            .read_following_files()
            .unwrap();
        let mut views = Views::new("acme-resolution-memo".into());
        let id = views
            .retain(&context, &state.candidates["project"], &state.operator)
            .unwrap();
        let captured = views.get(&id).unwrap();
        let candidate = captured.candidate.clone();
        let rollback = views.clone();
        let before = views.bytes;
        let first = views.resolution(&id, &candidate, 0, &source).unwrap();
        let charged = crate::json::encoded_len(&(0_usize, first.as_ref())).unwrap();
        assert_eq!(views.bytes, before + charged);
        assert_eq!(
            first["inspect"]["resolved"]["conditions"]["arguments"]["values"],
            json!(["status"])
        );
        let again = views.resolution(&id, &candidate, 0, &source).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(views.bytes, before + charged);
        assert!(rollback.handles[&id].resolutions.is_empty());
        assert!(captured.evaluator.get().is_none());
        let other_config = config.replace("status", "diff");
        let other_bundle =
            configuration::Bundle::capture(&other_config, std::slice::from_ref(&source), None)
                .unwrap();
        let other_candidate = json!(other_bundle);
        let other = views
            .retain(&context, &other_candidate, &state.operator)
            .unwrap();
        assert_ne!(id, other);
        let other_report = views
            .resolution(&other, &other_candidate, 0, &source)
            .unwrap();
        assert_eq!(
            other_report["inspect"]["resolved"]["conditions"]["arguments"]["values"],
            json!(["diff"])
        );
        assert!(!Arc::ptr_eq(&first, &other_report));
        // No optional memo admission can evict another handle or fail a view.
        let mut full = rollback;
        full.bytes = HANDLE_BYTES;
        let uncached = full.resolution(&id, &candidate, 0, &source).unwrap();
        assert_eq!(*uncached, *first);
        assert_eq!(full.bytes, HANDLE_BYTES);
        assert!(full.handles[&id].resolutions.is_empty());
    }

    #[test]
    fn recorded_handles_admit_plan_bytes_only_on_explicit_first_evaluation() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let state = Store::open(root.path())
            .unwrap()
            .read_following_files()
            .unwrap();
        let mut views = Views::new("acme-lazy-plan-budget".into());
        let id = views
            .retain(&context, &state.candidates["project"], &state.operator)
            .unwrap();
        let exposed = views.get(&id).unwrap();
        assert!(exposed.evaluator.get().is_none());
        let rollback = views.clone();
        let before = views.bytes;
        let prepared = views.prepare(&context, &id).unwrap();
        let plan_bytes = prepared.evaluator().unwrap().compiled_bytes().len();
        assert_eq!(views.bytes, before + plan_bytes);
        assert!(
            rollback.handles[&id].captured.evaluator.get().is_none(),
            "rollback captures never acquire uncharged plans"
        );
        assert!(exposed.evaluator.get().is_none());
        views.prepare(&context, &id).unwrap();
        assert_eq!(views.bytes, before + plan_bytes);
    }

    #[test]
    fn the_active_handle_is_the_policy_the_files_hold() {
        let root = assert_fs::TempDir::new().unwrap();
        let config = "schema_version: 1\npacks: []\n";
        let bundle = crate::configuration::Bundle::capture(config, &[], None).unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let path = root.path().join(".shield/config.yml");
        std::fs::write(&path, config).unwrap();
        let mut views = Views::new("acme-files".into());
        let first = views.active(&context).unwrap();
        let files = crate::review::files::load(root.path()).unwrap();
        assert_eq!(
            views.get(&first).unwrap().revision(),
            files.evaluator.revision()
        );
        assert!(views.get(&first).unwrap().evaluator().is_ok());

        // A person edits the files; the next handle is their policy.
        std::fs::write(
            &path,
            format!(
                "{config}access_paths:\n  read:\n  - {}\n",
                crate::test_support::absolute("/srv/acme")
            ),
        )
        .unwrap();
        let second = views.active_readonly(&context).unwrap();
        assert_ne!(first, second);
        let edited = crate::review::files::load(root.path()).unwrap();
        assert_ne!(edited.evaluator.revision(), files.evaluator.revision());
        assert_eq!(
            views.get(&second).unwrap().revision(),
            edited.evaluator.revision()
        );
    }
}
