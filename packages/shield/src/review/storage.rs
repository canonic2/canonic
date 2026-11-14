//! One bounded atomic project-state owner; locks are never stolen from writers.
pub(in crate::review) mod authority;
mod locking;
pub(in crate::review) mod private_files;
pub(in crate::review) mod registry_trust;
pub(crate) mod retired;
mod upgrades;
use super::{Checkpoint, Context, Error, model::State};
use locking::Lock;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(test)]
use std::{thread, time::Duration};
pub(in crate::review) use upgrades::report::recent as recent_upgrade;
const MAX_STATE: usize = 16 * 1024 * 1024;
type ComparisonCapture = (
    State,
    crate::configuration::Evaluator,
    Vec<(crate::policy::Scope, crate::configuration::Snapshot)>,
);

#[cfg(test)]
mod tests;

pub(crate) struct Store {
    directory: PathBuf,
    root: String,
    writer_owned: Mutex<Option<std::thread::ThreadId>>,
}
/// A store-local witness for conversion reached from an existing writer.
struct Writer<'a> {
    lock: Option<Lock>,
    owned: &'a Mutex<Option<std::thread::ThreadId>>,
}
impl Drop for Writer<'_> {
    fn drop(&mut self) {
        if self.lock.is_some() {
            *self
                .owned
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            drop(self.lock.take());
        }
    }
}
impl Writer<'_> {
    fn release(mut self) -> std::io::Result<()> {
        // Forget before unlocking: another thread may acquire this same Store
        // immediately afterwards, and its witness must not be cleared by Drop.
        *self
            .owned
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.lock.take().expect("owned writer").release()
    }
}
/// Operator fields only earlier identity-bound formats wrote; dropped on load.
const LEGACY_FIELDS: &[&str] = &[
    "runtime_incarnation",
    "administration_origin",
    "portable_admin_unverified",
    "portable_lineage_unverified",
    "policy_consent",
    "policy_consent_key",
    "policy_consent_version",
    "portable_policy",
    "policy_lineage",
    "policy_reuse",
];

/// A loaded checkpoint as this release keeps it: without the folder identity
/// and consent records earlier formats wrote, and with only this folder's
/// grants. Nothing here grants anything; the policy files are the policy.
fn admit(mut state: State) -> State {
    state.recorded_root = None;
    let expired = state.operator.get("upgrade_notice").is_some()
        && upgrades::report::recent(&state.operator).is_none();
    if let Some(fields) = state.operator.as_object_mut() {
        for field in LEGACY_FIELDS {
            fields.remove(*field);
        }
        // The conversion notice is shown for a while, then dropped.
        if expired {
            fields.remove("upgrade_notice");
        }
    }
    super::read_approval::retain_local(&mut state);
    super::run_approval::retain_local(&mut state);
    state
}

