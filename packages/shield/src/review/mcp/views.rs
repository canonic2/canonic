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
                    configuration::Evaluator::new(snapshots(&BTreeMap::from([(
                        "project".into(),
                        self.source()?,
                    )]))?)
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
    data: Arc<Value>,
    field: String,
    offset: usize,
}
#[derive(Clone)]
pub(super) struct Views {
    seed: String,
    sequence: u64,
    bytes: usize,
    cursor_bytes: usize,
    handles: BTreeMap<String, Arc<Captured>>,
    identities: BTreeMap<String, String>,
    cursors: BTreeMap<String, Cursor>,
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
        }
    }
    fn id(&mut self, kind: &str) -> String {
        self.sequence += 1;
        format!("{kind}-{}-{}", self.seed, self.sequence)
    }
    pub(super) fn retain(&mut self, candidate: &Value, metadata: &Value) -> Result<String, Error> {
        self.retain_prepared(candidate, metadata, None)
    }
    /// Reserve an exact capture with the plan already prepared for its review.
    pub(super) fn retain_prepared(
        &mut self,
        candidate: &Value,
        metadata: &Value,
        prepared: Option<configuration::Evaluator>,
    ) -> Result<String, Error> {
        if candidate.is_null() {
            return Err(Error::CandidateUnavailable);
        }
        let bytes = json!([candidate, metadata]).to_string();
        let key = identity(candidate, metadata);
        if let Some(id) = self.identities.get(&key) {
            return Ok(id.clone());
        }
        if self.handles.len() >= 256 || self.bytes.saturating_add(bytes.len()) > HANDLE_BYTES {
            return Err(Error::Overloaded);
        }
        let evaluator = if let Some(prepared) = prepared {
            prepared
        } else {
            configuration::Evaluator::new(snapshots(&BTreeMap::from([(
                "project".into(),
                candidate.clone(),
            )]))?)?
        };
        let length = bytes.len().saturating_add(evaluator.compiled_bytes().len());
        if self.bytes.saturating_add(length) > HANDLE_BYTES {
            return Err(Error::Overloaded);
        }
        let id = self.id("candidate");
        self.bytes += length;
        self.handles.insert(
            id.clone(),
            Arc::new(Captured {
                candidate: candidate.clone(),
                metadata: metadata.clone(),
                evaluator: OnceLock::from(evaluator),
                cached: None,
            }),
        );
        self.identities.insert(key, id.clone());
        Ok(id)
    }
    pub(super) fn active(&mut self, context: &Context) -> Result<String, Error> {
        let store = Store::open(Path::new(&context.project_root))?;
        let state = store.read_checkpoint()?;
        state.check(context)?;
        let active = state
            .active
            .as_ref()
            .ok_or(Error::Invalid("project has no confirmed active policy"))?;
        active.validate_binding(&state)?;
        let key = identity(active.project_candidate()?, &state.operator);
        if let Some(id) = self.identities.get(&key) {
            return Ok(id.clone());
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
        if self.handles.len() >= 256 || self.bytes.saturating_add(length) > HANDLE_BYTES {
            return Err(Error::Overloaded);
        }
        let id = self.id("candidate");
        self.bytes += length;
        self.handles.insert(
            id.clone(),
            Arc::new(Captured {
                candidate,
                metadata: state.operator,
                evaluator: OnceLock::new(),
                cached: Some(Cached {
                    artifact: compiled.clone(),
                    root: state.project_root,
                    project_id: state.project_id,
                    revision: active.revision().into(),
                }),
            }),
        );
        self.identities.insert(key, id.clone());
        Ok(id)
    }
    pub(super) fn get(&self, id: &str) -> Result<Arc<Captured>, Error> {
        self.handles
            .get(id)
            .cloned()
            .ok_or(Error::CandidateUnavailable)
    }
    pub(super) fn proposal(
        &mut self,
        context: &Context,
        id: &str,
    ) -> Result<(String, String), Error> {
        let state = Store::open(Path::new(&context.project_root))?.read()?;
        state.check(context)?;
        let row = state
            .proposals
            .iter()
            .find(|p| p.id == id)
            .ok_or(Error::Missing)?;
        let before = self.retain(&row.base_candidate, &row.base_metadata)?;
        let after = self.retain(&row.candidate, &row.candidate_metadata)?;
        Ok((before, after))
    }
    pub(super) fn page(
        &mut self,
        name: &str,
        args: &Value,
        data: Value,
        field: &str,
    ) -> Result<Value, Error> {
        if args.get("cursor").is_some() {
            return self.resume(name, args);
        }
        if !data[field].is_array() {
            return Ok(data);
        }
        let limit = limit(args)?;
        let size = data.to_string().len();
        let retains = data[field]
            .as_array()
            .is_some_and(|rows| rows.len() > limit);
        if retains && self.cursor_bytes.saturating_add(size) > 64 * 1024 * 1024 {
            return Err(Error::Overloaded);
        }
        let result = self.slice(query(name, args), Arc::new(data), field, 0, limit)?;
        if retains {
            self.cursor_bytes += size;
        }
        Ok(result)
    }
    pub(super) fn resume(&mut self, name: &str, args: &Value) -> Result<Value, Error> {
        let cursor = self
            .cursors
            .get(required(args, "cursor")?)
            .cloned()
            .ok_or(Error::CursorUnavailable)?;
        if cursor.query != query(name, args) {
            return Err(Error::CursorUnavailable);
        }
        self.slice(
            cursor.query,
            cursor.data,
            &cursor.field,
            cursor.offset,
            limit(args)?,
        )
    }
    fn slice(
        &mut self,
        query: String,
        data: Arc<Value>,
        field: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value, Error> {
        let rows = data[field].as_array().ok_or(Error::CursorUnavailable)?;
        let end = offset.saturating_add(limit).min(rows.len());
        let mut page = data.as_ref().clone();
        page[field] = json!(&rows[offset..end]);
        if page.to_string().len() > 1024 * 1024 {
            return Err(crate::policy::Error::Limit.into());
        }
        let next = if end < rows.len() {
            if self.cursors.len() >= 256 || data.to_string().len() > 64 * 1024 * 1024 {
                return Err(Error::Overloaded);
            }
            let id = self.id("cursor");
            self.cursors.insert(
                id.clone(),
                Cursor {
                    query,
                    data,
                    field: field.into(),
                    offset: end,
                },
            );
            Some(id)
        } else {
            None
        };
        page["next_cursor"] = json!(next);
        if page.get("truncated").is_none() {
            page["truncated"] = json!(false);
        }
        Ok(page)
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
