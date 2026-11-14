//! Registry access, registration interpretation, retained catalog checkpoints and
//! the mapping of shared registry refusals to Shield errors.
//!
//! Sources, catalog authentication, bounded acquisition, checkpoint admission
//! and registration precedence come from `canonic_registry_kit`.
use super::project as state;
use crate::{
    configuration::settings::Values,
    policy,
    review::{Context, Error, storage::Store},
};
use canonic_registry_kit::{
    acquisition::{AcquisitionError, Fetcher},
    catalog::{Catalog, CatalogError, Entry},
    checkpoint::{CheckpointError, Checkpoints},
    names,
    registrations::{Effective, GlobalFile, MAX_REGISTRATIONS, RegistrationError, Registrations},
    source::{Source, SourceError},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// The built-in registry's reserved alias and pinned source identity.
pub(super) const BUILTIN: &str = "main";
const MAIN_URL: &str = "https://canonic2.github.io/shield-registry/";
const MAIN_KEY: &str = "d95d845212b35871db0edb962c1bf3ab8d076907c68a9235fb33d2b7b42983e8";
const MAIN_REGISTRY_ID: &str = "canonic-shield-main";
/// Overall deadline of each HTTPS request.
const DEADLINE: Duration = Duration::from_secs(10);
/// Largest artifact read from an HTTPS source.
const MAX_HOSTED_ARTIFACT: u64 = 1_048_576;

#[cfg(test)]
mod tests;

/// The production fetcher: public TLS roots and the per-request deadline.
pub(in crate::review) fn fetcher() -> Fetcher {
    Fetcher::new(DEADLINE)
}

/// The binary-pinned main registry origin and release signing root.
pub(super) fn main_source() -> Result<Source, Error> {
    Source::pinned(MAIN_URL, BUILTIN, MAIN_KEY, MAIN_REGISTRY_ID)
        .map_err(|_| Error::Invalid("main registry source invalid"))
}

/// Authenticates a source's catalog and admits it to the project's retained checkpoints.
pub(super) fn checked_catalog(context: &Context, source: &Source) -> Result<Catalog, Error> {
    context.progress.stage(super::progress::Stage::Catalog);
    let current = state::checkpoint(context)?;
    if let Some(catalog) =
        crate::review::registry_trust::checked_catalog(context, source, &current)?
    {
        super::pack_updates::retain(context, source, &catalog);
        return Ok(catalog);
    }
    let catalog = catalog(&context.fetcher, source)?;
    retain_catalog(context, &catalog)?;
    super::pack_updates::retain(context, source, &catalog);
    Ok(catalog)
}

/// Load only the selected pack's version metadata and retain its observed digests.
pub(super) fn checked_history(
    context: &Context,
    source: &Source,
    catalog: &mut Catalog,
    id: &names::PackId,
    version: Option<&semver::Version>,
) -> Result<(), Error> {
    context.progress.stage(super::progress::Stage::History);
    let before = catalog.entries().len();
    context
        .fetcher
        .history(source, catalog, id, version)
        .map_err(|error| acquisition_error(error, source.is_signed()))?;
    if catalog.entries().len() == before {
        Ok(())
    } else {
        retain_catalog(context, catalog)
    }
}

pub(super) fn history(
    fetcher: &Fetcher,
    source: &Source,
    catalog: &mut Catalog,
    id: &names::PackId,
) -> Result<(), Error> {
    fetcher
        .history(source, catalog, id, None)
        .map_err(|error| acquisition_error(error, source.is_signed()))
}

fn retain_catalog(context: &Context, catalog: &Catalog) -> Result<(), Error> {
    // Catalog admission changes only anti-rollback checkpoints, never policy or
    // grants. The checkpoint transaction verifies enrollment without compiling
    // the same active policy again during acquisition.
    Store::open(Path::new(&context.project_root))?.update_checkpoint(
        context,
        &|| Ok(()),
        |state| {
            let mut checkpoints: Checkpoints =
                serde_json::from_value(json!(state.registry_checkpoints))
                    .map_err(|_| Error::Invalid("invalid registry checkpoints"))?;
            let installed = state.operator["installed"]
                .as_array()
                .map_or(&[][..], Vec::as_slice);
            let protected = protected_releases(installed, catalog.source_identity())?;
            checkpoints
                .admit_retaining(catalog, &protected)
                .map_err(|error| checkpoint_error(&error))?;
            state.registry_checkpoints = retained(&checkpoints)?;
            Ok(json!({}))
        },
    )?;
    Ok(())
}

pub(super) fn protected_releases(
    rows: &[Value],
    source: &str,
) -> Result<BTreeMap<String, String>, Error> {
    rows.iter()
        .filter(|row| row["source_identity"] == source)
        .map(|row| {
            let id = row["id"]
                .as_str()
                .ok_or(Error::Invalid("installed pack identity missing"))?;
            let version = row["version"]
                .as_str()
                .ok_or(Error::Invalid("installed pack version missing"))?;
            let digest = row["digest"]
                .as_str()
                .ok_or(Error::Invalid("installed pack digest missing"))?;
            Ok((format!("{id}@{version}"), digest.to_owned()))
        })
        .collect()
}

/// Checkpoints in the stored state's shape.
pub(super) fn retained(checkpoints: &Checkpoints) -> Result<BTreeMap<String, Value>, Error> {
    serde_json::from_value(json!(checkpoints))
        .map_err(|_| Error::Invalid("invalid registry checkpoints"))
}

/// Authenticates a source's current catalog without retaining it.
pub(super) fn catalog(fetcher: &Fetcher, source: &Source) -> Result<Catalog, Error> {
    if let Some(catalog) = crate::review::registry_trust::catalog(fetcher, source)? {
        return Ok(catalog);
    }
    fetcher
        .catalog(source, SystemTime::now())
        .map_err(|error| acquisition_error(error, source.is_signed()))
}

/// Reads one release and checks it against its catalog digest, as UTF-8 text.
pub(super) fn artifact(fetcher: &Fetcher, source: &Source, entry: &Entry) -> Result<String, Error> {
    crate::review::registry_trust::check_entry(source, entry, true)?;
    let limit = if source.is_signed() {
        MAX_HOSTED_ARTIFACT
    } else {
        policy::MAX_DOCUMENT_BYTES as u64
    };
    let bytes = fetcher
        .artifact(source, entry, limit)
        .map_err(|error| acquisition_error(error, source.is_signed()))?;
    String::from_utf8(bytes).map_err(|_| {
        Error::Invalid(if source.is_signed() {
            "registry response must be UTF-8"
        } else {
            "registry artifact unreadable or oversized"
        })
    })
}

pub(super) fn global_file() -> Result<GlobalFile, Error> {
    Ok(GlobalFile::new(
        global_path(std::env::home_dir().map(PathBuf::into_os_string))?,
        BUILTIN,
    ))
}

/// The user's global registrations under an absolute home directory; a relative
/// or empty home path would otherwise resolve against the working directory.
fn global_path(home: Option<std::ffi::OsString>) -> Result<PathBuf, Error> {
    home.map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".shield/registries.json"))
        .ok_or(Error::Invalid("home directory unavailable"))
}