struct Temporary {
    path: PathBuf,
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Store {
    fn checkpoint_present(&self) -> Result<bool, Error> {
        match fs::symlink_metadata(self.directory.join("state.json")) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    pub(in crate::review) fn root(&self) -> &str {
        &self.root
    }
    fn location(project: &Path) -> Result<Self, Error> {
        let root = project.canonicalize()?;
        if !root.is_dir() {
            return Err(Error::Invalid("project must be a directory"));
        }
        let root_text = root
            .to_str()
            .ok_or(Error::Invalid("project must be UTF-8"))?
            .to_owned();
        let directory = root.join(".shield").join("review");
        for p in [root.join(".shield"), directory.clone()] {
            match fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                    return Err(Error::Invalid(
                        "review directories cannot be links or files",
                    ));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(Self {
            directory,
            root: root_text,
            writer_owned: Mutex::new(None),
        })
    }
    /// Open this folder's runtime state. A folder whose policy files arrived
    /// by clone, worktree or copy gets empty runtime state on first use: the
    /// files are its policy, so no enrollment step stands in between.
    pub(crate) fn open(project: &Path) -> Result<Self, Error> {
        let store = Self::location(project)?;
        if !store.checkpoint_present()? {
            if Path::new(&store.root).join(".shield/config.yml").is_file() {
                match Self::initialize_runtime(project) {
                    Ok(_) | Err(Error::Invalid("review project already enrolled")) => {}
                    Err(error) => return Err(error),
                }
            }
            if !store.directory.is_dir() {
                return Err(Error::Missing);
            }
        }
        #[cfg(windows)]
        crate::windows_security::check_private(&store.directory)?;
        Ok(store)
    }
    /// Locate retained state for inspection without adopting source-only
    /// consent, repairing a checkout epoch or publishing any derived files.
    pub(crate) fn inspect(project: &Path) -> Result<Self, Error> {
        let store = Self::location(project)?;
        if !store.directory.is_dir() {
            return Err(Error::Missing);
        }
        #[cfg(windows)]
        crate::windows_security::check_private(&store.directory)?;
        Ok(store)
    }
    #[cfg(test)]
    pub(crate) fn initialize(
        project: &Path,
        candidates: BTreeMap<String, Value>,
        revision: &str,
    ) -> Result<Value, Error> {
        let operator =
            serde_json::json!({"sources":{},"default":null,"installed":[],"projection":null});
        let written = Self::write_fixture_files(project, &candidates)?;
        let result = Self::initialize_with(
            project,
            candidates,
            revision,
            operator,
            false,
            BTreeMap::default(),
        )?;
        Self::follow_fixture_files(project, written)?;
        Ok(result)
    }
    /// A test project's policy is its files, as in a real project: write the
    /// candidate's files when the fixture has not written its own. Returns
    /// whether it wrote them.
    #[cfg(test)]
    fn write_fixture_files(
        project: &Path,
        candidates: &BTreeMap<String, Value>,
    ) -> Result<bool, Error> {
        let Some(candidate) = candidates.get("project") else {
            return Ok(false);
        };
        if project.join(".shield/config.yml").exists() || candidate.get("config").is_none() {
            return Ok(false);
        }
        let merged = crate::configuration::layers::merged(candidate)?;
        let text = |field: &str| candidate[field].as_str().map(str::to_owned);
        let mut files = BTreeMap::new();
        for (path, field) in [
            (Some("config.yml"), "config"),
            (Some("config.local.yml"), "local_config"),
            (merged["rules"].as_str(), "rules"),
            (merged["local_rules"].as_str(), "local_rules"),
        ] {
            if let (Some(path), Some(source)) = (path, text(field)) {
                files.insert(path.to_owned(), source);
            }
        }
        for (selections, artifacts) in [("packs", "packs"), ("disabled", "disabled")] {
            for (index, row) in merged[selections]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let (Some(path), Some(source)) =
                    (row["path"].as_str(), candidate[artifacts][index].as_str())
                {
                    files.insert(path.to_owned(), source.to_owned());
                }
            }
        }
        let root = project.canonicalize()?.to_string_lossy().into_owned();
        super::publication::projections::materialize(
            &Context::new(String::new(), root, "cli"),
            &files,
        )?;
        Ok(true)
    }
    #[cfg(test)]
    pub(crate) fn initialize_active(
        project: &Path,
        candidates: BTreeMap<String, Value>,
        revision: &str,
        operator: Value,
    ) -> Result<Value, Error> {
        let written = Self::write_fixture_files(project, &candidates)?;
        let result = Self::initialize_with(
            project,
            candidates,
            revision,
            operator,
            true,
            BTreeMap::default(),
        )?;
        Self::follow_fixture_files(project, written)?;
        Ok(result)
    }
    /// Files a fixture wrote from its own candidate are what the state holds,
    /// as after a real enrollment: record them as followed.
    #[cfg(test)]
    fn follow_fixture_files(project: &Path, written: bool) -> Result<(), Error> {
        if !written {
            return Ok(());
        }
        let store = Self::open(project)?;
        let lock = store.lock(&|| Ok(()))?;
        let mut state = store.read_checkpoint()?;
        state.followed_files = Some(super::files::digest(project)?);
        store.write(&mut state)?;
        Ok(lock.release()?)
    }
    pub(crate) fn initialize_verified(
        project: &Path,
        candidates: BTreeMap<String, Value>,
        revision: &str,
        operator: Value,
        checkpoints: BTreeMap<String, Value>,
    ) -> Result<Value, Error> {
        Self::initialize_with(project, candidates, revision, operator, true, checkpoints)
    }

    /// Empty runtime state for a folder whose policy files arrived without
    /// it. Files that cannot be read are still the folder's policy: requests
    /// ask with the problem, so the state starts from an empty candidate.
    fn initialize_runtime(project: &Path) -> Result<Value, Error> {
        let config = project.join(".shield/config.yml");
        let candidate = match super::integrations::project_sources::capture(&config) {
            Ok(bundle) => json!(bundle),
            Err(_) => json!(crate::configuration::Bundle::capture(
                r#"{"schema_version":1,"packs":[]}"#,
                &[],
                None
            )?),
        };
        let merged = crate::configuration::layers::merged(&candidate)?;
        let mut operator = json!({"sources":{},"default":null,"projection":null,
            "installed":super::integrations::project_sources::installed(&merged, &candidate)?});
        let disabled = super::integrations::project_sources::disabled(&merged, &candidate)?;
        if !disabled.is_empty() {
            operator["disabled"] = json!(disabled);
        }
        let candidates = BTreeMap::from([("project".into(), candidate)]);
        let revision =
            crate::configuration::Evaluator::revision_of(&super::snapshots(&candidates)?)?;
        let result = Self::initialize_with(
            project,
            candidates,
            &revision,
            operator,
            false,
            BTreeMap::default(),
        )?;
        // Follow the files as they are, as every later write does, so the
        // first change starts from the files' own revision.
        let store = Self::location(project)?;
        let lock = store.lock(&|| Ok(()))?;
        let mut state = store.read_checkpoint()?;
        let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
        if super::files::sync(&context, &mut state).unwrap_or(false) {
            store.write(&mut state)?;
        }
        lock.release()?;
        Ok(result)
    }

    /// Keep the review folder out of version control with its own
    /// `.gitignore`. An existing one must already exclude everything and be a
    /// regular file.
    fn ignore_review_folder(&self) -> Result<(), Error> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        let ignore_path = self.directory.join(".gitignore");
        match options.open(&ignore_path) {
            Ok(mut ignore) => ignore.write_all(b"*\n")?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&ignore_path)?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(Error::Invalid("review ignore file cannot be a link"));
                }
                if fs::read_to_string(&ignore_path)?.trim() != "*" {
                    return Err(Error::Invalid(
                        "review ignore file must exclude local state",
                    ));
                }
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    /// Write a folder's first runtime state. `verify_registries` refuses
    /// policy naming a revoked pack, as a person's enrollment does.
    fn initialize_with(
        project: &Path,
        candidates: BTreeMap<String, Value>,
        revision: &str,
        mut operator: Value,
        verify_registries: bool,
        registry_checkpoints: BTreeMap<String, Value>,
    ) -> Result<Value, Error> {
        let store = Self::location(project)?;
        fs::create_dir_all(&store.directory)?;
        #[cfg(windows)]
        crate::windows_security::make_private(&store.directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&store.directory, fs::Permissions::from_mode(0o700))?;
        }
        if store.checkpoint_present()? {
            return Err(Error::Invalid("review project already enrolled"));
        }
        store.ignore_local_state();
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Invalid("clock before epoch"))?
            .as_nanos();
        let project_id = crate::policy::fingerprint(
            history::identifier(
                "checkout",
                &crate::policy::fingerprint(format!("{epoch}:{}", std::process::id()).as_bytes()),
            )?
            .as_bytes(),
        );
        if let Some(fields) = operator.as_object_mut() {
            fields.remove("policy_lineage");
        }
        let mut state = State {
            unconverted: false,
            earlier_format: false,
            followed_files: None,
            history_generation: history::generation()?,
            history_owner: None,
            history_available: true,
            pending_history: vec![],
            history_rewrite: None,
            history_control: false,
            history_evicted: false,
            operator,
            schema_version: 1,
            project_id: project_id.clone(),
            project_root: store.root.clone(),
            recorded_root: None,
            sequence: 0,
            policy_revision: revision.into(),
            candidates,
            retired_policy_revision: None,
            retired_candidates: None,
            retired_followed_files: None,
            registry_checkpoints,
            retired_active: None,
            read_requests: vec![],
            run_requests: vec![],
            session_grants: super::session_grants::Registry::default(),
            hook_requests: vec![],
            terminal_receipts: super::authorization_receipts::Registry::default(),
            proposals: vec![],
            policy_drafts: vec![],
            dismissed_reviews: vec![],
            events: vec![],
            retired_configuration_changes: None,
            authorization_events: vec![],
            settings: super::settings::current(
                Path::new(&store.root),
                &super::settings::Legacy::default(),
            ),
            history_previous_limit: None,
            legacy_history_enabled: None,
            legacy_history_limit: None,
            legacy_enforcement: None,
            legacy_settings: super::settings::Legacy::default(),
        };
        let mut _registry_guard = None;
        if verify_registries {
            let effective = crate::configuration::recorded_effective(
                &state.candidates["project"],
                &state.policy_revision,
            )?;
            let guard = super::registry_trust::guard_effective(&state.operator, &effective)?;
            guard.bind_modes(&mut state.operator)?;
            _registry_guard = Some(guard);
        }
        let lock = store.lock(&|| Ok(()))?;
        if store.checkpoint_present()? {
            return Err(Error::Invalid("review project already enrolled"));
        }
        store.ignore_review_folder()?;
        store.write(&mut state)?;
        lock.release()?;
        Ok(
            json!({"schema_version":1,"project_id":project_id,"policy_revision":revision,"mode":"local-review","enforcement":"not-installed"}),
        )
    }
    #[cfg(test)]
    pub(crate) fn read(&self) -> Result<State, Error> {
        self.read_evaluated().map(|(state, _)| state)
    }
    /// Keep the validated active plan for workflows that also need its prefixes.
    #[cfg(test)]
    pub(in crate::review) fn read_evaluated(
        &self,
    ) -> Result<(State, crate::configuration::Evaluator), Error> {
        let state = self.read_unprepared()?;
        let evaluator = crate::configuration::Evaluator::new_for_revision(
            super::snapshots(&state.candidates)?,
            &state.policy_revision,
        )?;
        Self::validate_evaluated(&state, &evaluator)?;
        Ok((state, evaluator))
    }

    /// A proposal compares the same validated source it used to prepare the
    /// base plan. Retain that capture instead of parsing every pack twice.
    pub(in crate::review) fn read_for_comparison(state: State) -> Result<ComparisonCapture, Error> {
        let _trace = crate::diagnostics::trace::span("proposal.capture_base");
        let snapshots = super::snapshots(&state.candidates)?;
        let evaluator = match Self::cached_evaluator(&state) {
            Some(evaluator) => evaluator,
            None => crate::configuration::Evaluator::new_for_revision(
                snapshots.clone(),
                &state.policy_revision,
            )?,
        };
        Self::validate_evaluated(&state, &evaluator)?;
        Ok((state, evaluator, snapshots))
    }

    /// The current policy, following the policy files. Optional history stays
    /// with history readers.
    pub(in crate::review) fn read_current(&self) -> Result<State, Error> {
        self.read_following_files()
    }

    /// Read the checkpoint with its copy of the policy brought up to the policy
    /// files in memory. Readers never take the writer lock; the next writer
    /// persists the same result. Unchanged files cost one read and hash.
    pub(in crate::review) fn read_following_files(&self) -> Result<State, Error> {
        let mut state = self.read_checkpoint()?;
        let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
        super::files::sync(&context, &mut state)?;
        Ok(state)
    }

    /// [`Self::read_following_files`] for inspection: the diagnostic checkpoint,
    /// never converted or written, brought up to the policy files in memory.
    pub(in crate::review) fn read_inspection_following_files(&self) -> Result<State, Error> {
        let (mut state, _) = self.read_diagnostic_checkpoint()?;
        let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
        super::files::sync(&context, &mut state)?;
        Ok(state)
    }

    pub(in crate::review) fn read_current_evaluated(
        &self,
    ) -> Result<(State, crate::configuration::Evaluator), Error> {
        let state = self.read_current()?;
        let evaluator = match Self::cached_evaluator(&state) {
            Some(evaluator) => evaluator,
            None => crate::configuration::Evaluator::new_for_revision(
                super::snapshots(&state.candidates)?,
                &state.policy_revision,
            )?,
        };
        Self::validate_evaluated(&state, &evaluator)?;
        Ok((state, evaluator))
    }

    /// Reuse immutable content only after the caller verified current source.
    pub(in crate::review) fn cached_evaluator(
        state: &State,
    ) -> Option<crate::configuration::Evaluator> {
        // The policy files' plan, when it holds this revision. Loading
        // compiles and saves it on a miss, so later requests reuse it.
        super::files::load(Path::new(&state.project_root))
            .ok()
            .map(|policy| policy.evaluator)
            .filter(|evaluator| evaluator.revision() == state.policy_revision)
    }

    /// Rebuild a proposal's candidates from the file texts it stores, bound to
    /// the digests it recorded. One an earlier release prepared stores no
    /// texts and cannot be applied.
    pub(in crate::review) fn hydrate_proposal(
        &self,
        proposal: &mut super::model::Proposal,
    ) -> Result<(), Error> {
        if !proposal.candidate.is_null() {
            return Ok(());
        }
        if proposal.files.is_empty() {
            return Err(Error::CandidateUnavailable);
        }
        let root = Path::new(&self.root);
        let candidate = super::files::write::bundle(root, &proposal.files, true)?;
        let base = super::files::write::bundle(root, &proposal.files, false)?;
        if super::candidate_identity(&candidate) != proposal.candidate_digest
            || super::candidate_identity(&base) != proposal.base_digest
        {
            return Err(Error::Invalid("proposal files differ from their review"));
        }
        proposal.candidate = candidate;
        proposal.base_candidate = base;
        Ok(())
    }

    fn validate_evaluated(
        state: &State,
        evaluator: &crate::configuration::Evaluator,
    ) -> Result<(), Error> {
        // The policy files decide requests; an earlier activation record is
        // not enforced, so it needs no validation here.
        if evaluator.revision() != state.policy_revision {
            return Err(Error::Invalid(
                "review revision differs from retained policy",
            ));
        }
        Ok(())
    }

    // Full fixture inspection; production readers select current source, one
    // proposal or history explicitly instead of hydrating every retained object.
    #[cfg(test)]
    pub(in crate::review) fn read_unprepared(&self) -> Result<State, Error> {
        let _trace = crate::diagnostics::trace::span("state.hydrate_all");
        let mut state = self.read_history()?;
        let context = Context::new(state.project_id.clone(), state.project_root.clone(), "cli");
        super::files::sync(&context, &mut state)?;
        Ok(state)
    }
    /// Read-only diagnostic snapshot including optional history health.
    #[cfg(test)]
    pub(in crate::review) fn read_diagnostics(&self) -> Result<(State, bool), Error> {
        let (mut state, pending) = self.read_diagnostic_checkpoint()?;
        // This explicit reader may inspect history; ordinary checkpoint
        // diagnosis never enters either history path.
        if pending {
            let raw = crate::json::parse(&self.checkpoint_bytes()?)
                .map_err(|_| Error::Invalid("corrupt historical review state"))?;
            history::inspect_legacy(self, &raw, &mut state);
        } else {
            let _history = crate::diagnostics::trace::span("history.read");
            let view =
                history::lock_diagnostics(self).and_then(|_guard| history::capture(self, &state));
            history::finish(view, &mut state);
        }
        Ok((state, pending))
    }

    /// Enrollment metadata, without current policy or history hydration after
    /// any first-use policy reuse or runtime relocation has completed.
    /// The boolean identifies an uncommitted upgrade; this state must never be
    /// used to authorize execution or a mutation. Historical conversion validates
    /// its retained authority in memory without publishing files.
    pub(in crate::review) fn read_diagnostic_checkpoint(&self) -> Result<(State, bool), Error> {
        let _trace = crate::diagnostics::trace::span("state.checkpoint");
        crate::diagnostics::trace::count("state.checkpoint_reads", 1);
        let bytes = self.checkpoint_bytes()?;
        // An earlier format converts in memory only; nothing is written.
        let (state, pending) = match self.decode(&bytes) {
            Ok(state) => {
                let pending = self.convertible(&state);
                (state, pending)
            }
            Err(_) => (upgrades::inspect(self, &bytes)?, true),
        };
        let mut state = admit(state);
        super::settings::load(Path::new(&self.root), &mut state);
        Self::validate_checkpoint(&state)?;
        super::proposal_lifecycle::refresh_inspection(&mut state);
        Ok((state, pending))
    }

    /// Decode a current-format checkpoint read from this folder. The folder is
    /// where it was found, never where the checkpoint says it was.
    fn decode(&self, bytes: &[u8]) -> Result<State, Error> {
        let mut state = upgrades::checkpoint(bytes)?;
        state.project_root.clone_from(&self.root);
        state.earlier_format = state.recorded_root.is_some()
            || state.retired_active.is_some()
            || state.retired_candidates.is_some()
            || state.retired_policy_revision.is_some()
            || state.retired_followed_files.is_some()
            || state.retired_configuration_changes.is_some()
            || state
                .operator
                .as_object()
                .is_some_and(|fields| LEGACY_FIELDS.iter().any(|key| fields.contains_key(*key)));
        Ok(state)
    }

    /// Whether a decoded checkpoint is in a format an earlier release wrote and
    /// the one conversion can rewrite now. Settings it holds move only into
    /// policy files; without them they stay and keep applying.
    fn convertible(&self, state: &State) -> bool {
        state.earlier_format
            || (super::settings::held(state)
                && Path::new(&self.root).join(".shield/config.yml").is_file())
    }

    // Current enrollment, authorization and source identities, without preparing policy.
    pub(in crate::review) fn read_checkpoint(&self) -> Result<State, Error> {
        self.read_settled_checkpoint()
    }

    fn read_settled_checkpoint(&self) -> Result<State, Error> {
        let _trace = crate::diagnostics::trace::span("state.checkpoint");
        crate::diagnostics::trace::count("state.checkpoint_reads", 1);
        let bytes = self.checkpoint_bytes()?;
        // Every earlier format, current-family or historical, takes the one
        // conversion; a checkpoint written by this release decodes as it is.
        let state = match self.decode(&bytes) {
            Ok(state) if !self.convertible(&state) => state,
            _ => upgrades::convert(self, &bytes)?,
        };
        let mut state = admit(state);
        super::settings::load(Path::new(&self.root), &mut state);
        Self::validate_checkpoint(&state)?;
        super::proposal_lifecycle::refresh_inspection(&mut state);
        Ok(state)
    }

    fn checkpoint_bytes(&self) -> Result<Vec<u8>, Error> {
        let file = self.directory.join("state.json");
        #[cfg(windows)]
        crate::windows_security::check_private(&file)?;
        let metadata = fs::symlink_metadata(&file)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::Invalid("review state must be a regular file"));
        }
        let mut bytes = vec![];
        fs::File::open(file)?
            .take((MAX_STATE + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_STATE {
            return Err(crate::policy::Error::Limit.into());
        }
        crate::diagnostics::trace::count("state.read_bytes", bytes.len() as u64);
        Ok(bytes)
    }

    fn validate_checkpoint(state: &State) -> Result<(), Error> {
        if state.project_id.len() != 64
            || state.project_root.len() > 4096
            || state.project_root.contains('\0')
            || !Path::new(&state.project_root).is_absolute()
            || !state.project_id.bytes().all(|b| b.is_ascii_hexdigit())
            || state.sequence > u64::MAX - 1024
        {
            return Err(Error::Invalid("invalid state identity"));
        }
        state.check(&Context::new(
            state.project_id.clone(),
            state.project_root.clone(),
            "cli",
        ))?;
        super::read_approval::validate(state)?;
        super::run_approval::validate(state)?;
        super::hook_approval::validate(state)?;
        super::session_grants::validate(state)?;
        state.terminal_receipts.validate()?;
        Ok(())
    }
    /// Captures the history view under the lock, then verifies, correlates and
    /// retains it after releasing the lock.
    pub(in crate::review) fn read_history(&self) -> Result<State, Error> {
        let _trace = crate::diagnostics::trace::span("history.read");
        let (mut state, view) = {
            let (state, _guard) = self.observation_checkpoint(false)?;
            let view = history::capture(self, &state);
            (state, view)
        };
        history::finish(view, &mut state);
        Ok(state)
    }
    /// Appends one callback's observations; no earlier record is read.
    pub(in crate::review) fn observe(
        &self,
        context: &Context,
        operation: impl FnOnce(&mut State) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        let (mut state, _guard) = self.observation_checkpoint(true)?;
        state.check(context)?;
        if !state.settings.history_enabled {
            return Ok(json!({"status":"disabled","request_id":null,"event_id":null}));
        }
        let result = operation(&mut state)?;
        if let Err(error) = history::publish(self, &state) {
            history::mark_failure(self, &state);
            return Err(error);
        }
        Ok(result)
    }
    /// Capture optional observations under their own lock. Legacy conversion
    /// needs authority -> history ordering, so release history before converting.
    fn observation_checkpoint(&self, optional: bool) -> Result<(State, fs::File), Error> {
        // One conversion attempt: a conversion that cannot write leaves the
        // earlier checkpoint readable in memory.
        let mut converted = false;
        loop {
            let guard = if optional {
                history::lock_optional(self)?
            } else {
                history::lock(self)?
            };
            let _trace = crate::diagnostics::trace::span("state.checkpoint");
            crate::diagnostics::trace::count("state.checkpoint_reads", 1);
            let bytes = self.checkpoint_bytes()?;
            if let Ok(state) = self.decode(&bytes) {
                if !converted && self.convertible(&state) {
                    converted = true;
                    drop(guard);
                    self.read_checkpoint()?;
                    continue;
                }
                let mut state = admit(state);
                super::settings::load(Path::new(&self.root), &mut state);
                Self::validate_checkpoint(&state)?;
                super::proposal_lifecycle::refresh_inspection(&mut state);
                return Ok((state, guard));
            }
            drop(guard);
            self.read_checkpoint()?;
        }
    }
    pub(in crate::review) fn with_interaction_lock<T>(
        &self,
        operation: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        let lock = self.lock(&|| Ok(()))?;
        let result = operation()?;
        lock.release()?;
        Ok(result)
    }
    fn lock(&self, checkpoint: Checkpoint<'_>) -> Result<Writer<'_>, Error> {
        let _trace = crate::diagnostics::trace::span("authority.lock_wait");
        let lock = locking::acquire(&self.directory, checkpoint)?;
        *self
            .writer_owned
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(std::thread::current().id());
        Ok(Writer {
            lock: Some(lock),
            owned: &self.writer_owned,
        })
    }
    /// Rare legacy/relocation normalization must acquire the projection lease
    /// before authority. Drop this attempted writer, normalize once, then retry.
    fn locked_state(
        &self,
        checkpoint: Checkpoint<'_>,
        read: fn(&Self) -> Result<State, Error>,
    ) -> Result<(Writer<'_>, State), Error> {
        for attempt in 0..2 {
            let lock = self.lock(checkpoint)?;
            match read(self) {
                Ok(state) => return Ok((lock, state)),
                Err(Error::StaleSnapshot) if attempt == 0 => {
                    drop(lock);
                    self.read_checkpoint()?;
                }
                Err(error) => return Err(error),
            }
        }
        Err(Error::StaleSnapshot)
    }
    #[cfg(test)]
    pub(crate) fn update(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&mut State) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        self.update_inner(context, checkpoint, operation, Self::read)
    }
    pub(in crate::review) fn update_checkpoint(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&mut State) -> Result<Value, Error>,
    ) -> Result<Value, Error> {
        self.update_inner(context, checkpoint, operation, Self::read_checkpoint)
    }
    /// Optional observational capture may discover under lock that nothing changed.
    pub(in crate::review) fn update_checkpoint_if_changed(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&mut State) -> Result<bool, Error>,
    ) -> Result<(), Error> {
        let (lock, mut state) = self.locked_state(checkpoint, Self::read_checkpoint)?;
        state.check(context)?;
        checkpoint()?;
        let synced = super::files::sync(context, &mut state)?;
        if operation(&mut state)? || synced {
            checkpoint()?;
            state.compact_authorization();
            self.write(&mut state)?;
        }
        lock.release()?;
        Ok(())
    }
    fn update_inner(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&mut State) -> Result<Value, Error>,
        read: fn(&Self) -> Result<State, Error>,
    ) -> Result<Value, Error> {
        let (lock, mut state) = self.locked_state(checkpoint, read)?;
        state.check(context)?;
        // Every write starts from the policy files as they are now, so its
        // comparisons and publication see the files, not an older copy.
        super::files::sync(context, &mut state)?;
        state.compact_authorization();
        checkpoint()?;
        let result = operation(&mut state)?;
        if serde_json::to_vec(&result)
            .map_err(|_| Error::Invalid("response encoding"))?
            .len()
            > 1_048_576
        {
            return Err(crate::policy::Error::Limit.into());
        }
        checkpoint()?;
        state.compact_authorization();
        super::proposal_lifecycle::refresh_inspection(&mut state);
        self.write(&mut state)?;
        lock.release()?;
        self.ignore_local_state();
        Ok(result)
    }
    /// Keeps review state and the in-project cache out of version control, so
    /// committing .shield shares only its policy files. Never replaces a file.
    fn ignore_local_state(&self) {
        let Some(path) = self
            .directory
            .parent()
            .map(|shield| shield.join(".gitignore"))
        else {
            return;
        };
        match fs::symlink_metadata(&path) {
            Err(_) => {
                let _ = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .and_then(|mut file| std::io::Write::write_all(&mut file, LOCAL_STATE_IGNORE));
            }
            // An existing regular file keeps its lines; missing entries are appended.
            Ok(metadata) if metadata.is_file() => {
                let Ok(existing) = fs::read_to_string(&path) else {
                    return;
                };
                let missing: String = String::from_utf8_lossy(LOCAL_STATE_IGNORE)
                    .lines()
                    .filter(|line| !existing.lines().any(|l| l.trim() == *line))
                    .flat_map(|line| [line, "\n"])
                    .collect();
                if !missing.is_empty() {
                    let separator = if existing.is_empty() || existing.ends_with('\n') {
                        ""
                    } else {
                        "\n"
                    };
                    let _ = fs::OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .and_then(|mut file| {
                            std::io::Write::write_all(
                                &mut file,
                                format!("{separator}{missing}").as_bytes(),
                            )
                        });
                }
            }
            Ok(_) => {}
        }
    }
    pub(in crate::review) fn inspect_checkpoint<T>(
        &self,
        context: &Context,
        checkpoint: Checkpoint<'_>,
        operation: impl FnOnce(&State) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let (lock, state) = self.locked_state(checkpoint, Self::read_checkpoint)?;
        state.check(context)?;
        checkpoint()?;
        let result = operation(&state)?;
        lock.release()?;
        Ok(result)
    }
    /// Publish minimum authority before the short callback, without releasing the
    /// activation lock. A publication/release failure never restores a grant.
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    pub(crate) fn transition_handoff<T>(
        &self,
        context: &Context,
        prepare: impl FnOnce(&mut State) -> Result<(Value, bool), Error>,
        handoff: impl FnOnce(&Value) -> T,
    ) -> Result<(Value, T), Error> {
        let (lock, mut state) = self.locked_state(&|| Ok(()), Self::read_checkpoint)?;
        state.check(context)?;
        let (decision, changed) = prepare(&mut state)?;
        if changed {
            self.write(&mut state)?;
        }
        let result = handoff(&decision);
        lock.release()?;
        Ok((decision, result))
    }
    fn write(&self, state: &mut State) -> Result<(), Error> {
        let _trace = crate::diagnostics::trace::span("state.write");
        // Settings an earlier checkpoint held move into the policy files with
        // the first write that can; until then the checkpoint keeps them.
        if super::settings::pending(state) {
            let _ = super::settings::convert(state);
        }
        let history_lock = if state.history_control {
            history::lock_control(self)
        } else {
            history::lock_optional(self)
        };
        let history_guard = match history_lock {
            Ok(guard) => Some(guard),
            Err(error) if state.history_control => return Err(error),
            Err(_) => None,
        };
        // One derived migration receipt has reserved capacity so a full older
        // journal upgrades without deleting any retained grant evidence.
        let upgrade_receipts = state
            .authorization_events
            .iter()
            .filter(|event| {
                event["kind"] == "compiled_cache_prepared"
                    && event["operator"]["mechanism"] == "derived-cache"
                    && event["details"]["migration"] == "optional-compiled-v1"
            })
            .count();
        if upgrade_receipts > 1 || state.authorization_events.len() > 1024 + upgrade_receipts {
            return Err(crate::policy::Error::Limit.into());
        }
        let bytes = history::encode(self, state, history_guard.is_some())?;
        if bytes.len() > MAX_STATE {
            return Err(crate::policy::Error::Limit.into());
        }
        let tick = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = Temporary {
            path: self
                .directory
                .join(format!(".state-{}-{tick}.tmp", std::process::id())),
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        let mut file = options.open(&temporary.path)?;
        #[cfg(test)]
        tests::observe(tests::Stage::Created);
        file.write_all(&bytes)?;
        #[cfg(test)]
        tests::observe(tests::Stage::Written);
        sync(&file)?;
        #[cfg(test)]
        tests::observe(tests::Stage::Synced);
        drop(file);
        replace(&temporary.path, &self.directory.join("state.json"))?;
        #[cfg(test)]
        tests::observe(tests::Stage::Renamed);
        #[cfg(unix)]
        sync(&fs::File::open(&self.directory)?)?;
        #[cfg(test)]
        tests::observe(tests::Stage::Complete);
        // Every state writer holds the writer lock, so no other `.state-*.tmp`
        // is still being written.
        remove_temporaries(&self.directory, |name| state_temporary(name).is_some());
        // Optional evidence cannot roll back an already durable authority
        // transition. Without pending observations nothing was lost, so a busy
        // history lock is not a failure.
        let pending = state.settings.history_enabled && !state.pending_history.is_empty();
        if pending && (history_guard.is_none() || history::publish(self, state).is_err()) {
            history::mark_failure(self, state);
        }
        if history_guard.is_some() {
            history::cleanup(self, state);
        }
        // Optional user consent is published by the activation owner only
        // after releasing this authority writer, with source graph pins held.
        Ok(())
    }
}

