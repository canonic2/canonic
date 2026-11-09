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
}
struct Cached {
    _sources: crate::review::storage::objects::Pins,
    artifact: crate::review::cache::Artifact,
    root: String,
    project_id: String,
    revision: String,
}
impl Captured {
    pub(super) fn revision(&self) -> Result<&str, Error> {
        self.cached.as_ref().map_or_else(
            || Ok(self.evaluator()?.revision()),
            |cached| Ok(cached.revision.as_str()),
        )
    }
    pub(super) fn evaluator(&self) -> Result<&configuration::Evaluator, Error> {
        if self.evaluator.get().is_none() {
            let cached = self.cached.as_ref().ok_or(Error::CandidateUnavailable)?;
            let evaluator = cached
                .artifact
                .load(&cached.root, &cached.project_id)
                .or_else(|_| {
                    // A retained handle must rebuild its own exact source, even after
                    // another generation becomes active. Never reload current files.
                    configuration::Evaluator::new_for_revision(
                        snapshots(&BTreeMap::from([("project".into(), self.source()?)]))?,
                        &cached.revision,
                    )
                    .map_err(Error::from)
                })?;
            if evaluator.revision() != cached.revision {
                return Err(Error::CandidateUnavailable);
            }
            let _ = self.evaluator.set(evaluator);
        }
        self.evaluator.get().ok_or(Error::CandidateUnavailable)
    }
    pub(super) fn source(&self) -> Result<Value, Error> {
        self.cached.as_ref().map_or_else(
            || Ok(self.candidate.clone()),
            |cached| Store::open(Path::new(&cached.root))?.captured_candidate(&self.candidate),
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
        self.retain_prepared(context, candidate, metadata, None, None)
    }
    /// Reserve an exact capture with the plan already prepared for its review.
    fn retain_prepared(
        &mut self,
        context: &Context,
        candidate: &Value,
        metadata: &Value,
        prepared: Option<configuration::Evaluator>,
        keep: Option<&str>,
    ) -> Result<String, Error> {
        let _trace = crate::diagnostics::trace::span("handle.retain");
        if candidate.is_null() {
            return Err(Error::CandidateUnavailable);
        }
        let key = identity(candidate, metadata);
        if let Some(id) = self.identities.get(&key).cloned() {
            let captured = self.get(&id)?;
            if captured.evaluator.get().is_none()
                && let Some(prepared) = prepared
            {
                // An active handle may have reserved bytes but not loaded its plan.
                // Keep the plan review just prepared instead of discarding it.
                let reserved = captured
                    .cached
                    .as_ref()
                    .map(|cached| usize::try_from(cached.artifact.byte_len()))
                    .transpose()
                    .map_err(|_| Error::Overloaded)?
                    .unwrap_or(0);
                let extra = prepared.compiled_bytes().len().saturating_sub(reserved);
                self.handle_room(context, extra, 0, &[Some(id.as_str()), keep])?;
                self.bytes += extra;
                self.handles
                    .get_mut(&id)
                    .ok_or(Error::CandidateUnavailable)?
                    .bytes += extra;
                let _ = captured.evaluator.set(prepared);
            }
            return Ok(id);
        }
        crate::diagnostics::trace::count("handle.admission_encodings", 1);
        let bytes = crate::json::encoded_len(&[candidate, metadata])
            .map_err(|_| Error::Invalid("candidate encoding"))?;
        let evaluator = if let Some(prepared) = prepared {
            prepared
        } else {
            configuration::Evaluator::new(snapshots(&BTreeMap::from([(
                "project".into(),
                candidate.clone(),
            )]))?)?
        };
        let length = bytes.saturating_add(evaluator.compiled_bytes().len());
        self.handle_room(context, length, 1, &[keep])?;
        let id = self.id("candidate");
        self.bytes += length;
        self.handles.insert(
            id.clone(),
            Handle {
                key: key.clone(),
                bytes: length,
                used: self.sequence,
                captured: Arc::new(Captured {
                    candidate: candidate.clone(),
                    metadata: metadata.clone(),
                    evaluator: OnceLock::from(evaluator),
                    cached: None,
                }),
            },
        );
        self.identities.insert(key, id.clone());
        Ok(id)
    }
    pub(super) fn active(&mut self, context: &Context) -> Result<String, Error> {
        let store = Store::open(Path::new(&context.project_root))?;
        let (state, pins) = store.read_pinned_checkpoint()?;
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
            .saturating_add(usize::try_from(compiled.byte_len()).map_err(|_| Error::Overloaded)?);
        self.handle_room(context, length, 1, &[])?;
        let id = self.id("candidate");
        self.bytes += length;
        self.handles.insert(
            id.clone(),
            Handle {
                key: key.clone(),
                bytes: length,
                used: self.sequence,
                captured: Arc::new(Captured {
                    candidate,
                    metadata: state.operator,
                    evaluator: OnceLock::new(),
                    cached: Some(Cached {
                        _sources: pins,
                        artifact: compiled.clone(),
                        root: state.project_root,
                        project_id: state.project_id,
                        revision: active.revision().into(),
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
