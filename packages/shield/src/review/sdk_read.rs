//! Explicit reviewed SDK read consumers, independent of native registrations.
mod binary;
mod files;
mod mapping;
mod model;
mod proofs;
mod setup;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests;

use super::{
    Checkpoint, Context, Error,
    model::State,
    storage::{Store, objects},
};
use model::{CONTEXT_LIMIT, ContextRecord, Profile};
use proofs::Secret;
use std::{
    fs::File,
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
};

pub(crate) use setup::Prepared;
pub(crate) const TOKEN_ENV: &str = "SHIELD_SDK_READ_CONTEXT_TOKEN";

fn ensure_supported() -> Result<(), Error> {
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        Ok(())
    } else {
        Err(Error::Invalid(
            "this registered SDK profile requires the supported macOS or Linux parent executable receipt; existing SDK and native integrations are unchanged",
        ))
    }
}

#[cfg(all(test, windows))]
mod unsupported_tests {
    #[test]
    fn new_sdk_profile_refuses_without_a_supported_parent_binary_guard() {
        assert!(super::ensure_supported().is_err());
        assert!(
            super::RegisteredReadConsumer::admit(std::path::Path::new("."), &|| Ok(())).is_err()
        );
        assert!(super::mint(std::path::Path::new("."), "").is_err());
    }
}

fn now() -> Result<u64, Error> {
    super::clock::now_ms()
}

fn current(project: &Path) -> Result<State, Error> {
    let store = Store::inspect(project)?;
    let (state, _) = store.read_diagnostic_checkpoint()?;
    if state.project_root != store.root() || state.unconverted {
        return Err(Error::StaleSnapshot);
    }
    // Each read is decided by the policy files when it arrives; admission needs
    // no separately activated policy.
    state.check_operations()?;
    Ok(state)
}

fn secret() -> Result<Secret, Error> {
    Secret::parse(
        std::env::var(TOKEN_ENV)
            .map_err(|_| Error::Invalid("SDK read context proof unavailable"))?,
    )
}

/// The only public token projection is the dedicated owned launch pipe.
/// This type deliberately has no serde/Debug/display implementation.
struct IssuedReadContext(Secret, File);
impl IssuedReadContext {
    fn launch_value(&self) -> &str {
        self.0.launch_value()
    }

    fn close(self, project: &Path) -> Result<(), Error> {
        let Self(secret, owner) = self;
        close_secret(project, &secret, Some(owner))
    }
}

/// An admitted fixed-mapping consumer, never constructible from wire claims.
/// Shared leases survive final read and reply handoff; close waits for them.
pub(crate) struct RegisteredReadConsumer {
    context: Context,
    profile: Profile,
    record: ContextRecord,
    context_directory: PathBuf,
    secret: Secret,
    binding: String,
    _profile_lease: File,
    _context_lease: File,
    _binary_lease: File,
    #[cfg(test)]
    _test_owner: Option<File>,
}

impl RegisteredReadConsumer {
    pub(crate) fn admit(project: &Path, checkpoint: Checkpoint<'_>) -> Result<Self, Error> {
        ensure_supported()?;
        let secret = secret()?;
        Self::admit_with_secret(project, secret, checkpoint)
    }

    #[cfg(test)]
    fn admit_secret(project: &Path, secret: Secret) -> Result<Self, Error> {
        Self::admit_with_secret(project, secret, &|| Ok(()))
    }