pub(in crate::review) mod history;
pub(in crate::review) mod objects;
const LOCAL_STATE_IGNORE: &[u8] =
    b"review/\ncache/\nconfig.local.yml\nrules.local.yml\nshield.local.lock\n";

pub(in crate::review) fn private_directory(path: &Path) -> Result<(), Error> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid("private directory cannot be a link or file"));
    }
    #[cfg(windows)]
    crate::windows_security::make_private(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
/// Flushes `file`, or a directory opened as a file, before or after a rename.
///
/// macOS uses plain `fsync`: `File::sync_all` issues `F_FULLFSYNC` there,
/// which also empties the drive's write cache and serializes behind every
/// other process's flush. Other platforms use `File::sync_all`.
pub(in crate::review) fn sync(file: &fs::File) -> std::io::Result<()> {
    let _trace = crate::diagnostics::trace::span("storage.sync");
    crate::diagnostics::trace::count("storage.sync_calls", 1);
    #[cfg(target_os = "macos")]
    {
        nix::unistd::fsync(file).map_err(std::io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}

/// Replace an owned temporary file. Windows requests write-through for the
/// rename; Unix callers separately flush the containing directory.
pub(in crate::review) fn replace(temporary: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        if [temporary, target]
            .iter()
            .any(|path| path.as_os_str().encode_wide().any(|unit| unit == 0))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "replacement path contains NUL",
            ));
        }
        atomicwrites::replace_atomic(temporary, target)
    }
    #[cfg(not(windows))]
    fs::rename(temporary, target)
}
/// The writer PID of a `.state-<pid>-<nanoseconds>.tmp` name.
fn state_temporary(name: &str) -> Option<u32> {
    stamp_pid(name.strip_prefix(".state-")?.strip_suffix(".tmp")?, 2)
}
/// The writer PID of a temporary-name stamp `<pid>-<n>…` with exactly `fields`
/// decimal fields, as Shield's writers name their temporaries.
pub(in crate::review) fn stamp_pid(stamp: &str, fields: usize) -> Option<u32> {
    let parts: Vec<_> = stamp.split('-').collect();
    if parts.len() != fields
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    parts[0].parse().ok()
}
/// Removes the regular files in `directory` whose names `stale` selects. A file
/// that cannot be removed stays for a later writer.
pub(in crate::review) fn remove_temporaries(directory: &Path, stale: impl Fn(&str) -> bool) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_file())
            && entry.file_name().to_str().is_some_and(&stale)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}
