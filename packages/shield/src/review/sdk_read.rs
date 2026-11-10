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
    Context, Error,
    model::State,
    storage::{Store, objects},
};
use model::{CONTEXT_LIMIT, CONTEXT_TTL_MS, ContextRecord, Profile};
use proofs::Secret;
use std::{fs::File, path::Path};

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
        assert!(super::RegisteredReadConsumer::admit(std::path::Path::new(".")).is_err());
        assert!(super::mint(std::path::Path::new("."), "").is_err());
    }
}

fn now() -> Result<u64, Error> {
    super::clock::now_ms()
}

fn current(project: &Path) -> Result<State, Error> {
    let store = Store::inspect(project)?;
    let (state, _) = store.read_diagnostic_checkpoint()?;
    if state.project_root != store.root() || state.legacy_preview.is_some() {
        return Err(Error::StaleSnapshot);
    }
    state.check_operations()?;
    state
        .active
        .as_ref()
        .ok_or(Error::Missing)?
        .validate_binding(&state)?;
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
pub(crate) struct IssuedReadContext(Secret);
impl IssuedReadContext {
    pub(crate) fn launch_value(&self) -> &str {
        self.0.launch_value()
    }
}

/// An admitted fixed-mapping consumer, never constructible from wire claims.
/// Shared leases survive final read and reply handoff; close waits for them.
pub(crate) struct RegisteredReadConsumer {
    context: Context,
    profile: Profile,
    record: ContextRecord,
    secret: Secret,
    binding: String,
    _profile_lease: File,
    _context_lease: File,
    _binary_lease: File,
}

impl RegisteredReadConsumer {
    pub(crate) fn admit(project: &Path) -> Result<Self, Error> {
        ensure_supported()?;
        let secret = secret()?;
        Self::admit_secret(project, secret)
    }

    fn admit_secret(project: &Path, secret: Secret) -> Result<Self, Error> {
        ensure_supported()?;
        let profile_directory = files::directory("sdk-read-profiles", false)?;
        let profile_lease = objects::read_lease(&profile_directory)?;
        let context_directory = files::directory("sdk-read-contexts", false)?;
        let context_lease = objects::read_lease(&context_directory)?;
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
            secret,
            _profile_lease: profile_lease,
            _context_lease: context_lease,
            _binary_lease: binary_lease,
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
        let time = now()?;
        if state.project_id != self.record.project_id
            || state.project_root != self.record.project_root
            || state.policy_lineage() != self.profile.lineage
            || self.profile.mapping_digest != mapping::digest()
            || self.record.profile != self.profile.key()?
            || self.record.binary.digest() != self.profile.binary_digest
            || state.operator["runtime_incarnation"]["digest"].as_str()
                != Some(self.record.incarnation.as_str())
            || time < self.record.created_at_ms
            || time >= self.record.expires_at_ms
        {
            return Err(Error::Invalid("SDK read context is stale or unavailable"));
        }
        Ok(())
    }
}

/// Actual factory startup may normalize this checkout; ordinary admit never does.
pub(crate) fn mint(project: &Path, mapping_digest: &str) -> Result<IssuedReadContext, Error> {
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
    let _profile_lease = objects::read_lease(&profile_directory)?;
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
    let time = now()?;
    contexts
        .contexts
        .retain(|_, row| row.expires_at_ms > time && row.created_at_ms <= time);
    if contexts.contexts.len() >= CONTEXT_LIMIT {
        return Err(Error::Overloaded);
    }
    let secret = Secret::generate()?;
    let record = ContextRecord {
        consumer_id: Secret::generate()?.hash(),
        project_id: state.project_id.clone(),
        incarnation: state.operator["runtime_incarnation"]["digest"]
            .as_str()
            .ok_or(Error::Invalid("SDK read incarnation unavailable"))?
            .into(),
        project_root: state.project_root.clone(),
        profile: profile_key,
        created_at_ms: time,
        expires_at_ms: time
            .checked_add(CONTEXT_TTL_MS)
            .ok_or(Error::Invalid("SDK read clock out of range"))?,
        binary,
    };
    record.validate()?;
    // Recheck operation continuity before publishing the private context.
    let after = current(Path::new(&context.project_root))?;
    if after.project_id != state.project_id
        || after.policy_lineage() != state.policy_lineage()
        || after.operator["runtime_incarnation"] != state.operator["runtime_incarnation"]
    {
        return Err(Error::StaleSnapshot);
    }
    contexts.contexts.insert(secret.hash(), record);
    files::write(&directory, &contexts)?;
    Ok(IssuedReadContext(secret))
}

/// Closing a token never normalizes a checkout or authorizes another operation.
pub(crate) fn close(project: &Path) -> Result<(), Error> {
    let proof = secret()?;
    close_secret(project, &proof)
}

fn close_secret(project: &Path, proof: &Secret) -> Result<(), Error> {
    let directory = files::directory("sdk-read-contexts", false)?;
    let _lease = objects::Directory::enter(&directory)?;
    let mut contexts = files::contexts(&directory)?;
    let key = proof.hash();
    if let Some(record) = contexts.contexts.get(&key)
        && Path::new(&record.project_root) != project.canonicalize()?
    {
        return Err(Error::Missing);
    }
    if contexts.contexts.remove(&key).is_some() {
        files::write(&directory, &contexts)?;
    }
    Ok(())
}