    fn admit_with_secret(
        project: &Path,
        secret: Secret,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Self, Error> {
        ensure_supported()?;
        let profile_directory = files::directory("sdk-read-profiles", false)?;
        let profile_lease = objects::read_lease(&profile_directory, Some(checkpoint))?;
        let context_directory = files::directory("sdk-read-contexts", false)?;
        let context_lease = objects::read_lease(&context_directory, Some(checkpoint))?;
        let record = files::contexts(&context_directory)?
            .contexts
            .remove(&secret.hash())
            .ok_or(Error::Missing)?;
        let profile = files::profiles(&profile_directory)?
            .profiles
            .remove(&record.profile)
            .ok_or(Error::Missing)?;
        let binary_lease = record.binary.admit_current()?;
        let state = current(project)?;
        let mut context = Context::new(
            state.project_id.clone(),
            state.project_root.clone(),
            "sdk-read",
        );
        context.agent = Some("claude-sdk".into());
        context.connection_id = Some(record.consumer_id.clone());
        context.source = "sdk-callback".into();
        let consumer = Self {
            context,
            profile,
            binding: record.binding()?,
            record,
            context_directory,
            secret,
            _profile_lease: profile_lease,
            _context_lease: context_lease,
            _binary_lease: binary_lease,
            #[cfg(test)]
            _test_owner: None,
        };
        consumer.check_state(&state)?;
        Ok(consumer)
    }

    pub(crate) fn context(&self) -> &Context {
        &self.context
    }
    pub(crate) fn consumer_id(&self) -> &str {
        &self.record.consumer_id
    }
    pub(crate) fn binding_digest(&self) -> &str {
        &self.binding
    }

    pub(crate) fn retry_reference(&self, request: &str, binding: &str) -> Result<String, Error> {
        self.secret.retry_reference(request, binding)
    }

    pub(crate) fn verify_retry_reference(
        &self,
        request: &str,
        binding: &str,
        reference: &str,
    ) -> Result<(), Error> {
        self.secret
            .verify_retry_reference(request, binding, reference)
    }

    /// Also callable while the existing read-approval writer owns State.
    /// No Store reentry, policy hydration, matcher construction or lock upgrade.
    pub(in crate::review) fn check_state(&self, state: &State) -> Result<(), Error> {
        state.check(&self.context)?;
        state.check_operations()?;
        if state.project_id != self.record.project_id
            || state.project_root != self.record.project_root
            || state.policy_lineage() != self.profile.lineage
            || self.profile.mapping_digest != mapping::digest()
            || self.record.profile != self.profile.key()?
            || self.record.binary.digest() != self.profile.binary_digest
            || self.record.expires_at_ms.is_some()
            || !files::context_alive(&self.context_directory, &self.record.consumer_id)?
        {
            return Err(Error::Invalid("SDK read context is stale or unavailable"));
        }
        Ok(())
    }
}

/// Actual factory startup may normalize this checkout; ordinary admit never does.
fn mint(project: &Path, mapping_digest: &str) -> Result<IssuedReadContext, Error> {
    ensure_supported()?;
    if mapping_digest != mapping::digest() {
        return Err(Error::Invalid(
            "SDK read mapping differs from the reviewed build",
        ));
    }
    let context = super::open(project)?;
    let state = current(Path::new(&context.project_root))?;
    let binary = binary::Receipt::capture()?;
    let profile = Profile {
        lineage: state.policy_lineage().into(),
        binary_digest: binary.digest().into(),
        mapping_digest: mapping_digest.into(),
    };
    let profile_key = profile.key()?;
    let profile_directory = files::directory("sdk-read-profiles", false)?;
    let _profile_lease = objects::read_lease(&profile_directory, None)?;
    if files::profiles(&profile_directory)?
        .profiles
        .get(&profile_key)
        != Some(&profile)
    {
        return Err(Error::Invalid(
            "SDK read profile is not reviewed; run shield sdk setup --adapter claude-read-only",
        ));
    }
    let directory = files::directory("sdk-read-contexts", true)?;
    let _lease = objects::Directory::enter(&directory)?;
    let mut contexts = files::contexts(&directory)?;
    let mut abandoned = Vec::new();
    for (key, row) in &contexts.contexts {
        if row.expires_at_ms.is_some() || !files::context_alive(&directory, &row.consumer_id)? {
            files::remove_owner(&directory, &row.consumer_id)?;
            abandoned.push(key.clone());
        }
    }
    for key in abandoned {
        contexts.contexts.remove(&key);
    }
    if contexts.contexts.len() >= CONTEXT_LIMIT {
        return Err(Error::Overloaded);
    }
    let secret = Secret::generate()?;
    let record = ContextRecord {
        consumer_id: Secret::generate()?.hash(),
        project_id: state.project_id.clone(),
        incarnation: None,
        project_root: state.project_root.clone(),
        profile: profile_key,
        created_at_ms: now()?,
        expires_at_ms: None,
        binary,
    };
    record.validate()?;
    // Recheck the project before publishing the private context.
    let after = current(Path::new(&context.project_root))?;
    if after.project_id != state.project_id || after.policy_lineage() != state.policy_lineage() {
        return Err(Error::StaleSnapshot);
    }
    let consumer = record.consumer_id.clone();
    contexts.contexts.insert(secret.hash(), record);
    files::write(&directory, &contexts)?;
    // Publish the row and acquire its owner while holding the namespace mutex.
    // A crash before acquisition leaves an unowned row for the next mint.
    Ok(IssuedReadContext(
        secret,
        files::own_context(&directory, &consumer)?,
    ))
}

/// The owning application keeps stdin open after receiving the private token.
pub(crate) fn serve_owner(project: &Path, mapping_digest: &str) -> Result<(), Error> {
    let issued = mint(project, mapping_digest)?;
    let served = (|| {
        {
            let mut output = io::stdout().lock();
            writeln!(output, "{}", issued.launch_value())?;
            output.flush()?;
        }
        if io::stdin().lock().read(&mut [0])? != 0 {
            return Err(Error::Invalid("unexpected SDK owner input"));
        }
        Ok(())
    })();
    served.and(issued.close(project))
}

/// Closing a token never normalizes a checkout or authorizes another operation.
pub(crate) fn close(project: &Path) -> Result<(), Error> {
    let proof = secret()?;
    close_secret(project, &proof, None)
}

/// The closed context's unanswered reads go with it: no retry can reach them.
fn close_secret(project: &Path, proof: &Secret, owner: Option<File>) -> Result<(), Error> {
    let binding = {
        let directory = files::directory("sdk-read-contexts", false)?;
        let _lease = objects::Directory::enter(&directory)?;
        let mut contexts = files::contexts(&directory)?;
        let key = proof.hash();
        if let Some(record) = contexts.contexts.get(&key)
            && Path::new(&record.project_root) != project.canonicalize()?
        {
            return Err(Error::Missing);
        }
        let Some(record) = contexts.contexts.remove(&key) else {
            return Ok(());
        };
        // A factory can reclaim an unowned row. Release the owner only after
        // claiming that row under the namespace lease.
        drop(owner);
        files::remove_owner(&directory, &record.consumer_id)?;
        files::write(&directory, &contexts)?;
        record.binding()?
    };
    // Outside the context lease, which admission holds while it takes the store lock.
    let context = super::open(project)?;
    Store::open(project)?.update_authorization_if_changed(&context, |state| {
        Ok(super::read_approval::end_context(state, &binding))
    })
}