/// Removes temporaries in `directory` whose writer, the PID `writer` reads from
/// a Shield temporary name, is no longer running.
pub(in crate::review) fn remove_dead_temporaries(
    directory: &Path,
    writer: impl Fn(&str) -> Option<u32>,
) {
    remove_temporaries(directory, |name| {
        writer(name).is_some_and(|pid| !writer_running(pid))
    });
}
/// Whether the process that named a temporary may still be writing it. This
/// process, and a PID whose exit cannot be established, count as running, so a
/// live writer's temporary is never removed. A reused PID only keeps a leftover.
pub(in crate::review) fn writer_running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    {
        i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .is_none_or(|pid| {
                rustix::process::test_kill_process(pid) != Err(rustix::io::Errno::SRCH)
            })
    }
    #[cfg(windows)]
    {
        use winsafe::{HPROCESS, co};
        if pid == 0 {
            return true;
        }
        match HPROCESS::OpenProcess(co::PROCESS::SYNCHRONIZE, false, pid) {
            Ok(process) => process.WaitForSingleObject(Some(0)) != Ok(co::WAIT::OBJECT_0),
            Err(error) if error == co::ERROR::INVALID_PARAMETER => false,
            Err(_) => true,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}
/// A PID whose process has exited and been reaped, for leftover temporaries.
#[cfg(all(test, unix))]
pub(in crate::review) fn exited_pid() -> u32 {
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("exiting child");
    let pid = child.id();
    child.wait().expect("reaped child");
    pid
}
#[cfg(all(test, windows))]
pub(in crate::review) fn exited_pid() -> u32 {
    let mut child =
        std::process::Command::new(std::env::current_exe().expect("fixture executable"))
            .arg("--list")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("owned exiting child");
    let pid = child.id();
    assert!(child.wait().expect("reaped child").success());
    pid
}
pub(in crate::review) fn private(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = options;
}
