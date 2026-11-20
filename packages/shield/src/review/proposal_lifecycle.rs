//! Bounded review interactions; retained evidence is never a queue of approvals.
use super::{
    Context, Error, clock,
    model::{Proposal, State},
    storage::Store,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

/// How long a closed review stays readable for outcome reconciliation.
const RECOVERY_MS: u64 = 24 * 60 * 60 * 1000;
pub(super) mod drafts;
#[cfg(test)]
mod tests;

/// A pending review lives as long as the connection that prepared it; age never
/// closes it. `settled_at_ms` starts the recovery window once it closes.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Interaction {
    connection: Option<String>,
    /// Stored under its first name so existing checkpoints read unchanged.
    #[serde(rename = "expires_at_ms")]
    settled_at_ms: u64,
}
impl Interaction {
    fn legacy(row: &Proposal, now: u64) -> Self {
        use canonic_registry_kit::timestamp::{parse, unix_seconds};
        let created = parse(&row.created_at)
            .and_then(|parts| u64::try_from(unix_seconds(parts)).ok())
            .and_then(|seconds| seconds.checked_mul(1000));
        Self {
            connection: None,
            settled_at_ms: created.unwrap_or(now),
        }
    }
    pub(super) fn new(context: &Context) -> Result<Self, Error> {
        Ok(Self {
            connection: context.connection_id.clone(),
            settled_at_ms: clock::now_ms()?,
        })
    }
    pub(super) fn owns(&self, context: &Context) -> bool {
        self.connection == context.connection_id
    }
}

/// The kernel releases this lease on process death. Paths and PIDs grant nothing.
pub(super) struct Lease {
    pub(super) root: String,
    pub(super) connection: String,
    file: Option<File>,
    path: PathBuf,
}
fn lease_path(root: &str, connection: &str) -> PathBuf {
    crate::layout::state(Path::new(root))
        .join("interactions")
        .join(format!(
            "{}.lock",
            crate::policy::fingerprint(connection.as_bytes())
        ))
}
impl Lease {
    pub(super) fn new(context: &Context) -> Result<Self, Error> {
        Store::open(Path::new(&context.project_root))?
            .with_interaction_lock(|| Self::create(context))
    }
    fn create(context: &Context) -> Result<Self, Error> {
        let connection = context
            .connection_id
            .as_ref()
            .ok_or(Error::Invalid("review connection required"))?;
        let path = lease_path(&context.project_root, connection);
        let directory = path.parent().ok_or(Error::Invalid("review lease path"))?;
        match fs::create_dir(directory) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
                }
                #[cfg(windows)]
                crate::windows_security::make_private(directory)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let meta = fs::symlink_metadata(directory)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::Invalid("review lease directory must be contained"));
        }
        // Creation and sweeping share the publication lock, so an unlocked
        // newly-created lease cannot be mistaken for a dead connection.
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str().and_then(|name| name.strip_suffix(".lock")) else {
                continue;
            };
            if name.len() != 64 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            // Another live connection may remove its lease during shutdown.
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if !meta.is_file() || meta.file_type().is_symlink() {
                continue;
            }
            if let Ok(file) = OpenOptions::new().read(true).write(true).open(entry.path())
                && file.try_lock().is_ok()
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        #[cfg(windows)]
        crate::windows_security::make_private(&path)?;
        file.try_lock().map_err(|_| Error::Locked)?;
        Ok(Self {
            root: context.project_root.clone(),
            connection: connection.clone(),
            file: Some(file),
            path,
        })
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}
fn alive_with(root: &str, connection: &str, cleanup: bool) -> bool {
    let path = lease_path(root, connection);
    if !path.parent().is_some_and(|dir| {
        fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
    }) {
        return false;
    }
    if !fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink()) {
        return false;
    }
    let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
        return false;
    };
    match file.try_lock() {
        Err(std::fs::TryLockError::WouldBlock) => true,
        Ok(()) => {
            if cleanup {
                let _ = fs::remove_file(path);
            }
            false
        }
        Err(std::fs::TryLockError::Error(_)) => false,
    }
}

/// Exact capture freshness is explicit, outside ordinary authority callbacks.
#[cfg(test)]
pub(super) fn refresh(state: &mut State) {
    refresh_inner(state, true);
}

/// Preflight only the selected live capture. Publication still revalidates its
/// exact complete file fingerprint inside the serialized commit boundary.
pub(in crate::review) fn validate_capture(root: &str, row: &Proposal) -> Result<(), Error> {
    for (name, expected) in row.reviewed_paths.iter().flat_map(|paths| paths.iter()) {
        crate::diagnostics::trace::count("proposal.capture_file_reads", 1);
        let bytes = super::changes::project_files::read_file(Path::new(root), name)?;
        let actual = bytes.map(|bytes| {
            crate::diagnostics::trace::count("proposal.capture_read_bytes", bytes.len() as u64);
            crate::policy::fingerprint(bytes.as_bytes())
        });
        if actual != *expected {
            return Err(Error::StaleSnapshot);
        }
    }
    Ok(())
}