pub(super) fn global() -> Result<Registrations, Error> {
    global_file()?.read().map_err(registration_error)
}

/// A project's registrations: the `registries` key of its policy files.
pub(super) fn project(settings: &Values) -> Registrations {
    settings.registries.clone()
}

/// Built-in, global and, with a project, its registrations in effect.
pub(in crate::review) fn effective(context: Option<&Context>) -> Result<Effective, Error> {
    let project = context
        .map(|context| state::checkpoint(context).map(|s| project(&s.settings)))
        .transpose()?;
    merge(&global()?, project.as_ref())
}

pub(super) fn merge(
    global: &Registrations,
    project: Option<&Registrations>,
) -> Result<Effective, Error> {
    Effective::merge((BUILTIN, &main_source()?), global, project)
        .map_err(|_| Error::Invalid("invalid project sources"))
}

/// Resolve registrations from the settings a caller already read; no policy
/// or catalog loading.
pub(in crate::review) fn effective_from(settings: &Values) -> Result<Effective, Error> {
    merge(&global()?, Some(&project(settings)))
}

pub(in crate::review) fn list(context: Option<&Context>) -> Result<Value, Error> {
    let effective = effective(context)?;
    let sources: serde_json::Map<_, _> = effective
        .iter()
        .map(|(alias, source, scope)| {
            let mut value = json!(source);
            value["scope"] = json!(scope);
            (alias.to_owned(), value)
        })
        .collect();
    Ok(json!({"schema_version":1,"sources":sources,"default":effective.default_alias()}))
}

/// Discovers a new source and authenticates its first catalog for review.
pub(super) fn enrollment(fetcher: &Fetcher, location: &str) -> Result<Source, Error> {
    fetcher
        .discover(location, SystemTime::now())
        .map(|discovery| discovery.source)
        .map_err(|error| acquisition_error(error, location.starts_with("https://")))
}

/// Refuses an alias that cannot be enrolled or replaced, before network access.
pub(super) fn addable(registrations: &Registrations, alias: &str) -> Result<(), Error> {
    if !names::is_identifier(alias) {
        Err(Error::Invalid("invalid registry alias"))
    } else if alias == BUILTIN {
        Err(Error::Invalid("main is reserved"))
    } else if !registrations.sources.contains_key(alias)
        && registrations.sources.len() >= MAX_REGISTRATIONS
    {
        Err(policy::Error::Limit.into())
    } else {
        Ok(())
    }
}

