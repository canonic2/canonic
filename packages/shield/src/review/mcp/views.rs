//! Immutable connection-local snapshot handles and retained cursor views.
use super::{Context, Error, required};
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
    cached: Option<Cached>,
    source: Option<SourceLease>,
    revision: String,
}
#[derive(Clone)]
struct Cached {
    artifact: crate::review::cache::Artifact,
    root: String,
    project_id: String,
}
#[derive(Clone)]
struct SourceLease {
    _pins: crate::review::storage::objects::Pins,
    root: String,
}
impl Captured {
    pub(super) fn revision(&self) -> &str {
        &self.revision
    }
    pub(super) fn evaluator(&self) -> Result<&configuration::Evaluator, Error> {
        self.evaluator.get().ok_or(Error::CandidateUnavailable)
    }
    fn load_evaluator(&self) -> Result<configuration::Evaluator, Error> {
        if let Some(cached) = &self.cached
            && let Ok(evaluator) = cached.artifact.load(&cached.root, &cached.project_id)
            && evaluator.revision() == self.revision
        {
            return Ok(evaluator);
        }
        // Rebuild only this exact immutable capture, never current project files.
        configuration::Evaluator::new_for_revision(
            snapshots(&BTreeMap::from([("project".into(), self.source()?)]))?,
            &self.revision,
        )
        .map_err(Error::from)
    }
    fn with_evaluator(&self, evaluator: configuration::Evaluator) -> Self {
        Self {
            candidate: self.candidate.clone(),
            metadata: self.metadata.clone(),
            cached: self.cached.clone(),
            source: self.source.clone(),
            revision: self.revision.clone(),
            evaluator: OnceLock::from(evaluator),
        }
    }
    pub(super) fn source(&self) -> Result<Value, Error> {
        self.source.as_ref().map_or_else(
            || Ok(self.candidate.clone()),
            |source| Store::inspect(Path::new(&source.root))?.captured_candidate(&self.candidate),
        )
    }
}
#[derive(Clone)]
struct Cursor {
    query: String,
    used: u64,
    content: paging::Content,
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
    cursor_bytes: usize,
    handles: BTreeMap<String, Handle>,
    identities: BTreeMap<String, String>,
    cursors: BTreeMap<String, Cursor>,
    approvals: BTreeMap<String, (Arc<Value>, bool)>,
    approval_bytes: usize,
}
impl Views {
    pub(in crate::review::mcp) fn retained_state(&self) -> bool {
        !self.handles.is_empty() || !self.cursors.is_empty() || !self.approvals.is_empty()
    }
    pub(super) fn new(seed: String) -> Self {
        Self {
            seed,
            sequence: 0,
            bytes: 0,
            cursor_bytes: 0,
            handles: BTreeMap::new(),
            identities: BTreeMap::new(),
            cursors: BTreeMap::new(),
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
        let _trace = crate::diagnostics::trace::span("handle.retain");
        if candidate.is_null() {
            return Err(Error::CandidateUnavailable);
        }
        let key = identity(candidate, metadata);
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
        let retained = matches!(
            candidate["kind"].as_str(),
            Some("retained-source-v1" | "retained-source-v2")
        );
        let revision = if let Some(prepared) = &prepared {
            if retained {
                crate::review::inspection::source::revision(
                    context,
                    candidate,
                    Some(prepared.revision()),
                )?;
            }
            prepared.revision().to_owned()
        } else if retained {
            crate::review::inspection::source::revision(context, candidate, expected)?
        } else if let Some(expected) = expected {
            configuration::recorded_effective(candidate, expected)?;
            expected.into()
        } else {
            configuration::recorded_revision(candidate)?
        };
        let source = if retained {
            let store = Store::inspect(Path::new(&context.project_root))?;
            Some(SourceLease {
                _pins: crate::review::storage::sources::pin_inspection(&store, candidate)?,
                root: context.project_root.clone(),
            })
        } else {
            None
        };
        let source_bytes = if retained {
            usize::try_from(
                candidate["length"]
                    .as_u64()
                    .ok_or(Error::CandidateUnavailable)?,
            )
            .map_err(|_| Error::Overloaded)?
        } else {
            0
        };
        let length = bytes
            .saturating_add(source_bytes)
            .saturating_add(source.as_ref().map_or(0, |source| source.root.len()))
            .saturating_add(
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
                    cached: None,
                    source,
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
            let reserved = captured
                .cached
                .as_ref()
                .map(|cached| usize::try_from(cached.artifact.byte_len()))
                .transpose()
                .map_err(|_| Error::Overloaded)?
                .unwrap_or(0);
            let extra = prepared.compiled_bytes().len().saturating_sub(reserved);
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
        self.active_with(context, false)
    }
    pub(super) fn active_readonly(&mut self, context: &Context) -> Result<String, Error> {
        self.active_with(context, true)
    }
    fn active_with(&mut self, context: &Context, readonly: bool) -> Result<String, Error> {
        let (state, pins) = if readonly {
            Store::inspect(Path::new(&context.project_root))?.read_pinned_diagnostics()?
        } else {
            Store::open(Path::new(&context.project_root))?.read_pinned_checkpoint()?
        };
        state.check(context)?;
        let active = state
            .active
            .as_ref()
            .ok_or(Error::Invalid("project has no confirmed active policy"))?;
        active.validate_binding(&state)?;
        let key = identity(active.project_candidate()?, &state.operator);
        if let Some(id) = self.identities.get(&key).cloned() {
            self.get(&id)?;
            return Ok(id);
        }
        let compiled = &active.compiled;
        let candidate = active.project_candidate()?.clone();
        let source_bytes = usize::try_from(candidate["length"].as_u64().unwrap_or_default())
            .map_err(|_| Error::Overloaded)?;
        let length = json!([candidate, state.operator])
            .to_string()
            .len()
            .saturating_add(source_bytes)
            .saturating_add(usize::try_from(compiled.byte_len()).map_err(|_| Error::Overloaded)?)
            .saturating_add(compiled.inspection_byte_len()?);
        self.handle_room(context, length, 1, &[])?;
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
                    candidate,
                    metadata: state.operator,
                    evaluator: OnceLock::new(),
                    revision: active.revision().into(),
                    source: Some(SourceLease {
                        _pins: pins,
                        root: state.project_root.clone(),
                    }),
                    cached: Some(Cached {
                        artifact: compiled.clone(),
                        root: state.project_root,
                        project_id: state.project_id,
                    }),
                }),
            },
        );
        self.identities.insert(key, id.clone());
        Ok(id)
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
        let reserved = captured
            .cached
            .as_ref()
            .map(|cached| usize::try_from(cached.artifact.byte_len()))
            .transpose()
            .map_err(|_| Error::Overloaded)?
            .unwrap_or(0);
        let extra = evaluator.compiled_bytes().len().saturating_sub(reserved);
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
    crate::policy::fingerprint(
        json!([
            env!("CARGO_PKG_VERSION"),
            crate::policy::compiled::FORMAT,
            crate::review::storage::sources::candidate_identity(candidate),
            metadata
        ])
        .to_string()
        .as_bytes(),
    )
}
fn query(name: &str, args: &Value) -> String {
    let mut args = args.clone();
    if let Some(o) = args.as_object_mut() {
        o.remove("cursor");
    }
    json!([name, args]).to_string()
}
fn limit(args: &Value) -> Result<usize, Error> {
    let limit = args
        .get("limit")
        .map_or(Some(50), Value::as_u64)
        .filter(|v| (1..=100).contains(v))
        .ok_or(Error::Invalid("invalid page limit"))?;
    usize::try_from(limit).map_err(|_| Error::Invalid("invalid page limit"))
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
        let state = Store::open(root.path()).unwrap().read_current().unwrap();
        let mut views = Views::new("acme-resolution-memo".into());
        let id = views
            .retain(&context, &state.candidates["project"], &state.operator)
            .unwrap();
        let captured = views.get(&id).unwrap();
        let candidate = captured.source().unwrap();
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
        let state = Store::open(root.path()).unwrap().read_current().unwrap();
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
    fn active_handle_admission_charges_its_retained_coverage_description() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let mut views = Views::new("acme-descriptor-budget".into());
        let id = views.active(&context).unwrap();
        let captured = views.get(&id).unwrap();
        let artifact = &captured.cached.as_ref().unwrap().artifact;
        let description = artifact.inspection_byte_len().unwrap();
        assert!(description > 0);
        let ordinary = json!([captured.candidate, captured.metadata])
            .to_string()
            .len()
            + usize::try_from(captured.candidate["length"].as_u64().unwrap()).unwrap()
            + usize::try_from(artifact.byte_len()).unwrap();
        assert_eq!(views.handles[&id].bytes, ordinary + description);
        assert_eq!(views.bytes, ordinary + description);
    }

    #[test]
    fn pages_preserve_envelopes_and_cursor_binding_for_large_retained_lists() {
        let mut views = Views::new("acme-pages".into());
        let rows: Vec<_> = (0..1000)
            .map(|id| json!({"id":id,"detail":"é\\\"".repeat(1000)}))
            .collect();
        let first = views
            .page(
                "acme",
                &json!({"limit":2}),
                json!({"rows":rows,"metadata":{"complete":true}}),
                "rows",
            )
            .unwrap();
        assert_eq!(first["rows"].as_array().unwrap().len(), 2);
        assert_eq!(first["rows"][0]["id"], 0);
        assert_eq!(first["metadata"], json!({"complete":true}));
        let args = json!({"limit":2,"cursor":first["next_cursor"]});
        assert!(matches!(
            views.resume("different", &args),
            Err(Error::CursorUnavailable)
        ));
        let second = views.resume("acme", &args).unwrap();
        assert_eq!(second["rows"][0]["id"], 2);
        assert_eq!(second["metadata"], first["metadata"]);
        assert!(crate::json::fits(&second, 1024 * 1024));
        let oversized = json!({"rows":[{"detail":"x".repeat(1024 * 1024)}]});
        assert!(views.page("acme", &json!({}), oversized, "rows").is_err());
    }
}