/// Derive lifecycle metadata without hashing projections or reclaiming files.
/// Exact capture freshness remains an apply/mutation precondition.
pub(super) fn refresh_inspection(state: &mut State) {
    refresh_inner(state, false);
}

fn refresh_inner(state: &mut State, exact: bool) {
    let now = clock::best_effort_ms();
    drafts::reclaim(&mut state.policy_drafts, now);
    super::web_reviews::reclaim(&mut state.web_reviews, now);
    for row in &mut state.proposals {
        if row.status != "pending" {
            // Stable original timestamps let even a full legacy checkpoint age
            // out without a successful intervening write or an approval queue.
            if row.interaction.is_none() {
                row.interaction = Some(Interaction::legacy(row, now));
            }
            continue;
        }
        // The files the proposal writes decide whether it still applies. One
        // an earlier release prepared stores none and is prepared again.
        let files_changed = exact && validate_capture(&state.project_root, row).is_err();
        // A checkpoint-only read has not followed the files and knows no
        // revision; the files' hashes are then the only comparison.
        let revision_changed =
            !state.policy_revision.is_empty() && row.policy_revision != state.policy_revision;
        let base_changed = revision_changed || row.files.is_empty() || files_changed;
        // Only an exact refresh may unlink a dead connection's lease file.
        let reason = match &row.interaction {
            _ if base_changed => Some(("stale", "base_changed")),
            None => Some(("withdrawn", "legacy_interaction")),
            Some(interaction) => interaction
                .connection
                .as_ref()
                .is_some_and(|id| !alive_with(&state.project_root, id, exact))
                .then_some(("withdrawn", "connection_closed")),
        };
        if let Some((status, reason)) = reason {
            close(row, status, reason);
            if row.interaction.is_none() {
                row.interaction = Some(Interaction::legacy(row, now));
            }
        }
    }
}

/// Keep a full day after a review closes for outcome reconciliation. The
/// receipt of the write that produced the current policy survives
/// independently of this recovery window.
pub(super) fn reclaim(state: &mut State) {
    let now = clock::best_effort_ms();
    let revision = state.policy_revision.as_str();
    state.proposals.retain(|row| {
        row.status == "pending"
            || row.receipt["policy_revision"] == revision
            || row
                .interaction
                .as_ref()
                .is_none_or(|i| now <= i.settled_at_ms.saturating_add(RECOVERY_MS))
    });
}
pub(super) fn close(row: &mut Proposal, status: &str, reason: &str) {
    if row.status == "pending" {
        row.status = status.into();
        row.closed_reason = Some(reason.into());
        if let Some(interaction) = &mut row.interaction {
            interaction.settled_at_ms = clock::best_effort_ms();
        }
    }
}
/// Start the recovery window when a review is applied.
pub(super) fn applied(row: &mut Proposal) {
    row.status = "applied".into();
    if let Some(interaction) = &mut row.interaction {
        interaction.settled_at_ms = clock::best_effort_ms();
    }
}
/// Whether the review a `policy_apply` call names is still pending. State that
/// cannot be read counts as pending, so its question is kept.
pub(crate) fn still_pending(context: &Context, args: &Value) -> bool {
    let adapter = crate::agents::review::for_context(context);
    let Ok(state) =
        Store::open(Path::new(&context.project_root)).and_then(|store| store.read_checkpoint())
    else {
        return true;
    };
    state
        .proposals
        .iter()
        .find(|row| adapter.identifies(args, &row.id))
        .is_some_and(|row| row.status == "pending")
}
pub(super) fn owns(row: &Proposal, context: &Context) -> bool {
    row.retry_owner == context.retry_owner()
        && row.interaction.as_ref().is_some_and(|i| i.owns(context))
}

/// Reconcile under the publication lock before closing a host outcome.
pub(crate) fn settle(
    context: &Context,
    args: &Value,
    status: &str,
    reason: &str,
) -> Result<Value, Error> {
    let adapter = crate::agents::review::for_context(context);
    Store::open(Path::new(&context.project_root))?.update_checkpoint(context, &|| Ok(()), |state| {
        state.check_operations()?;
        let row = state.proposals.iter_mut().find(|p| adapter.identifies(args, &p.id)).ok_or(Error::Missing)?;
        if row.status == "applied" { return Ok(json!({"proposal_id":row.id,"status":"applied","receipt":row.receipt,"operator":row.operator})); }
        if !owns(row, context) { return Err(Error::ApprovalRequired); }
        close(row, status, reason);
        Ok(row.summary())
    })
}
/// A task ending never closes another session's review or a committed receipt.
pub(crate) fn end_session(root: &Path, agent: &str, session: &str) -> Result<(), Error> {
    let context = super::open(root)?;
    Store::open(root)?.update_checkpoint_if_changed(&context, &|| Ok(()), |state| {
        state.check_operations()?;
        let mut changed = false;
        for row in &mut state.proposals {
            if row.status == "pending"
                && row.provenance["agent"] == agent
                && row.provenance["session_id"] == session
            {
                close(row, "withdrawn", "task_ended");
                changed = true;
            }
        }
        Ok(changed)
    })
}
