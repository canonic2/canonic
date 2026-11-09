//! Reviewed registry trust changes, independent of presentation and transport.
use crate::review::{
    Checkpoint, Context, Error, project,
    registries::{
        BUILTIN, addable, enrollment, global, global_file, merge, project as project_registrations,
        registration_error,
    },
    storage::Store,
};
use canonic_registry_kit::{acquisition::Fetcher, registrations::Registrations};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Clone, Copy)]
pub(in crate::review) enum Change<'a> {
    Add { name: &'a str, location: &'a str },
    Remove { name: &'a str },
    Default { name: &'a str },
}

pub(in crate::review) struct Prepared {
    root: Option<String>,
    global: Registrations,
    before: Registrations,
    after: Registrations,
    metadata: Option<Value>,
    receipt: Value,
}

impl Prepared {
    pub(in crate::review) fn new(
        context: Option<&Context>,
        fetcher: &Fetcher,
        change: Change<'_>,
    ) -> Result<Self, Error> {
        let global = global()?;
        let metadata = context
            .map(|c| project::checkpoint(c).map(|state| project::metadata(&state)))
            .transpose()?;
        let before = metadata
            .as_ref()
            .map_or_else(|| Ok(global.clone()), project_registrations)?;
        let mut after = before.clone();
        let (kind, alias, source) = match change {
            Change::Add { name, location } => {
                addable(&after, name)?;
                let source = enrollment(fetcher, location)?;
                after
                    .add(name, source.clone(), BUILTIN)
                    .map_err(registration_error)?;
                ("add", name, Some(source))
            }
            Change::Remove { name } => {
                let source = after.remove(name, BUILTIN).map_err(registration_error)?;
                ("remove", name, Some(source))
            }
            Change::Default { name } => {
                after
                    .set_default(name, &merge(&global, metadata.as_ref().map(|_| &before))?)
                    .map_err(registration_error)?;
                ("default", name, None)
            }
        };
        let resolved = |registrations: &Registrations| {
            (if metadata.is_some() {
                merge(&global, Some(registrations))
            } else {
                merge(registrations, None)
            })
            .map(|v| v.default_alias().to_owned())
        };
        let receipt = json!({"committed":true,"change":kind,"alias":alias,"scope":if metadata.is_some() {"project"} else {"user"},"source":source,
            "trust":source.as_ref().map(trust_review),
            "scope_effect":if metadata.is_some() {"Only this project's registry configuration changes."} else {"User-wide registry configuration changes; other projects using this user's sources are affected."},
            "default":{"before":resolved(&before)?,"after":resolved(&after)?},"installed_policy":"preserved"});
        Ok(Self {
            root: context.map(|c| c.project_root.clone()),
            global,
            before,
            after,
            metadata,
            receipt,
        })
    }

    pub(in crate::review) fn review(&self) -> Value {
        let mut value = self.receipt.clone();
        value["operation"] = json!("registry configuration");
        value
            .as_object_mut()
            .map(|fields| fields.remove("committed"));
        value
    }

    pub(in crate::review) fn commit(
        self,
        context: Option<&Context>,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        checkpoint()?;
        if context.map(|c| &c.project_root) != self.root.as_ref() {
            return Err(Error::StaleSnapshot);
        }
        if json!(global()?) != json!(self.global) {
            return Err(Error::StaleSnapshot);
        }
        let Some((context, before_meta)) = context.zip(self.metadata) else {
            global_file()?
                .replace(&self.before, &self.after)
                .map_err(registration_error)?;
            let mut receipt = self.receipt;
            receipt["registries"] = json!(self.after.sources);
            return Ok(receipt);
        };
        Store::open(Path::new(&context.project_root))?.update_checkpoint(
            context,
            checkpoint,
            |current| {
                if json!(global()?) != json!(self.global) {
                    return Err(Error::StaleSnapshot);
                }
                if project::metadata(current) != before_meta {
                    return Err(Error::StaleSnapshot);
                }
                current.operator["sources"] = json!(self.after.sources);
                current.operator["default"] = json!(self.after.default);
                let mut receipt = self.receipt;
                receipt["registries"] = current.operator["sources"].clone();
                Ok(receipt)
            },
        )
    }
}

/// Discovery proves catalog integrity under the presented key, not publisher
/// identity. Only the person's confirmation enrolls the retained origin/key.
fn trust_review(source: &canonic_registry_kit::source::Source) -> Value {
    let fingerprint = source
        .key()
        .and_then(|key| canonic_registry_kit::digest::decode(key, 32))
        .map(|key| canonic_registry_kit::digest::sha256_hex(&key));
    json!({
        "canonical_location":source.location(),
        "source_identity":source.identity(),
        "registry_identity":source.registry_id(),
        "signing_key_fingerprint_sha256":fingerprint,
        "basis":if source.is_signed() {
            "Trust on first use: trust this HTTPS origin, registry identity and signing key. The presented key verifies the catalog; this is not independent proof of publisher identity."
        } else {
            "Local operator-enrolled directory; unsigned catalog. Trust applies to this canonical directory."
        },
        "policy_effect":"Enrollment grants no pack permissions. Installing policy requires a separate review."
    })
}