pub(super) fn registration_error(error: RegistrationError) -> Error {
    match error {
        RegistrationError::InvalidAlias => Error::Invalid("invalid registry alias"),
        RegistrationError::Reserved(_) => Error::Invalid("main is reserved"),
        RegistrationError::Duplicate => Error::Invalid("registry alias already enrolled"),
        RegistrationError::Missing(_) => Error::Missing,
        RegistrationError::Limit | RegistrationError::TooLarge => policy::Error::Limit.into(),
        RegistrationError::Malformed => Error::Invalid("invalid global registry configuration"),
        RegistrationError::Identity(_) | RegistrationError::InvalidDefault => {
            Error::Invalid("invalid global registry settings")
        }
        RegistrationError::Link => Error::Invalid("global registry configuration cannot be a link"),
        RegistrationError::Path => Error::Invalid("global configuration parent"),
        RegistrationError::Changed => Error::Invalid("global registries changed; review again"),
        RegistrationError::CommittedRepair => Error::Invalid(
            "global registry change committed; configuration projection needs repair",
        ),
        RegistrationError::Locked(lock) => Error::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "global registries have a live or unverifiable legacy owner: {}; close other registry writers and inspect the marker's ownership before repairing it; an unknown owner is never removed automatically",
                lock.display()
            ),
        )),
        RegistrationError::NotDurable(error)
        | RegistrationError::LockRemains(_, error)
        | RegistrationError::RefusedLockRemains { source: error, .. }
        | RegistrationError::Io(error) => Error::Io(error),
    }
}

/// Maps a read or discovery refusal; `signed` selects HTTPS wording.
fn acquisition_error(error: AcquisitionError, signed: bool) -> Error {
    let message = match error {
        AcquisitionError::Cancelled => return Error::Cancelled,
        AcquisitionError::Trust(_) => "versioned registry trust refused",
        AcquisitionError::Checkpoint(error) => return checkpoint_error(&error),
        AcquisitionError::Location if signed => "HTTPS registry requires a safe URL ending in /",
        AcquisitionError::Location => "registry directory required",
        AcquisitionError::Path => "invalid registry artifact path",
        AcquisitionError::LocalIdentityChanged => "local registry identity changed",
        AcquisitionError::Escapes => "registry artifact escapes source",
        AcquisitionError::Transport => "registry HTTPS acquisition failed",
        AcquisitionError::Timeout => "registry acquisition timed out",
        AcquisitionError::Response => "registry requires an unencoded HTTP 200 response",
        AcquisitionError::TooLarge if signed => "registry response exceeds limits",
        AcquisitionError::TooLarge => "registry artifact unreadable or oversized",
        AcquisitionError::Key | AcquisitionError::Source(SourceError::Key) => {
            "invalid signature/key encoding"
        }
        AcquisitionError::RegistryId => "signed registry identity missing",
        AcquisitionError::Digest => "registry artifact digest mismatch",
        AcquisitionError::Source(SourceError::LocalPath) => "registry path must be UTF-8",
        AcquisitionError::Source(SourceError::Url) => {
            "HTTPS registry requires a safe URL ending in /"
        }
        AcquisitionError::Source(SourceError::Identity | SourceError::Trust) => {
            "registry identity, sequence or expiry invalid"
        }
        AcquisitionError::Catalog(error) => return catalog_error(&error),
        AcquisitionError::Io(error) => return Error::Io(error),
    };
    Error::Invalid(message)
}

fn catalog_error(error: &CatalogError) -> Error {
    Error::Invalid(match error {
        CatalogError::Trust(_) => "versioned registry trust refused",
        CatalogError::Limit => return policy::Error::Limit.into(),
        CatalogError::SignatureMissing => "registry signature missing",
        CatalogError::SignatureInvalid => "registry catalog signature invalid",
        CatalogError::Malformed => "invalid registry catalog",
        CatalogError::Identity
        | CatalogError::SequenceMissing
        | CatalogError::ExpiryInvalid
        | CatalogError::Expired => "registry identity, sequence or expiry invalid",
        CatalogError::DuplicateRelease | CatalogError::InvalidEntry => {
            "duplicate registry release or invalid digest"
        }
    })
}

pub(super) fn checkpoint_error(error: &CheckpointError) -> Error {
    match error {
        CheckpointError::Rollback => {
            Error::Invalid("registry rollback or same-sequence replacement")
        }
        CheckpointError::Replaced(_) => {
            Error::Invalid("registry replaced bytes for a retained release")
        }
        CheckpointError::Limit => policy::Error::Limit.into(),
        CheckpointError::InvalidRetention => Error::Invalid("invalid installed registry release"),
    }
}
